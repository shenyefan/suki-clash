//! TUN native settings and the final CV DNS/TUN composition step.
use crate::{
    preferences::{self, SettingsKind},
    storage::Values,
};
use serde_yaml_ng::{Mapping, Value};
pub(crate) const FIELDS: &[(&str, &str)] = &[
    ("IsTunEnabled", "enable"),
    ("StackIndex", "stack"),
    ("DeviceName", "device"),
    ("Mtu", "mtu"),
    ("AutoRoute", "auto-route"),
    ("AutoRedirect", "auto-redirect"),
    ("StrictRoute", "strict-route"),
    ("AutoDetectInterface", "auto-detect-interface"),
    ("RouteExcludeAddress", "route-exclude-address"),
    ("DnsHijack", "dns-hijack"),
];
pub(crate) fn project(native: &Mapping) -> Result<Values, String> {
    let mut values = preferences::defaults(SettingsKind::Tun);
    let Some(tun) = native.get("tun") else {
        return Ok(values);
    };
    let tun = tun.as_mapping().ok_or("tun 必须是对象")?;
    for (field, key) in FIELDS {
        let Some(value) = tun.get(*key) else { continue };
        let text = match *key {
            "enable"
            | "auto-route"
            | "auto-redirect"
            | "strict-route"
            | "auto-detect-interface" => value
                .as_bool()
                .ok_or_else(|| format!("tun.{key} 必须是布尔值"))?
                .to_string(),
            "mtu" => value.as_u64().ok_or("tun.mtu 必须是正整数")?.to_string(),
            "dns-hijack" | "route-exclude-address" => value
                .as_sequence()
                .ok_or_else(|| format!("tun.{key} 必须是列表"))?
                .iter()
                .map(|item| {
                    item.as_str()
                        .ok_or_else(|| format!("tun.{key} 条目必须是文本"))
                })
                .collect::<Result<Vec<_>, _>>()?
                .join("\n"),
            _ => value
                .as_str()
                .ok_or_else(|| format!("tun.{key} 必须是文本"))?
                .into(),
        };
        values.insert((*field).into(), text);
    }
    preferences::validate(SettingsKind::Tun, &values)?;
    if !cfg!(target_os = "linux")
        && values
            .get("AutoRedirect")
            .is_some_and(|value| value == "true")
    {
        return Err("自动重定向仅支持 Linux".into());
    }
    Ok(values)
}
pub(crate) fn apply(values: &Values, native: &mut Mapping) -> Result<(), String> {
    let mut merged = project(native)?;
    merged.extend(
        values
            .iter()
            .filter(|(field, _)| FIELDS.iter().any(|(name, _)| *name == field.as_str()))
            .map(|(key, value)| (key.clone(), value.clone())),
    );
    let encoded = preferences::choices::encode(merged);
    let values = &encoded;
    preferences::validate(SettingsKind::Tun, values)?;
    if !cfg!(target_os = "linux")
        && values
            .get("AutoRedirect")
            .is_some_and(|value| value == "true")
    {
        return Err("自动重定向仅支持 Linux".into());
    }
    let mut tun = native
        .get("tun")
        .and_then(Value::as_mapping)
        .cloned()
        .unwrap_or_default();
    for (field, key) in FIELDS {
        let Some(text) = values.get(*field) else {
            continue;
        };
        let value = match *key {
            "enable"
            | "auto-route"
            | "auto-redirect"
            | "strict-route"
            | "auto-detect-interface" => Value::Bool(text == "true"),
            "mtu" => Value::Number(
                text.parse::<u64>()
                    .map_err(|error| crate::logging::rejected("MTU 无效", error))?
                    .into(),
            ),
            "dns-hijack" | "route-exclude-address" => Value::Sequence(
                preferences::schema::split_entries(text)
                    .map(|item| Value::String(item.into()))
                    .collect(),
            ),
            _ => Value::String(text.clone()),
        };
        tun.insert((*key).into(), value);
    }
    native.insert("tun".into(), tun.into());
    Ok(())
}
pub(crate) fn enabled(config: &Value) -> Result<bool, String> {
    match config.get("tun").and_then(|tun| tun.get("enable")) {
        Some(value) => value
            .as_bool()
            .ok_or_else(|| "tun.enable 必须是布尔值".into()),
        None => Ok(false),
    }
}
/// Fill missing fake-IP options for TUN without replacing explicit DNS settings.
pub(crate) fn prepare(config: &mut Mapping) -> Result<(), String> {
    if !enabled(&Value::Mapping(config.clone()))? {
        return Ok(());
    }
    let mut dns = match config.get("dns") {
        Some(value) => value.as_mapping().cloned().ok_or("DNS 配置必须是对象")?,
        None => Mapping::new(),
    };
    if dns.get("enable").and_then(Value::as_bool) == Some(false) {
        return Ok(());
    }
    if dns
        .get("enhanced-mode")
        .and_then(Value::as_str)
        .is_none_or(|mode| mode == "fake-ip")
    {
        dns.entry("enable".into()).or_insert(true.into());
        dns.entry("enhanced-mode".into())
            .or_insert("fake-ip".into());
        dns.entry("fake-ip-range".into())
            .or_insert("198.18.0.1/16".into());
        if dns.get("ipv6").and_then(Value::as_bool) == Some(true) {
            dns.entry("fake-ip-range6".into())
                .or_insert("2001:2::0/64".into());
        }
    }
    // macOS otherwise keeps a LAN resolver outside the auto-routed TUN. Let
    // Mihomo own a temporary catch-all resolver instead of overwriting a
    // network service's persistent DNS settings. Explicit native choices stand.
    #[cfg(target_os = "macos")]
    if dns.get("enable").and_then(Value::as_bool) == Some(true)
        && dns.get("enhanced-mode").and_then(Value::as_str) == Some("fake-ip")
        && let Some(tun) = config.get_mut("tun").and_then(Value::as_mapping_mut)
        && tun
            .get("dns-mode")
            .is_none_or(|value| value.as_str() == Some(""))
    {
        tun.insert("dns-mode".into(), "native".into());
    }
    config.insert("dns".into(), dns.into());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn display_stack_choices_are_written_as_native_identifiers() {
        for (label, expected) in [
            ("System", "system"),
            ("gVisor", "gvisor"),
            ("Mixed", "mixed"),
            ("Mips", "mips"),
        ] {
            let mut native = Mapping::new();
            apply(
                &Values::from([("StackIndex".into(), label.into())]),
                &mut native,
            )
            .unwrap();
            assert_eq!(native["tun"]["stack"], expected);
            let root = tempfile::tempdir().unwrap();
            let paths = crate::storage::AppPaths::isolated(root.path());
            paths.create().unwrap();
            let preferences = super::super::Preferences::from_values(
                &Values::from([("StackIndex".into(), label.into())]),
                None,
            )
            .unwrap();
            preferences.save(&paths).unwrap();
            let loaded = super::super::Preferences::load(&paths).unwrap();
            assert_eq!(loaded.native()["tun"]["stack"], expected);
        }
    }

    #[test]
    fn native_fields_preserve_unknowns_and_partial_updates() {
        let mut native: Mapping = serde_yaml_ng::from_str(
            "tun: {enable: false, mtu: 1400, stack: mixed, endpoint-independent-nat: true}",
        )
        .unwrap();
        apply(
            &Values::from([("IsTunEnabled".into(), "true".into())]),
            &mut native,
        )
        .unwrap();
        assert_eq!(native["tun"]["mtu"], 1400);
        assert_eq!(native["tun"]["stack"], "mixed");
        assert_eq!(native["tun"]["endpoint-independent-nat"], true);
        assert_eq!(project(&native).unwrap()["IsTunEnabled"], "true");
        for yaml in [
            "tun: []",
            "tun: {enable: 'true'}",
            "tun: {mtu: -1}",
            "tun: {dns-hijack: [53]}",
        ] {
            assert!(
                project(&serde_yaml_ng::from_str(yaml).unwrap()).is_err(),
                "{yaml}"
            );
        }
    }
    #[test]
    fn dns_preparation_matches_cv_modes_and_preserves_explicit_ranges() {
        let mut config:Mapping=serde_yaml_ng::from_str("tun: {enable: true}\nipv6: false\ndns: {ipv6: true, fake-ip-range: 198.19.0.1/16, nameserver: ['1.1.1.1']}").unwrap();
        prepare(&mut config).unwrap();
        assert_eq!(config["dns"]["enable"], true);
        assert_eq!(config["dns"]["ipv6"], true);
        assert_eq!(config["dns"]["fake-ip-range"], "198.19.0.1/16");
        assert_eq!(config["dns"]["fake-ip-range6"], "2001:2::0/64");
        assert_eq!(config["dns"]["nameserver"][0], "1.1.1.1");
        let mut redir: Mapping = serde_yaml_ng::from_str(
            "tun: {enable: true}\ndns: {enable: false, enhanced-mode: redir-host}",
        )
        .unwrap();
        let before = redir.clone();
        prepare(&mut redir).unwrap();
        assert_eq!(redir, before);
        let mut disabled: Mapping = serde_yaml_ng::from_str("tun: {enable: false}").unwrap();
        prepare(&mut disabled).unwrap();
        assert!(disabled.get("dns").is_none());
    }
    #[test]
    fn tun_does_not_copy_top_level_ipv6_or_replace_explicit_dns_choices() {
        for global in [false, true] {
            for nested in [None, Some(false), Some(true)] {
                let mut config: Mapping = serde_yaml_ng::from_str(&format!(
                    "ipv6: {global}\ntun: {{enable: true}}\ndns: {{enable: true}}"
                ))
                .unwrap();
                if let Some(ipv6) = nested {
                    config["dns"]["ipv6"] = ipv6.into();
                }
                prepare(&mut config).unwrap();
                assert_eq!(config["ipv6"], global);
                assert_eq!(config["dns"].get("ipv6").and_then(Value::as_bool), nested);
                assert_eq!(
                    config["dns"].get("fake-ip-range6").is_some(),
                    nested == Some(true)
                );
            }
        }
        let mut config: Mapping = serde_yaml_ng::from_str(
            "ipv6: true\ntun: {enable: true}\ndns: {enable: false, ipv6: false}",
        )
        .unwrap();
        let before = config.clone();
        prepare(&mut config).unwrap();
        assert_eq!(config, before);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_fake_ip_uses_core_owned_dns_without_overwriting_native_choices() {
        for supplied in [None, Some("")] {
            let mut config: Mapping = serde_yaml_ng::from_str(
                "tun: {enable: true, dns-hijack: []}\ndns: {enhanced-mode: fake-ip}",
            )
            .unwrap();
            if let Some(mode) = supplied {
                config["tun"]["dns-mode"] = mode.into();
            }
            prepare(&mut config).unwrap();
            assert_eq!(config["tun"]["dns-mode"], "native");
            assert_eq!(config["tun"]["dns-hijack"], Value::Sequence(vec![]));
        }
        for mode in ["disabled", "native", "hijack"] {
            let mut config: Mapping = serde_yaml_ng::from_str(&format!(
                "tun: {{enable: true, dns-mode: {mode}}}\ndns: {{enhanced-mode: fake-ip}}"
            ))
            .unwrap();
            prepare(&mut config).unwrap();
            assert_eq!(config["tun"]["dns-mode"], mode);
        }
        for source in [
            "tun: {enable: false}\ndns: {enhanced-mode: fake-ip}",
            "tun: {enable: true}\ndns: {enable: false, enhanced-mode: redir-host}",
        ] {
            let mut config: Mapping = serde_yaml_ng::from_str(source).unwrap();
            prepare(&mut config).unwrap();
            assert!(config["tun"].get("dns-mode").is_none());
        }
    }
    #[test]
    fn repository_uses_only_native_tun_fields() {
        let temp = tempfile::tempdir().unwrap();
        let paths = crate::storage::AppPaths::isolated(temp.path());
        let repository = crate::storage::Repository::open(paths.clone()).unwrap();
        preferences::initialize(&repository).unwrap();
        let namespace = crate::storage::Namespace::parse("tun").unwrap();
        repository
            .save(&namespace, &Values::from([("Mtu".into(), "1400".into())]))
            .unwrap();
        assert_eq!(
            repository.read_limited(&namespace, 1024 * 1024).unwrap()["Mtu"],
            "1400"
        );
        let native: Value = serde_yaml_ng::from_slice(
            &std::fs::read(paths.settings().join("mihomo.yaml")).unwrap(),
        )
        .unwrap();
        assert_eq!(native["tun"]["mtu"], 1400);
        let app: serde_json::Value =
            serde_json::from_slice(&std::fs::read(paths.settings().join("settings.json")).unwrap())
                .unwrap();
        assert!(app.get("tun").is_none());
    }
}
