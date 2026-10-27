//! GeoData preferences use Mihomo's native keys; unknown URL entries are retained.
use crate::storage::Values;
use serde_yaml_ng::{Mapping, Value};

pub(crate) const DATABASES: [(&str, &str, &str, &str); 4] = [
    ("GeoIpUrl", "geoip", "GeoIP", "geoip.dat"),
    ("GeoSiteUrl", "geosite", "GeoSite", "geosite.dat"),
    ("MmdbUrl", "mmdb", "MMDB", "Country.mmdb"),
    ("AsnUrl", "asn", "ASN", "ASN.mmdb"),
];
pub(crate) fn apply(values: &Values, native: &mut Mapping) -> Result<(), String> {
    let mut urls = native
        .get("geox-url")
        .and_then(Value::as_mapping)
        .cloned()
        .unwrap_or_default();
    for (field, key, name, _) in DATABASES {
        if let Some(value) = values.get(field) {
            if value.is_empty() {
                urls.remove(key);
            } else {
                let valid = value.parse::<hyper::Uri>().ok().is_some_and(|uri| {
                    matches!(uri.scheme_str(), Some("http" | "https"))
                        && uri.host().is_some_and(|host| !host.is_empty())
                        && uri
                            .authority()
                            .is_some_and(|authority| !authority.as_str().contains('@'))
                });
                if !valid || value.len() > 4096 {
                    return Err(format!("{name} 地址无效"));
                }
                urls.insert(key.into(), value.as_str().into());
            }
        }
    }
    if !urls.is_empty() {
        native.insert("geox-url".into(), urls.into());
    } else {
        native.remove("geox-url");
    }
    if let Some(mode) = values.get("GeoMode") {
        let mode = match mode.as_str() {
            "db" => false,
            "dat" => true,
            _ => return Err("GeoData 格式无效".into()),
        };
        native.insert("geodata-mode".into(), mode.into());
    }
    if let Some(enabled) = values.get("GeoAutoUpdate") {
        let enabled = match enabled.as_str() {
            "true" => true,
            "false" => false,
            _ => return Err("自动更新状态无效".into()),
        };
        native.insert("geo-auto-update".into(), enabled.into());
    }
    if let Some(interval) = values.get("GeoUpdateIntervalValue") {
        let hours = interval
            .parse::<u32>()
            .ok()
            .filter(|hours| *hours > 0)
            .ok_or("更新间隔需大于 0 小时")?;
        native.insert("geo-update-interval".into(), hours.into());
    }
    Ok(())
}
pub(crate) fn read(native: &Mapping) -> Values {
    let mut values = Values::new();
    for (field, key, _, _) in DATABASES {
        if let Some(value) = native
            .get("geox-url")
            .and_then(|urls| urls.get(key))
            .and_then(Value::as_str)
        {
            values.insert(field.into(), value.into());
        }
    }
    if let Some(mode) = native.get("geodata-mode").and_then(Value::as_bool) {
        values.insert("GeoMode".into(), if mode { "dat" } else { "db" }.into());
    }
    if let Some(enabled) = native.get("geo-auto-update").and_then(Value::as_bool) {
        values.insert("GeoAutoUpdate".into(), enabled.to_string());
    }
    if let Some(hours) = native.get("geo-update-interval").and_then(Value::as_u64) {
        values.insert("GeoUpdateIntervalValue".into(), hours.to_string());
    }
    values
}
pub(crate) fn is_field(key: &str) -> bool {
    DATABASES.iter().any(|(field, _, _, _)| key == *field)
        || matches!(key, "GeoMode" | "GeoAutoUpdate" | "GeoUpdateIntervalValue")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edits_preserve_unknown_urls_and_roundtrip_native_types() {
        let mut native: Mapping =
            serde_yaml_ng::from_str("geox-url: {custom: keep, geoip: 'https://old.test/ip'}")
                .unwrap();
        let values = Values::from([
            ("GeoIpUrl".into(), "https://new.test/ip".into()),
            ("GeoMode".into(), "dat".into()),
            ("GeoAutoUpdate".into(), "true".into()),
            ("GeoUpdateIntervalValue".into(), "12".into()),
        ]);
        apply(&values, &mut native).unwrap();
        assert_eq!(native["geox-url"]["custom"], "keep");
        assert_eq!(native["geodata-mode"], true);
        assert_eq!(native["geo-update-interval"], 12);
        let loaded = read(&native);
        for (key, value) in values {
            assert_eq!(loaded[&key], value);
        }
        apply(&Values::from([("GeoIpUrl".into(), "".into())]), &mut native).unwrap();
        assert!(native["geox-url"].get("geoip").is_none());
        assert_eq!(native["geox-url"]["custom"], "keep");
    }
    #[test]
    fn invalid_resource_fields_are_rejected() {
        for (key, value) in [
            ("GeoIpUrl", "file:///tmp/ip"),
            ("AsnUrl", "https://user:secret@host/file"),
            ("GeoMode", "yaml"),
            ("GeoAutoUpdate", "1"),
            ("GeoUpdateIntervalValue", "0"),
        ] {
            assert!(
                apply(
                    &Values::from([(key.into(), value.into())]),
                    &mut Mapping::new()
                )
                .is_err()
            );
        }
    }
}

