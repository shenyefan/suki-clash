//! Native SystemConfiguration/networksetup adapter. Dictionary decoding and
//! networksetup handling adapted from sysproxy-rs (MIT), revision
//! c37a60e1fc9079cc0f074200e2296afd1705208c; see licenses/sysproxy-MIT.txt.
//! Only services touched by this process are retained for later cleanup.
use crate::system_proxy::{
    Target,
    types::{Endpoint, OsSnapshot, PacProxy},
};
use std::{
    collections::BTreeMap,
    process::{Command, Output},
};
use system_configuration::{
    core_foundation::{
        array::CFArray,
        base::{CFRelease, CFType, TCFType},
        dictionary::CFDictionary,
        number::CFNumber,
        string::CFString,
    },
    dynamic_store::SCDynamicStoreBuilder,
    network_configuration::SCNetworkService,
    preferences::SCPreferences,
    sys::network_configuration::{
        SCNetworkProtocolGetConfiguration, SCNetworkServiceCopyProtocol, SCNetworkServiceGetName,
    },
};
#[derive(Clone)]
struct Service {
    id: String,
    name: String,
    native: SCNetworkService,
}
fn service(id: &str) -> Result<Option<Service>, String> {
    let preferences = SCPreferences::default(&CFString::new("suki-clash-system-proxy"));
    let services = SCNetworkService::get_services(&preferences);
    let Some(native) = services
        .iter()
        .find(|s| s.id().is_some_and(|value| value == id))
    else {
        return Ok(None);
    };
    let native = native.clone();
    let pointer = unsafe { SCNetworkServiceGetName(native.as_concrete_TypeRef()) };
    if pointer.is_null() {
        return Err("网络服务名称不可用".into());
    }
    Ok(Some(Service {
        id: id.into(),
        name: unsafe { CFString::wrap_under_get_rule(pointer) }.to_string(),
        native,
    }))
}
fn active() -> Result<Option<Service>, String> {
    let store = SCDynamicStoreBuilder::new("suki-clash-system-proxy")
        .build()
        .ok_or("无法读取系统网络服务")?;
    for family in ["IPv4", "IPv6"] {
        if let Some(global) = store
            .get(CFString::new(&format!("State:/Network/Global/{family}")))
            .and_then(|v| v.downcast_into::<CFDictionary>())
        {
            let key = CFString::new("PrimaryService");
            if let Some(pointer) = global.find(key.as_CFTypeRef() as *const _)
                && let Some(id) = unsafe { CFType::wrap_under_get_rule(*pointer as _) }
                    .downcast_into::<CFString>()
            {
                return service(&id.to_string())?
                    .map(Some)
                    .ok_or_else(|| "活动网络服务已不存在".into());
            }
        }
    }
    Ok(None)
}
fn dictionary(service: &Service) -> Result<CFDictionary<CFString, CFType>, String> {
    // CopyProtocol is owned; the dictionary is retained before releasing it.
    unsafe {
        let protocol = SCNetworkServiceCopyProtocol(
            service.native.as_concrete_TypeRef(),
            CFString::new("Proxies").as_concrete_TypeRef(),
        );
        if protocol.is_null() {
            return Err("网络服务未提供代理设置".into());
        }
        let config = SCNetworkProtocolGetConfiguration(protocol);
        let result = if config.is_null() {
            Ok(CFDictionary::from_CFType_pairs(&[]))
        } else {
            Ok(CFDictionary::wrap_under_get_rule(config as _))
        };
        CFRelease(protocol.cast());
        result
    }
}
fn decode(dict: &CFDictionary<CFString, CFType>) -> OsSnapshot {
    let number = |key: &str| {
        dict.find(CFString::new(key))
            .and_then(|v| v.downcast::<CFNumber>())
            .and_then(|v| v.to_i32())
            .unwrap_or(0)
    };
    let string = |key: &str| {
        dict.find(CFString::new(key))
            .and_then(|v| v.downcast::<CFString>())
            .map(|v| v.to_string())
            .unwrap_or_default()
    };
    let endpoint = |protocol: &str| {
        let switched_on = number(&format!("{protocol}Enable")) != 0;
        let host = string(&format!("{protocol}Proxy"));
        let port = u16::try_from(number(&format!("{protocol}Port"))).unwrap_or(0);
        Endpoint {
            enable: switched_on && !host.is_empty() && port > 0,
            switched_on,
            host,
            port,
        }
    };
    let bypass = dict
        .find(CFString::new("ExceptionsList"))
        .and_then(|v| v.downcast::<CFArray>())
        .map(|values| {
            values
                .iter()
                .filter_map(|value| {
                    unsafe { CFType::wrap_under_get_rule(*value as _) }.downcast_into::<CFString>()
                })
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default();
    let url = string("ProxyAutoConfigURLString");
    // networksetup represents an empty PAC URL with this literal argument.
    let url = if url == "\"\"" { String::new() } else { url };
    let auto_switched_on = number("ProxyAutoConfigEnable") != 0;
    OsSnapshot {
        http: endpoint("HTTP"),
        https: endpoint("HTTPS"),
        socks: endpoint("SOCKS"),
        auto: PacProxy {
            enable: auto_switched_on && !url.is_empty(),
            url,
        },
        auto_switched_on,
        bypass,
        auto_detect: number("ProxyAutoDiscoveryEnable") != 0,
    }
}
fn read_service(service: &Service) -> Result<OsSnapshot, String> {
    // networksetup commits through another SCPreferences session. A retained
    // SCNetworkService still belongs to the old session and can return its cache.
    let current = self::service(&service.id)?.ok_or("目标网络服务已不存在")?;
    Ok(decode(&dictionary(&current)?))
}
pub(super) fn read() -> Result<OsSnapshot, String> {
    read_service(&active()?.ok_or("没有活动网络服务")?)
}
pub(in crate::system_proxy) struct Checkpoint {
    snapshot: OsSnapshot,
    service: Service,
}
pub(super) fn capture() -> Result<(OsSnapshot, Checkpoint), String> {
    let service = active()?.ok_or("没有活动网络服务")?;
    let snapshot = read_service(&service)?;
    Ok((snapshot.clone(), Checkpoint { snapshot, service }))
}
fn check_output(action: &str, output: Output) -> Result<(), String> {
    if output.status.success() {
        return Ok(());
    }
    if output.status.code() == Some(14)
        || [output.stdout.as_slice(), output.stderr.as_slice()]
            .iter()
            .any(|bytes| String::from_utf8_lossy(bytes).contains("requires admin privileges"))
    {
        return Err("修改系统代理需要管理员权限".into());
    }
    Err(format!("networksetup {action} 未完成（{}）", output.status))
}
fn run(args: &[&str]) -> Result<(), String> {
    let output = Command::new("/usr/sbin/networksetup")
        .args(args)
        .output()
        .map_err(|error| format!("无法执行 networksetup：{error}"))?;
    check_output(args[0], output)
}
fn write_snapshot(
    service: &Service,
    before: &OsSnapshot,
    after: &OsSnapshot,
) -> Result<(), String> {
    for (old, new, set, state) in [
        (
            &before.http,
            &after.http,
            "-setwebproxy",
            "-setwebproxystate",
        ),
        (
            &before.https,
            &after.https,
            "-setsecurewebproxy",
            "-setsecurewebproxystate",
        ),
        (
            &before.socks,
            &after.socks,
            "-setsocksfirewallproxy",
            "-setsocksfirewallproxystate",
        ),
    ] {
        if old == new {
            continue;
        }
        if new.switched_on && (old.host != new.host || old.port != new.port || !old.switched_on) {
            run(&[set, &service.name, &new.host, &new.port.to_string()])?;
        }
        run(&[
            state,
            &service.name,
            if new.switched_on { "on" } else { "off" },
        ])?;
    }
    if before.bypass != after.bypass {
        let mut args = vec!["-setproxybypassdomains", service.name.as_str()];
        let bypass = super::super::entries(&after.bypass);
        if bypass.is_empty() {
            args.push("Empty");
        } else {
            args.extend(bypass);
        }
        run(&args)?;
    }
    if before.auto.url != after.auto.url {
        run(&[
            "-setautoproxyurl",
            &service.name,
            if after.auto.url.is_empty() {
                "\"\""
            } else {
                &after.auto.url
            },
        ])?;
    }
    if before.auto_switched_on != after.auto_switched_on || before.auto.url != after.auto.url {
        run(&[
            "-setautoproxystate",
            &service.name,
            if after.auto_switched_on { "on" } else { "off" },
        ])?;
    }
    if before.auto_detect != after.auto_detect {
        run(&[
            "-setproxyautodiscovery",
            &service.name,
            if after.auto_detect { "on" } else { "off" },
        ])?;
    }
    Ok(())
}
pub(super) fn restore(checkpoint: &Checkpoint) -> Result<(), String> {
    let service = service(&checkpoint.service.id)?.ok_or("待恢复的网络服务已不存在")?;
    write_snapshot(&service, &read_service(&service)?, &checkpoint.snapshot)
}
#[derive(Default)]
pub(super) struct Adapter {
    owned: BTreeMap<String, Vec<Target>>,
    pending: Option<String>,
}
impl Adapter {
    pub(super) fn capture(&mut self) -> Result<(OsSnapshot, Checkpoint), String> {
        let (snapshot, checkpoint) = capture()?;
        self.pending = Some(checkpoint.service.id.clone());
        Ok((snapshot, checkpoint))
    }
    pub(super) fn write(&mut self, target: &Target) -> Result<(), String> {
        if !target.system.enable && !target.auto.enable {
            self.clear(None)?;
        }
        let service = service(
            self.pending
                .as_deref()
                .ok_or("系统代理操作缺少网络服务身份")?,
        )?
        .ok_or("目标网络服务已不存在")?;
        let before = read_service(&service)?;
        if target.system.enable || target.auto.enable {
            let previous = self.owned.entry(service.id.clone()).or_default();
            previous.retain(|old| old.owns(&before));
            if !previous.contains(target) {
                previous.push(target.clone());
            }
        }
        let mut after = before.clone();
        for endpoint in [&mut after.http, &mut after.https, &mut after.socks] {
            endpoint.switched_on = target.system.enable;
            endpoint.enable = target.system.enable;
            if target.system.enable {
                endpoint.host = target.system.host.clone();
                endpoint.port = target.system.port;
            }
        }
        if target.system.enable {
            after.bypass = target.system.bypass.clone();
        }
        after.auto_switched_on = target.auto.enable;
        after.auto.enable = target.auto.enable;
        if target.auto.enable {
            after.auto.url = target.auto.url.clone();
        }
        after.auto_detect = false;
        write_snapshot(&service, &before, &after)
    }
    pub(super) fn clear(&mut self, target: Option<&Target>) -> Result<OsSnapshot, String> {
        if let (Some(target), Some(service)) = (target, active()?)
            && target.owns(&read_service(&service)?)
        {
            self.owned
                .entry(service.id)
                .or_default()
                .push(target.clone());
        }
        let mut failures = Vec::new();
        for (id, targets) in self.owned.clone() {
            let result = (|| {
                let Some(service) = service(&id)? else {
                    return Ok(());
                };
                let before = read_service(&service)?;
                let after = without_owned(&before, &targets);
                write_snapshot(&service, &before, &after)?;
                let observed = read_service(&service)?;
                if !super::super::equivalent(&after, &observed) {
                    return Err("网络服务代理清理后回读不一致".into());
                }
                Ok::<(), String>(())
            })();
            match result {
                Ok(()) => {
                    self.owned.remove(&id);
                }
                Err(error) => failures.push(format!("网络服务 {id}：{error}")),
            }
        }
        if !failures.is_empty() {
            return Err(crate::failure::join(&failures));
        }
        match active()? {
            Some(service) => read_service(&service),
            None => Ok(OsSnapshot::default()),
        }
    }
}

fn without_owned(before: &OsSnapshot, targets: &[Target]) -> OsSnapshot {
    let mut after = before.clone();
    for endpoint in [&mut after.http, &mut after.https, &mut after.socks] {
        if endpoint.switched_on
            && targets.iter().any(|target| {
                endpoint.host == target.system.host && endpoint.port == target.system.port
            })
        {
            endpoint.switched_on = false;
            endpoint.enable = false;
        }
    }
    if after.auto_switched_on
        && targets
            .iter()
            .any(|target| target.auto.enable && target.auto.url == after.auto.url)
    {
        after.auto_switched_on = false;
        after.auto.enable = false;
    }
    after
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_switches_and_unusable_endpoints_remain_distinct() {
        for port in [-1, i32::MAX] {
            let dict = CFDictionary::from_CFType_pairs(&[
                (CFString::new("HTTPEnable"), CFNumber::from(1).as_CFType()),
                (
                    CFString::new("HTTPProxy"),
                    CFString::new("localhost").as_CFType(),
                ),
                (CFString::new("HTTPPort"), CFNumber::from(port).as_CFType()),
                (
                    CFString::new("ProxyAutoDiscoveryEnable"),
                    CFNumber::from(1).as_CFType(),
                ),
            ]);
            let state = decode(&dict);
            assert!(state.http.switched_on);
            assert!(!state.http.enable);
            assert_eq!(state.http.port, 0);
            assert!(state.auto_detect);
            assert!(!state.is_all_disabled());
        }
    }
    #[test]
    fn pac_and_unicode_bypass_are_read_from_the_same_dictionary() {
        let bypass =
            CFArray::from_CFTypes(&[CFString::new("localhost"), CFString::new("内网.local")]);
        let dict = CFDictionary::from_CFType_pairs(&[
            (
                CFString::new("ProxyAutoConfigEnable"),
                CFNumber::from(1).as_CFType(),
            ),
            (
                CFString::new("ProxyAutoConfigURLString"),
                CFString::new("http://127.0.0.1:1234/proxy.pac").as_CFType(),
            ),
            (CFString::new("ExceptionsList"), bypass.as_CFType()),
        ]);
        let state = decode(&dict);
        assert!(state.auto.enable);
        assert_eq!(state.bypass, "localhost,内网.local");
        assert!(state.matches_pac(&state.auto));
    }
    #[test]
    fn cleanup_removes_only_owned_protocols_across_attempted_targets() {
        let own = Target {
            system: crate::system_proxy::types::ManualProxy {
                host: "127.0.0.1".into(),
                port: 7897,
                enable: true,
                ..Default::default()
            },
            auto: PacProxy {
                enable: true,
                url: "http://127.0.0.1:1234/proxy.pac".into(),
            },
        };
        let mut next = own.clone();
        next.system.port = 7899;
        let endpoint = |port| Endpoint {
            host: "127.0.0.1".into(),
            port,
            enable: true,
            switched_on: true,
        };
        let before = OsSnapshot {
            http: endpoint(7897),
            https: endpoint(7899),
            socks: Endpoint {
                host: "external.example".into(),
                port: 1080,
                enable: true,
                switched_on: true,
            },
            bypass: "localhost".into(),
            auto: own.auto.clone(),
            auto_switched_on: true,
            auto_detect: true,
        };
        let after = without_owned(&before, &[own, next]);
        assert!(!after.http.switched_on);
        assert!(!after.https.switched_on);
        assert!(!after.auto_switched_on);
        assert_eq!(after.socks, before.socks);
        assert_eq!(after.bypass, before.bypass);
        assert!(after.auto_detect);
    }
    #[test]
    fn native_failures_keep_status_and_do_not_expose_arguments() {
        use std::os::unix::process::ExitStatusExt;
        let error = check_output(
            "-setwebproxy",
            Output {
                status: std::process::ExitStatus::from_raw(14 << 8),
                stdout: vec![],
                stderr: vec![],
            },
        )
        .unwrap_err();
        assert!(error.contains("管理员权限"));
        assert!(
            check_output(
                "-setwebproxy",
                Output {
                    status: std::process::ExitStatus::from_raw(0),
                    stdout: vec![0xff],
                    stderr: vec![]
                }
            )
            .is_ok()
        );
    }
}
