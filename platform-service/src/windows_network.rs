//! Native AppContainer loopback workflow, following ClashSuki's service-owned writes.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct UwpApp {
    pub sid: String,
    pub name: String,
    pub package: String,
    pub exempt: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Request {
    List,
    SetLoopback {
        expected: Vec<String>,
        selected: Vec<String>,
    },
    RebuildFirewall,
}
#[cfg(any(windows, test))]
fn network_failure(code: Option<u32>, detail: &str) -> String {
    if matches!(code, Some(1753 | 0x800706d9)) {
        format!(
            "Windows 网络接口未注册 RPC 端点，请检查 Windows Defender Firewall（MpsSvc）、Base Filtering Engine（BFE）及 RPC 相关服务是否正常运行；{detail}"
        )
    } else {
        detail.to_owned()
    }
}
#[cfg(windows)]
mod native {
    use super::*;
    use anyhow::{Result, ensure};
    use std::collections::{BTreeMap, BTreeSet};
    #[cfg(feature = "standalone")]
    use windows_sys::Win32::Security::{Authorization::ConvertStringSidToSidW, SID_AND_ATTRIBUTES};
    use windows_sys::Win32::{
        Foundation::LocalFree,
        NetworkManagement::WindowsFirewall::*,
        Security::Authorization::ConvertSidToStringSidW,
        System::Memory::{GetProcessHeap, HeapFree},
        UI::Shell::SHLoadIndirectString,
    };
    fn status(code: u32) -> Result<()> {
        #[cfg(any(feature = "client", feature = "standalone"))]
        if code != 0 {
            log::error!(
                "{}",
                super::network_failure(
                    Some(code),
                    &std::io::Error::from_raw_os_error(code as i32).to_string()
                )
            );
        }
        ensure!(code == 0, "Windows 网络操作失败（{code}）");
        Ok(())
    }
    unsafe fn text(pointer: *const u16) -> String {
        if pointer.is_null() {
            return String::new();
        }
        let mut size = 0;
        while unsafe { *pointer.add(size) } != 0 {
            size += 1;
        }
        String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(pointer, size) })
    }
    unsafe fn sid(pointer: *mut std::ffi::c_void) -> Result<String> {
        let mut value = std::ptr::null_mut();
        ensure!(
            unsafe { ConvertSidToStringSidW(pointer, &mut value) } != 0,
            "无法读取 AppContainer SID"
        );
        let result = unsafe { text(value) };
        unsafe { LocalFree(value.cast()) };
        Ok(result)
    }
    fn indirect(value: String) -> String {
        if !value.starts_with('@') {
            return value;
        }
        let source = value.encode_utf16().chain([0]).collect::<Vec<_>>();
        let mut output = vec![0; 2048];
        if unsafe {
            SHLoadIndirectString(
                source.as_ptr(),
                output.as_mut_ptr(),
                output.len() as u32,
                std::ptr::null(),
            )
        } >= 0
        {
            unsafe { text(output.as_ptr()) }
        } else {
            value
        }
    }
    fn exemptions() -> Result<BTreeSet<String>> {
        let mut count = 0;
        let mut items = std::ptr::null_mut();
        status(unsafe { NetworkIsolationGetAppContainerConfig(&mut count, &mut items) })?;
        let result = (|| {
            let mut values = BTreeSet::new();
            for ix in 0..count as usize {
                values.insert(unsafe { sid((*items.add(ix)).Sid) }?);
            }
            Ok(values)
        })();
        if !items.is_null() {
            unsafe {
                let heap = GetProcessHeap();
                for ix in 0..count as usize {
                    HeapFree(heap, 0, (*items.add(ix)).Sid);
                }
                HeapFree(heap, 0, items.cast());
            }
        }
        result
    }
    pub fn list() -> Result<Vec<UwpApp>> {
        let exempt = exemptions()?;
        let mut count = 0;
        let mut items = std::ptr::null_mut();
        status(unsafe { NetworkIsolationEnumAppContainers(0, &mut count, &mut items) })?;
        let result = (|| {
            let mut apps = BTreeMap::new();
            for ix in 0..count as usize {
                let item = unsafe { &*items.add(ix) };
                let sid = unsafe { sid(item.appContainerSid.cast()) }?;
                let package = unsafe { text(item.appContainerName) };
                let name = indirect(unsafe { text(item.displayName) });
                apps.insert(
                    sid.clone(),
                    UwpApp {
                        exempt: exempt.contains(&sid),
                        sid,
                        name: if name.is_empty() {
                            package.clone()
                        } else {
                            name
                        },
                        package,
                    },
                );
            }
            for sid in &exempt {
                apps.entry(sid.clone()).or_insert_with(|| UwpApp {
                    sid: sid.clone(),
                    name: "未知或已卸载的 AppContainer".into(),
                    package: sid.clone(),
                    exempt: true,
                });
            }
            let mut values = apps.into_values().collect::<Vec<_>>();
            values.sort_by(|a, b| a.name.cmp(&b.name));
            Ok(values)
        })();
        if !items.is_null() {
            status(unsafe { NetworkIsolationFreeAppContainers(items) })?;
        }
        result
    }
    #[cfg(feature = "standalone")]
    fn write(values: &BTreeSet<String>) -> Result<()> {
        struct Sids(Vec<*mut std::ffi::c_void>);
        impl Drop for Sids {
            fn drop(&mut self) {
                for sid in &self.0 {
                    unsafe { LocalFree(*sid) };
                }
            }
        }
        let mut owned = Sids(Vec::new());
        let mut items = Vec::new();
        for value in values {
            let wide = value.encode_utf16().chain([0]).collect::<Vec<_>>();
            let mut sid = std::ptr::null_mut();
            ensure!(
                unsafe { ConvertStringSidToSidW(wide.as_ptr(), &mut sid) } != 0,
                "AppContainer SID 无效"
            );
            owned.0.push(sid);
            items.push(SID_AND_ATTRIBUTES {
                Sid: sid,
                Attributes: 0,
            });
        }
        status(unsafe { NetworkIsolationSetAppContainerConfig(items.len() as u32, items.as_ptr()) })
    }
    #[cfg(feature = "standalone")]
    pub fn apply(
        request: Request,
        app_root: &std::path::Path,
        owner_key: &str,
    ) -> Result<Vec<UwpApp>> {
        match request {
            Request::List => list(),
            Request::SetLoopback { expected, selected } => {
                ensure!(
                    selected.len() <= 4096 && expected.len() <= 4096,
                    "回环豁免数量过多"
                );
                let before = exemptions()?;
                ensure!(
                    before == expected.into_iter().collect(),
                    "回环配置已变化，请刷新后重试"
                );
                let selected = selected.into_iter().collect::<BTreeSet<_>>();
                let known = list()?
                    .into_iter()
                    .map(|app| app.sid)
                    .collect::<BTreeSet<_>>();
                ensure!(selected.is_subset(&known), "选择包含不存在的 AppContainer");
                if selected == before {
                    return list();
                }
                let result = write(&selected).and_then(|()| {
                    ensure!(exemptions()? == selected, "回环配置回读不一致");
                    Ok(())
                });
                if let Err(error) = result {
                    let recovery = write(&before).and_then(|()| {
                        ensure!(exemptions()? == before, "回环恢复回读不一致");
                        Ok(())
                    });
                    return Err(match recovery {
                        Ok(()) => error,
                        Err(recovery) => anyhow::anyhow!("{error}；回环恢复失败：{recovery}"),
                    });
                }
                list().map_err(|error| anyhow::anyhow!("回环设置已生效；应用列表刷新失败：{error}"))
            }
            Request::RebuildFirewall => {
                firewall(app_root, owner_key)?;
                Ok(Vec::new())
            }
        }
    }
    #[cfg(feature = "standalone")]
    fn firewall(app_root: &std::path::Path, owner_key: &str) -> Result<()> {
        use windows::{
            Win32::{
                Foundation::VARIANT_BOOL,
                NetworkManagement::WindowsFirewall::{
                    INetFwPolicy2, INetFwRule, NET_FW_ACTION_ALLOW, NET_FW_PROFILE2_ALL,
                    NET_FW_RULE_DIR_IN, NetFwPolicy2, NetFwRule,
                },
                System::Com::{
                    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance,
                    CoInitializeEx, CoUninitialize,
                },
            },
            core::BSTR,
        };
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        }
        struct Com;
        impl Drop for Com {
            fn drop(&mut self) {
                unsafe { CoUninitialize() }
            }
        }
        let _com = Com;
        let policy: INetFwPolicy2 =
            unsafe { CoCreateInstance(&NetFwPolicy2, None, CLSCTX_INPROC_SERVER) }?;
        let rules = unsafe { policy.Rules() }?;
        let approved = crate::service_paths()?.core_dir().join("mihomo.exe");
        let local = app_root.join("core").join("mihomo.exe");
        let mut targets = Vec::new();
        if approved.is_file() {
            targets.push((
                format!("{} core (mihomo.exe)", crate::SERVICE_DISPLAY_NAME),
                approved,
            ));
        }
        if local.is_file() {
            let canonical = std::fs::canonicalize(&local)?;
            ensure!(
                canonical.starts_with(app_root)
                    && canonical.file_name() == Some(std::ffi::OsStr::new("mihomo.exe")),
                "本地内核路径超出应用目录"
            );
            targets.push((
                format!("Suki Clash {owner_key} core (mihomo.exe)"),
                canonical,
            ));
        }
        ensure!(!targets.is_empty(), "内核不存在，无法创建防火墙规则");
        let mut changed: Vec<(BSTR, Option<INetFwRule>, bool)> = Vec::new();
        let applied = (|| -> Result<()> {
            for (name, core) in targets {
                let name = BSTR::from(name);
                let previous = match unsafe { rules.Item(&name) } {
                    Ok(rule) => Some(rule),
                    Err(error) if error.code() == windows::core::HRESULT::from_win32(2) => None,
                    Err(error) => return Err(error.into()),
                };
                let rule: INetFwRule =
                    unsafe { CoCreateInstance(&NetFwRule, None, CLSCTX_INPROC_SERVER) }?;
                unsafe {
                    rule.SetName(&name)?;
                    rule.SetApplicationName(&BSTR::from(core.to_string_lossy().as_ref()))?;
                    rule.SetProtocol(256)?;
                    rule.SetDirection(NET_FW_RULE_DIR_IN)?;
                    rule.SetProfiles(NET_FW_PROFILE2_ALL.0)?;
                    rule.SetAction(NET_FW_ACTION_ALLOW)?;
                    rule.SetEnabled(VARIANT_BOOL(-1))?;
                }
                if previous.is_some() {
                    unsafe { rules.Remove(&name) }?;
                }
                changed.push((name.clone(), previous, false));
                unsafe { rules.Add(&rule) }?;
                if let Some(record) = changed.last_mut() {
                    record.2 = true;
                }
                let observed = unsafe { rules.Item(&name) }?;
                ensure!(
                    unsafe { observed.Enabled() }?.as_bool()
                        && unsafe { observed.Direction() }? == NET_FW_RULE_DIR_IN
                        && unsafe { observed.Action() }? == NET_FW_ACTION_ALLOW
                        && unsafe { observed.Protocol() }? == 256
                        && unsafe { observed.Profiles() }? == NET_FW_PROFILE2_ALL.0,
                    "防火墙规则回读不一致"
                );
                ensure!(
                    unsafe { observed.ApplicationName() }? == core.to_string_lossy(),
                    "防火墙程序路径回读不一致"
                );
            }
            Ok(())
        })();
        if let Err(error) = applied {
            let mut failures = vec![error.to_string()];
            for (name, previous, added) in changed.into_iter().rev() {
                if added && let Err(error) = unsafe { rules.Remove(&name) } {
                    failures.push(format!("新规则清理失败：{error}"));
                    continue;
                }
                if let Some(previous) = previous
                    && let Err(error) = unsafe { rules.Add(&previous) }
                {
                    failures.push(format!("旧规则恢复失败：{error}"));
                }
            }
            anyhow::bail!("{}", failures.join("；"));
        }

        Ok(())
    }
}
#[cfg(windows)]
pub fn list() -> anyhow::Result<Vec<UwpApp>> {
    native::list()
}
#[cfg(feature = "standalone")]
pub(crate) fn apply(
    request: Request,
    app_root: &std::path::Path,
    owner_key: &str,
) -> anyhow::Result<Vec<UwpApp>> {
    #[cfg(windows)]
    {
        native::apply(request, app_root, owner_key).inspect_err(|error| {
            let code = error
                .downcast_ref::<windows::core::Error>()
                .map(|error| error.code().0 as u32);
            log::error!("{}", network_failure(code, &format!("{error:#}")));
        })
    }
    #[cfg(not(windows))]
    {
        let _ = (request, app_root, owner_key);
        anyhow::bail!("UWP／Windows 防火墙仅支持 Windows")
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn rpc_endpoint_failure_keeps_native_reason_and_names_service_dependencies() {
        for code in [1753, 0x800706d9] {
            let message = super::network_failure(Some(code), "原始原因 (0x800706D9)");
            assert!(message.contains("MpsSvc"));
            assert!(message.contains("BFE"));
            assert!(message.contains("原始原因 (0x800706D9)"));
        }
        assert_eq!(
            super::network_failure(Some(5), "Access denied"),
            "Access denied"
        );
        assert_eq!(
            super::network_failure(None, "规则回读不一致"),
            "规则回读不一致"
        );
    }
}
