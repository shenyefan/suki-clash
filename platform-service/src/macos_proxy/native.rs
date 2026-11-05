//! Native SystemConfiguration adapter. Dictionary decoding and native writing
//! informed by sysproxy-rs (MIT), revision
//! c37a60e1fc9079cc0f074200e2296afd1705208c; see licenses/sysproxy-MIT.txt.
//! Only services touched by this process are retained for later cleanup.
use crate::proxy_owner::{
    Target,
    types::{Endpoint, OsSnapshot, PacProxy},
};
use std::collections::BTreeMap;
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
    sys::network_configuration::{SCNetworkProtocolGetConfiguration, SCNetworkServiceCopyProtocol},
};
#[derive(Clone)]
struct Service {
    id: String,
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
    Ok(Some(Service {
        id: id.into(),
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
pub(crate) fn active_service_id() -> Result<Option<String>, String> {
    Ok(active()?.map(|service| service.id))
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
    // Writes commit through another SCPreferences session. A retained
    // SCNetworkService still belongs to the old session and can return its cache.
    let current = self::service(&service.id)?.ok_or("目标网络服务已不存在")?;
    Ok(decode(&dictionary(&current)?))
}
pub(crate) fn read() -> Result<OsSnapshot, String> {
    read_service(&active()?.ok_or("没有活动网络服务")?)
}
pub(crate) struct Checkpoint {
    snapshot: OsSnapshot,
    service: Service,
}
pub(crate) fn capture() -> Result<(OsSnapshot, Checkpoint), String> {
    let service = active()?.ok_or("没有活动网络服务")?;
    let snapshot = read_service(&service)?;
    Ok((snapshot.clone(), Checkpoint { snapshot, service }))
}
fn write_snapshot(
    service: &Service,
    before: &OsSnapshot,
    after: &OsSnapshot,
    authorization: &mut Option<Authorization>,
    interactive: &mut bool,
) -> Result<(), String> {
    if crate::proxy_owner::equivalent(before, after) {
        return Ok(());
    }
    write_native(&service.id, after, authorization, interactive)
}

/// PAC is an SC protocol dictionary, not a shell URL argument. In particular,
/// removing its URL must remove the key instead of passing literal quotes to
/// networksetup (which rejects that argument on current macOS).
fn pac_dictionary(
    current: &CFDictionary<CFString, CFType>,
    url: &str,
    enabled: bool,
) -> CFDictionary<CFString, CFType> {
    let (keys, values) = current.get_keys_and_values();
    let mut pairs = keys
        .into_iter()
        .zip(values)
        .filter_map(|(key, value)| {
            let key = unsafe { CFString::wrap_under_get_rule(key as _) };
            (!matches!(
                key.to_string().as_str(),
                "ProxyAutoConfigURLString" | "ProxyAutoConfigEnable"
            ))
            .then(|| (key, unsafe { CFType::wrap_under_get_rule(value as _) }))
        })
        .collect::<Vec<_>>();
    pairs.push((
        CFString::new("ProxyAutoConfigEnable"),
        CFNumber::from(i32::from(enabled)).as_CFType(),
    ));
    if !url.is_empty() {
        pairs.push((
            CFString::new("ProxyAutoConfigURLString"),
            CFString::new(url).as_CFType(),
        ));
    }
    CFDictionary::from_CFType_pairs(&pairs)
}
fn proxy_dictionary(
    current: &CFDictionary<CFString, CFType>,
    snapshot: &OsSnapshot,
) -> CFDictionary<CFString, CFType> {
    let current = pac_dictionary(current, &snapshot.auto.url, snapshot.auto_switched_on);
    let (keys, values) = current.get_keys_and_values();
    let replaced = [
        "HTTPProxy",
        "HTTPPort",
        "HTTPEnable",
        "HTTPSProxy",
        "HTTPSPort",
        "HTTPSEnable",
        "SOCKSProxy",
        "SOCKSPort",
        "SOCKSEnable",
        "ExceptionsList",
        "ProxyAutoDiscoveryEnable",
    ];
    let mut pairs = keys
        .into_iter()
        .zip(values)
        .filter_map(|(key, value)| {
            let key = unsafe { CFString::wrap_under_get_rule(key as _) };
            (!replaced.contains(&key.to_string().as_str()))
                .then(|| (key, unsafe { CFType::wrap_under_get_rule(value as _) }))
        })
        .collect::<Vec<_>>();
    for (endpoint, host, port, enabled) in [
        (&snapshot.http, "HTTPProxy", "HTTPPort", "HTTPEnable"),
        (&snapshot.https, "HTTPSProxy", "HTTPSPort", "HTTPSEnable"),
        (&snapshot.socks, "SOCKSProxy", "SOCKSPort", "SOCKSEnable"),
    ] {
        pairs.push((
            CFString::new(host),
            CFString::new(&endpoint.host).as_CFType(),
        ));
        pairs.push((
            CFString::new(port),
            CFNumber::from(i32::from(endpoint.port)).as_CFType(),
        ));
        pairs.push((
            CFString::new(enabled),
            CFNumber::from(i32::from(endpoint.switched_on)).as_CFType(),
        ));
    }
    let bypass = crate::proxy_owner::entries(&snapshot.bypass)
        .into_iter()
        .map(CFString::new)
        .collect::<Vec<_>>();
    pairs.push((
        CFString::new("ExceptionsList"),
        CFArray::from_CFTypes(&bypass).as_CFType(),
    ));
    pairs.push((
        CFString::new("ProxyAutoDiscoveryEnable"),
        CFNumber::from(i32::from(snapshot.auto_detect)).as_CFType(),
    ));
    CFDictionary::from_CFType_pairs(&pairs)
}

// Owned by the serial proxy worker, including cleanup and rollback. Keeping the
// reference lets macOS reuse rights granted for this authorization session.
struct Authorization(security_framework_sys::authorization::AuthorizationRef);
impl Authorization {
    fn new() -> Result<Self, String> {
        use security_framework_sys::authorization::{
            AuthorizationCreate, kAuthorizationFlagDefaults,
        };
        let mut authorization = std::ptr::null_mut();
        let status = unsafe {
            AuthorizationCreate(
                std::ptr::null(),
                std::ptr::null(),
                kAuthorizationFlagDefaults,
                &mut authorization,
            )
        };
        if status != 0 {
            return Err(format!("无法取得系统代理授权（{status}）"));
        }
        Ok(Self(authorization))
    }
}
impl Drop for Authorization {
    fn drop(&mut self) {
        use security_framework_sys::authorization::{
            AuthorizationFree, kAuthorizationFlagDefaults,
        };
        unsafe {
            AuthorizationFree(self.0, kAuthorizationFlagDefaults);
        }
    }
}

fn write_native(
    id: &str,
    snapshot: &OsSnapshot,
    authorization: &mut Option<Authorization>,
    interactive: &mut bool,
) -> Result<(), String> {
    use system_configuration::sys::{
        network_configuration::SCNetworkProtocolSetConfiguration,
        preferences::{
            SCPreferencesApplyChanges, SCPreferencesCommitChanges,
            SCPreferencesCreateWithAuthorization, SCPreferencesLock, SCPreferencesUnlock,
        },
    };
    unsafe extern "C" {
        fn SCError() -> i32;
        fn SCErrorString(status: i32) -> *const std::ffi::c_char;
    }
    fn failure(stage: &str) -> String {
        let code = unsafe { SCError() };
        let pointer = unsafe { SCErrorString(code) };
        let reason = if pointer.is_null() {
            String::new()
        } else {
            unsafe { std::ffi::CStr::from_ptr(pointer) }
                .to_string_lossy()
                .into_owned()
        };
        format!("{stage}：{reason}（{code}）")
    }
    let authorization = match authorization {
        Some(authorization) => authorization,
        slot @ None => slot.insert(Authorization::new()?),
    };
    let raw = unsafe {
        SCPreferencesCreateWithAuthorization(
            std::ptr::null(),
            CFString::new("Suki Clash system proxy").as_concrete_TypeRef(),
            std::ptr::null(),
            authorization.0.cast(),
        )
    };
    if raw.is_null() {
        return Err(failure("无法打开系统代理配置"));
    }
    let preferences = unsafe { SCPreferences::wrap_under_create_rule(raw) };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        if unsafe { SCPreferencesLock(raw, 0) } != 0 {
            break;
        }
        if unsafe { SCError() } == 1003 && *interactive {
            use security_framework_sys::authorization::{
                AuthorizationCopyRights, AuthorizationItem, AuthorizationRights,
                kAuthorizationFlagExtendRights, kAuthorizationFlagInteractionAllowed,
            };
            *interactive = false;
            let mut item = AuthorizationItem {
                name: c"system.preferences.network".as_ptr(),
                valueLength: 0,
                value: std::ptr::null_mut(),
                flags: 0,
            };
            let rights = AuthorizationRights {
                count: 1,
                items: &mut item,
            };
            let status = unsafe {
                AuthorizationCopyRights(
                    authorization.0,
                    &rights,
                    std::ptr::null(),
                    kAuthorizationFlagInteractionAllowed | kAuthorizationFlagExtendRights,
                    std::ptr::null_mut(),
                )
            };
            if status != 0 {
                return Err(format!("系统代理授权未完成（{status}）"));
            }
            continue;
        }
        if unsafe { SCError() } != 3002 || std::time::Instant::now() >= deadline {
            return Err(failure("无法锁定系统代理配置"));
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let result = (|| {
        let native = SCNetworkService::get_services(&preferences)
            .iter()
            .find(|native| native.id().is_some_and(|value| value == id))
            .map(|native| native.clone())
            .ok_or("目标网络服务已不存在")?;
        let protocol = unsafe {
            SCNetworkServiceCopyProtocol(
                native.as_concrete_TypeRef(),
                CFString::new("Proxies").as_concrete_TypeRef(),
            )
        };
        if protocol.is_null() {
            return Err("网络服务未提供代理设置".into());
        }
        let config = unsafe { SCNetworkProtocolGetConfiguration(protocol) };
        let current = if config.is_null() {
            CFDictionary::from_CFType_pairs(&[])
        } else {
            unsafe { CFDictionary::wrap_under_get_rule(config as _) }
        };
        let candidate = proxy_dictionary(&current, snapshot);
        let updated = unsafe {
            SCNetworkProtocolSetConfiguration(protocol, candidate.as_concrete_TypeRef() as _)
        };
        unsafe {
            CFRelease(protocol.cast());
        }
        if updated == 0 {
            return Err(failure("无法设置系统代理"));
        }
        if unsafe { SCPreferencesCommitChanges(raw) } == 0 {
            return Err(failure("无法保存系统代理"));
        }
        if unsafe { SCPreferencesApplyChanges(raw) } == 0 {
            return Err(failure("系统代理已保存但系统未应用"));
        }
        Ok(())
    })();
    unsafe {
        SCPreferencesUnlock(raw);
    }
    result
}
fn restore(
    checkpoint: &Checkpoint,
    authorization: &mut Option<Authorization>,
    interactive: &mut bool,
) -> Result<(), String> {
    let service = service(&checkpoint.service.id)?.ok_or("待恢复的网络服务已不存在")?;
    write_snapshot(
        &service,
        &read_service(&service)?,
        &checkpoint.snapshot,
        authorization,
        interactive,
    )
}
#[derive(Default)]
pub(crate) struct Adapter {
    authorization: Option<Authorization>,
    interactive: bool,
    owned: BTreeMap<String, Vec<Target>>,
    pending: Option<String>,
    journal: Option<std::path::PathBuf>,
}
impl Adapter {
    pub(crate) fn available(&self) -> Result<bool, String> {
        Ok(active()?.is_some())
    }
    pub(crate) fn allow_interaction(&mut self, allowed: bool) {
        self.interactive = allowed;
    }
    pub(crate) fn restore(&mut self, checkpoint: &Checkpoint) -> Result<(), String> {
        restore(checkpoint, &mut self.authorization, &mut self.interactive)
    }
    pub(crate) fn new(
        journal: Option<std::path::PathBuf>,
        interactive: bool,
    ) -> Result<Self, String> {
        let owned = if let Some(path) = &journal {
            match std::fs::read(path) {
                Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| {
                    format!("{}：系统代理恢复记录损坏，未覆盖原文件", path.display())
                })?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
                Err(error) => return Err(format!("无法读取系统代理恢复记录：{error}")),
            }
        } else {
            BTreeMap::new()
        };
        let mut adapter = Self {
            authorization: None,
            owned,
            interactive,
            pending: None,
            journal,
        };
        if !adapter.owned.is_empty() {
            adapter
                .clear(None)
                .map_err(|error| format!("上次退出的系统代理恢复未完成：{error}"))?;
        }
        Ok(adapter)
    }
    fn persist(&self) -> Result<(), String> {
        if let Some(path) = &self.journal {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            atomic_write(
                path,
                &serde_json::to_vec(&self.owned).map_err(|_| "无法编码系统代理恢复记录")?,
            )
            .map_err(|error| format!("无法保存系统代理恢复记录：{error}"))?;
        }
        Ok(())
    }

    pub(crate) fn capture(&mut self) -> Result<(OsSnapshot, Checkpoint), String> {
        let (snapshot, checkpoint) = capture()?;
        self.pending = Some(checkpoint.service.id.clone());
        Ok((snapshot, checkpoint))
    }
    pub(crate) fn write(&mut self, target: &Target) -> Result<(), String> {
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
        self.persist()?;
        let mut after = before.clone();
        for endpoint in [&mut after.http, &mut after.https, &mut after.socks] {
            endpoint.switched_on = target.system.enable;
            endpoint.enable = target.system.enable;
            if target.system.enable {
                endpoint.host = target.system.host.clone();
                endpoint.port = target.system.port;
            }
        }
        if target.system.enable || target.auto.enable {
            after.bypass = target.system.bypass.clone();
        }
        after.auto_switched_on = target.auto.enable;
        after.auto.enable = target.auto.enable;
        if target.auto.enable {
            after.auto.url = target.auto.url.clone();
        }
        after.auto_detect = false;
        write_snapshot(
            &service,
            &before,
            &after,
            &mut self.authorization,
            &mut self.interactive,
        )
    }
    pub(crate) fn clear(&mut self, target: Option<&Target>) -> Result<OsSnapshot, String> {
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
                write_snapshot(
                    &service,
                    &before,
                    &after,
                    &mut self.authorization,
                    &mut self.interactive,
                )?;
                let observed = read_service(&service)?;
                if !crate::proxy_owner::equivalent(&after, &observed) {
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
        if let Err(error) = self.persist() {
            failures.push(error);
        }
        if !failures.is_empty() {
            return Err(failures.join("；"));
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

fn atomic_write(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    let temporary = path.with_extension("json.tmp");
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    std::fs::rename(temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn one_native_dictionary_covers_manual_pac_and_cleanup_without_losing_unowned_keys() {
        let current = CFDictionary::from_CFType_pairs(&[(
            CFString::new("ExcludeSimpleHostnames"),
            CFNumber::from(1).as_CFType(),
        )]);
        let endpoint = Endpoint {
            host: "127.0.0.1".into(),
            port: 7897,
            enable: true,
            switched_on: true,
        };
        let mut snapshot = OsSnapshot {
            http: endpoint.clone(),
            https: endpoint.clone(),
            socks: endpoint,
            bypass: "localhost,内网.local".into(),
            ..Default::default()
        };
        let manual = proxy_dictionary(&current, &snapshot);
        assert!(crate::proxy_owner::equivalent(&snapshot, &decode(&manual)));
        for endpoint in [&mut snapshot.http, &mut snapshot.https, &mut snapshot.socks] {
            endpoint.enable = false;
            endpoint.switched_on = false;
        }
        snapshot.auto = PacProxy {
            enable: true,
            url: "http://127.0.0.1:53535/proxy.pac".into(),
        };
        snapshot.auto_switched_on = true;
        let pac = proxy_dictionary(&manual, &snapshot);
        assert!(crate::proxy_owner::equivalent(&snapshot, &decode(&pac)));
        snapshot.auto_switched_on = false;
        snapshot.auto.enable = false;
        snapshot.auto.url.clear();
        let cleared = proxy_dictionary(&pac, &snapshot);
        assert!(decode(&cleared).is_all_disabled());
        assert!(
            cleared
                .find(CFString::new("ProxyAutoConfigURLString"))
                .is_none()
        );
        assert!(
            cleared
                .find(CFString::new("ExcludeSimpleHostnames"))
                .is_some()
        );
    }
    #[test]
    fn native_pac_update_preserves_other_proxy_fields_and_removes_empty_url() {
        let before = CFDictionary::from_CFType_pairs(&[
            (
                CFString::new("HTTPProxy"),
                CFString::new("keep").as_CFType(),
            ),
            (
                CFString::new("ProxyAutoConfigURLString"),
                CFString::new("http://old.test/proxy.pac").as_CFType(),
            ),
        ]);
        let updated = pac_dictionary(&before, "http://127.0.0.1:53111/proxy.pac", true);
        assert_eq!(
            decode(&updated).auto.url,
            "http://127.0.0.1:53111/proxy.pac"
        );
        assert!(decode(&updated).auto_switched_on);
        assert_eq!(decode(&updated).http.host, "keep");
        let cleared = pac_dictionary(&updated, "", false);
        assert!(
            cleared
                .find(CFString::new("ProxyAutoConfigURLString"))
                .is_none()
        );
        assert!(!decode(&cleared).auto_switched_on);
    }
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
            system: crate::proxy_owner::types::ManualProxy {
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
}