#[cfg(test)]
mod inheritance_tests {
    use crate::configuration::Preferences;
    use crate::storage::Values;
    use serde_yaml_ng::Mapping;
    #[test]
    fn unset_resources_inherit_source_and_url_edits_merge_by_database() {
        let preferences = Preferences::from_values(&Values::new(), None).unwrap();
        let mut config: Mapping = serde_yaml_ng::from_str(
            "geodata-mode: true\ngeox-url: {geosite: 'https://source.test/site', custom: keep}",
        )
        .unwrap();
        preferences.overlay(&mut config);
        assert_eq!(config["geodata-mode"], true);
        let changed = Preferences::from_values(
            &Values::from([("GeoIpUrl".into(), "https://user.test/ip".into())]),
            Some(&preferences),
        )
        .unwrap();
        changed.overlay(&mut config);
        assert_eq!(config["geox-url"]["geosite"], "https://source.test/site");
        assert_eq!(config["geox-url"]["custom"], "keep");
        assert_eq!(config["geox-url"]["geoip"], "https://user.test/ip");
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Database {
    GeoIp,
    GeoSite,
    Country,
    Asn,
}
impl Database {
    pub(crate) fn from_key(key: &str) -> Option<Self> {
        match key {
            "GeoIpUrl" => Some(Self::GeoIp),
            "GeoSiteUrl" => Some(Self::GeoSite),
            "MmdbUrl" => Some(Self::Country),
            "AsnUrl" => Some(Self::Asn),
            _ => None,
        }
    }
    pub(crate) fn spec(self) -> (&'static str, &'static str, &'static str, &'static str) {
        DATABASES[match self {
            Self::GeoIp => 0,
            Self::GeoSite => 1,
            Self::Country => 2,
            Self::Asn => 3,
        }]
    }
    pub(crate) const fn default_url(self) -> &'static str {
        match self {
            Self::GeoIp => {
                "https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geoip-lite.dat"
            }
            Self::GeoSite => {
                "https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geosite.dat"
            }
            Self::Country => {
                "https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geoip.metadb"
            }
            Self::Asn => {
                "https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/GeoLite2-ASN.mmdb"
            }
        }
    }
    pub(crate) fn validation_rule(self, bytes: &[u8]) -> Result<String, String> {
        match self {
            Self::GeoIp | Self::GeoSite => {
                let country = first_country(bytes).ok_or("GeoData 文件格式无效")?;
                Ok(format!(
                    "{},{country},DIRECT",
                    if self == Self::GeoIp {
                        "GEOIP"
                    } else {
                        "GEOSITE"
                    }
                ))
            }
            Self::Country | Self::Asn => {
                if !bytes
                    .windows(14)
                    .any(|part| part == b"\xab\xcd\xefMaxMind.com")
                {
                    return Err("MMDB 文件格式无效".into());
                }
                Ok(if self == Self::Country {
                    "GEOIP,CN,DIRECT"
                } else {
                    "IP-ASN,13335,DIRECT"
                }
                .into())
            }
        }
    }
}
fn first_country(mut bytes: &[u8]) -> Option<&str> {
    fn varint(bytes: &mut &[u8]) -> Option<u64> {
        let mut value = 0;
        for shift in (0..64).step_by(7) {
            let (&byte, tail) = bytes.split_first()?;
            *bytes = tail;
            if shift == 63 && byte > 1 {
                return None;
            }
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Some(value);
            }
        }
        None
    }
    if varint(&mut bytes)? != 10 {
        return None;
    }
    let length = usize::try_from(varint(&mut bytes)?).ok()?;
    let mut record = bytes.get(..length)?;
    if varint(&mut record)? != 10 {
        return None;
    }
    let length = usize::try_from(varint(&mut record)?).ok()?;
    let country = std::str::from_utf8(record.get(..length)?).ok()?;
    (!country.is_empty()
        && country.len() <= 128
        && country
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_@!+".contains(&byte)))
    .then_some(country)
}
