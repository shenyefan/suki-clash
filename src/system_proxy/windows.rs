//! Read active WinINET flags, not the presence of stale AutoConfigURL registry values.
use super::super::{Backend, Target};
use crate::system_proxy::types::{Endpoint, OsSnapshot, PacProxy};
use std::{collections::BTreeMap, path::PathBuf};
use windows_sys::Win32::{
    Foundation::GlobalFree,
    NetworkManagement::Rras::{ERROR_BUFFER_TOO_SMALL, RASENTRYNAMEW, RasEnumEntriesW},
    Networking::WinInet::*,
};

#[derive(Clone, PartialEq, Eq)]
struct ConnectionSettings {
    flags: u32,
    strings: [String; 3],
}
pub(in crate::system_proxy) struct Checkpoint {
    connections: Vec<(Option<String>, ConnectionSettings)>,
}
pub(super) fn read() -> Result<OsSnapshot, String> {
    query(None).map(|(state, _)| state)
}
fn query(connection: Option<&str>) -> Result<(OsSnapshot, ConnectionSettings), String> {
    let mut name = connection.map(|name| name.encode_utf16().chain(Some(0)).collect::<Vec<_>>());
    let mut options = [
        INTERNET_PER_CONN_FLAGS,
        INTERNET_PER_CONN_PROXY_SERVER,
        INTERNET_PER_CONN_PROXY_BYPASS,
        INTERNET_PER_CONN_AUTOCONFIG_URL,
    ]
    .map(|dw_option| INTERNET_PER_CONN_OPTIONW {
        dwOption: dw_option,
        ..Default::default()
    });
    let mut list = INTERNET_PER_CONN_OPTION_LISTW {
        pszConnection: name
            .as_mut()
            .map_or(std::ptr::null_mut(), |name| name.as_mut_ptr()),
        dwSize: std::mem::size_of::<INTERNET_PER_CONN_OPTION_LISTW>() as u32,
        dwOptionCount: options.len() as u32,
        pOptions: options.as_mut_ptr(),
        ..Default::default()
    };
    let mut size = list.dwSize;
    // WinINET allocates the three string values; each is freed exactly once below.
    if unsafe {
        InternetQueryOptionW(
            std::ptr::null(),
            INTERNET_OPTION_PER_CONNECTION_OPTION,
            (&mut list as *mut INTERNET_PER_CONN_OPTION_LISTW).cast(),
            &mut size,
        )
    } == 0
    {
        return Err(format!(
            "无法读取 WinINET 系统代理：{}",
            std::io::Error::last_os_error()
        ));
    }
    let flags = unsafe { options[0].Value.dwValue };
    let mut strings = Vec::new();
    for option in &options[1..] {
        let pointer = unsafe { option.Value.pszValue };
        let text = if pointer.is_null() {
            String::new()
        } else {
            let mut len = 0;
            unsafe {
                while *pointer.add(len) != 0 {
                    len += 1;
                }
            }
            let text =
                String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(pointer, len) });
            unsafe {
                GlobalFree(pointer.cast());
            }
            text
        };
        strings.push(text);
    }
    let endpoint = |protocol: &str| -> Result<Endpoint, String> {
        let server = &strings[0];
        let address = if server.contains('=') {
            server
                .split(';')
                .filter_map(|p| p.split_once('='))
                .find(|(key, _)| key.trim().eq_ignore_ascii_case(protocol))
                .map(|(_, value)| value.trim())
                .unwrap_or_default()
        } else {
            server.as_str()
        };
        let switched_on = flags & PROXY_TYPE_PROXY != 0 && !address.is_empty();
        let (host, port) = if address.is_empty() {
            (String::new(), 0)
        } else {
            let url = url::Url::parse(&format!("http://{address}"))
                .map_err(|error| crate::logging::diagnostic_error("WinINET 代理地址无效", error))?;
            (
                url.host_str()
                    .unwrap_or_default()
                    .trim_matches(['[', ']'])
                    .to_owned(),
                url.port().unwrap_or(80),
            )
        };
        Ok(Endpoint {
            enable: switched_on,
            switched_on,
            host,
            port,
        })
    };
    let mut snapshot = OsSnapshot {
        http: endpoint("http")?,
        https: endpoint("https")?,
        socks: endpoint("socks")?,
        auto: PacProxy {
            enable: flags & PROXY_TYPE_AUTO_PROXY_URL != 0,
            url: strings[2].clone(),
        },
        auto_switched_on: flags & (PROXY_TYPE_AUTO_PROXY_URL | PROXY_TYPE_AUTO_DETECT) != 0,
        bypass: strings[1].clone(),
        auto_detect: flags & PROXY_TYPE_AUTO_DETECT != 0,
    };
    if flags & PROXY_TYPE_PROXY != 0
        && !snapshot.http.switched_on
        && !snapshot.https.switched_on
        && !snapshot.socks.switched_on
    {
        snapshot.http.switched_on = true;
    }
    Ok((
        snapshot,
        ConnectionSettings {
            flags,
            strings: strings
                .try_into()
                .map_err(|error| crate::logging::diagnostic_error("WinINET 字段数量无效", error))?,
        },
    ))
}

fn restore_connection(
    connection: Option<&str>,
    checkpoint: &ConnectionSettings,
) -> Result<(), String> {
    let mut name = connection.map(|name| name.encode_utf16().chain(Some(0)).collect::<Vec<_>>());
    let flags = checkpoint.flags;
    let mut strings = checkpoint
        .strings
        .clone()
        .map(|s| s.encode_utf16().chain(Some(0)).collect::<Vec<_>>());
    let mut options = [
        INTERNET_PER_CONN_OPTIONW {
            dwOption: INTERNET_PER_CONN_FLAGS,
            Value: INTERNET_PER_CONN_OPTIONW_0 { dwValue: flags },
        },
        INTERNET_PER_CONN_OPTIONW {
            dwOption: INTERNET_PER_CONN_PROXY_SERVER,
            Value: INTERNET_PER_CONN_OPTIONW_0 {
                pszValue: strings[0].as_mut_ptr(),
            },
        },
        INTERNET_PER_CONN_OPTIONW {
            dwOption: INTERNET_PER_CONN_PROXY_BYPASS,
            Value: INTERNET_PER_CONN_OPTIONW_0 {
                pszValue: strings[1].as_mut_ptr(),
            },
        },
        INTERNET_PER_CONN_OPTIONW {
            dwOption: INTERNET_PER_CONN_AUTOCONFIG_URL,
            Value: INTERNET_PER_CONN_OPTIONW_0 {
                pszValue: strings[2].as_mut_ptr(),
            },
        },
    ];
    let list = INTERNET_PER_CONN_OPTION_LISTW {
        pszConnection: name
            .as_mut()
            .map_or(std::ptr::null_mut(), |name| name.as_mut_ptr()),
        dwSize: std::mem::size_of::<INTERNET_PER_CONN_OPTION_LISTW>() as u32,
        dwOptionCount: options.len() as u32,
        pOptions: options.as_mut_ptr(),
        ..Default::default()
    };
    for (option, pointer, size) in [
        (
            INTERNET_OPTION_PER_CONNECTION_OPTION,
            (&list as *const INTERNET_PER_CONN_OPTION_LISTW).cast(),
            list.dwSize,
        ),
        (INTERNET_OPTION_PROXY_SETTINGS_CHANGED, std::ptr::null(), 0),
        (INTERNET_OPTION_REFRESH, std::ptr::null(), 0),
    ] {
        if unsafe { InternetSetOptionW(std::ptr::null(), option, pointer, size) } == 0 {
            return Err(format!(
                "无法恢复 WinINET 代理：{}",
                std::io::Error::last_os_error()
            ));
        }
    }
    Ok(())
}

fn connections() -> Result<Vec<Option<String>>, String> {
    let mut size = 0;
    let mut count = 0;
    let result = unsafe {
        RasEnumEntriesW(
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            &mut size,
            &mut count,
        )
    };
    if result == 0 {
        return Ok(vec![None]);
    }
    if result != ERROR_BUFFER_TOO_SMALL {
        return Err(format!("无法枚举 RAS 代理连接（{result}）"));
    }
    let length = (size as usize).div_ceil(std::mem::size_of::<RASENTRYNAMEW>());
    let mut buffer = vec![RASENTRYNAMEW::default(); length];
    buffer[0].dwSize = std::mem::size_of::<RASENTRYNAMEW>() as u32;
    let result = unsafe {
        RasEnumEntriesW(
            std::ptr::null(),
            std::ptr::null(),
            buffer.as_mut_ptr(),
            &mut size,
            &mut count,
        )
    };
    if result != 0 {
        return Err(format!("无法读取 RAS 代理连接（{result}）"));
    }
    let mut names = vec![None];
    for entry in buffer.into_iter().take(count as usize) {
        let length = entry
            .szEntryName
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(entry.szEntryName.len());
        let name = String::from_utf16(&entry.szEntryName[..length])
            .map_err(|error| crate::logging::diagnostic_error("RAS 连接名称编码无效", error))?;
        names.push(Some(name));
    }
    Ok(names)
}
fn desired(target: &Target, before: &ConnectionSettings) -> ConnectionSettings {
    if !target.system.enable && !target.auto.enable {
        let mut disabled = before.clone();
        disabled.flags = PROXY_TYPE_DIRECT;
        return disabled;
    }
    ConnectionSettings {
        flags: PROXY_TYPE_DIRECT
            | if target.system.enable {
                PROXY_TYPE_PROXY
            } else {
                PROXY_TYPE_AUTO_PROXY_URL
            },
        strings: [
            super::super::authority(&target.system.host, target.system.port),
            target.system.bypass.clone(),
            target.auto.url.clone(),
        ],
    }
}
fn without_owned(before: &ConnectionSettings, targets: &[Target]) -> ConnectionSettings {
    let mut result = before.clone();
    let owned = |address: &str| {
        url::Url::parse(&format!("http://{address}"))
            .ok()
            .is_some_and(|url| {
                targets.iter().any(|target| {
                    target.system.enable
                        && url
                            .host_str()
                            .is_some_and(|host| host.trim_matches(['[', ']']) == target.system.host)
                        && url.port().unwrap_or(80) == target.system.port
                })
            })
    };
    if before.flags & PROXY_TYPE_PROXY != 0 {
        if before.strings[0].contains('=') {
            let parts = before.strings[0]
                .split(';')
                .filter(|part| {
                    part.split_once('=')
                        .is_none_or(|(_, address)| !owned(address.trim()))
                })
                .collect::<Vec<_>>();
            if parts.is_empty() {
                result.flags = (result.flags & !PROXY_TYPE_PROXY) | PROXY_TYPE_DIRECT;
            } else {
                result.strings[0] = parts.join(";");
            }
        } else if owned(&before.strings[0]) {
            result.flags = (result.flags & !PROXY_TYPE_PROXY) | PROXY_TYPE_DIRECT;
        }
    }
    if before.flags & PROXY_TYPE_AUTO_PROXY_URL != 0
        && targets
            .iter()
            .any(|target| target.auto.enable && target.auto.url == before.strings[2])
    {
        result.flags = (result.flags & !PROXY_TYPE_AUTO_PROXY_URL) | PROXY_TYPE_DIRECT;
    }
    result
}
pub(super) struct Adapter {
    journal: Option<PathBuf>,
    owned: BTreeMap<Option<String>, Vec<Target>>,
    pending: Vec<Option<String>>,
}
impl Adapter {
    pub(super) fn new(journal: Option<PathBuf>, _: bool) -> Result<Self, String> {
        let owned = if let Some(path) = &journal {
            match std::fs::read(path) {
                Ok(bytes) => serde_json::from_slice::<Vec<(Option<String>, Vec<Target>)>>(&bytes)
                    .map_err(|error| {
                        crate::logging::diagnostic_error(
                            "系统代理恢复记录损坏，未覆盖原文件",
                            error,
                        )
                    })?
                    .into_iter()
                    .collect(),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
                Err(error) => return Err(format!("无法读取系统代理恢复记录：{error}")),
            }
        } else {
            BTreeMap::new()
        };
        let mut adapter = Self {
            journal,
            owned,
            pending: Vec::new(),
        };
        if !adapter.owned.is_empty() {
            adapter.cleanup()?;
        }
        Ok(adapter)
    }
    fn persist(&self) -> Result<(), String> {
        if let Some(path) = &self.journal {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(crate::logging::error_message)?;
            }
            crate::storage::atomic_write(
                path,
                &serde_json::to_vec(&self.owned.iter().collect::<Vec<_>>()).map_err(|error| {
                    crate::logging::diagnostic_error("无法编码代理连接归属", error)
                })?,
            )
            .map_err(|error| format!("无法保存代理连接归属：{error}"))?;
        }
        Ok(())
    }
    fn cleanup(&mut self) -> Result<(), String> {
        let existing = connections()?;
        let mut failures = Vec::new();
        for (name, targets) in self.owned.clone() {
            if !existing.contains(&name) {
                self.owned.remove(&name);
                continue;
            }
            let result = (|| {
                let (_, before) = query(name.as_deref())?;
                let after = without_owned(&before, &targets);
                if after != before {
                    restore_connection(name.as_deref(), &after)?;
                }
                if query(name.as_deref())?.1 != after {
                    return Err("连接代理清理后回读不一致".to_owned());
                }
                Ok(())
            })();
            match result {
                Ok(()) => {
                    self.owned.remove(&name);
                }
                Err(error) => failures.push(format!(
                    "代理连接 {}：{error}",
                    name.as_deref().unwrap_or("默认连接")
                )),
            }
        }
        if let Err(error) = self.persist() {
            failures.push(error);
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("；"))
        }
    }
}
impl Backend for Adapter {
    type Checkpoint = Checkpoint;
    fn capture(&mut self) -> Result<(OsSnapshot, Checkpoint), String> {
        self.pending = connections()?;
        let mut saved = Vec::new();
        let mut default = None;
        for name in &self.pending {
            let (snapshot, settings) = query(name.as_deref())?;
            if name.is_none() {
                default = Some(snapshot);
            }
            saved.push((name.clone(), settings));
        }
        Ok((
            default.ok_or("默认代理连接缺失")?,
            Checkpoint { connections: saved },
        ))
    }
    fn read(&self) -> Result<OsSnapshot, String> {
        read()
    }
    fn matches(&self, target: &Target, _: &OsSnapshot) -> Result<bool, String> {
        for name in connections()? {
            if !target.matches(&query(name.as_deref())?.0) {
                return Ok(false);
            }
        }
        Ok(true)
    }
    fn checkpoint_matches(&self, checkpoint: &Checkpoint, _: &OsSnapshot) -> Result<bool, String> {
        for (name, saved) in &checkpoint.connections {
            if query(name.as_deref())?.1 != *saved {
                return Ok(false);
            }
        }
        Ok(true)
    }
    fn write(&mut self, target: &Target) -> Result<(), String> {
        if !target.system.enable && !target.auto.enable {
            return self.cleanup();
        }
        // Capture fixed identities before any write. Every changed connection is journalled.
        for name in &self.pending {
            let values = self.owned.entry(name.clone()).or_default();
            if !values.contains(target) {
                values.push(target.clone());
            }
        }
        self.persist()?;
        for name in &self.pending {
            let (_, before) = query(name.as_deref())?;
            let after = desired(target, &before);
            restore_connection(name.as_deref(), &after).map_err(|error| {
                format!(
                    "代理连接 {}：{error}",
                    name.as_deref().unwrap_or("默认连接")
                )
            })?;
            if !target.matches(&query(name.as_deref())?.0) {
                return Err("代理连接写入后回读不一致".into());
            }
        }
        Ok(())
    }
    fn restore(&mut self, checkpoint: &Checkpoint) -> Result<(), String> {
        let mut errors = Vec::new();
        for (name, settings) in &checkpoint.connections {
            if let Err(error) = restore_connection(name.as_deref(), settings) {
                errors.push(format!(
                    "代理连接 {}：{error}",
                    name.as_deref().unwrap_or("默认连接")
                ));
            }
        }
        if !errors.is_empty() {
            return Err(errors.join("；"));
        }
        if !self.checkpoint_matches(checkpoint, &OsSnapshot::default())? {
            return Err("代理连接恢复后回读不一致".into());
        }
        Ok(())
    }
    fn clear(&mut self, target: &Target) -> Result<OsSnapshot, String> {
        for name in connections()? {
            if target.owns(&query(name.as_deref())?.0) {
                let targets = self.owned.entry(name).or_default();
                if !targets.contains(target) {
                    targets.push(target.clone());
                }
            }
        }
        self.persist()?;
        self.cleanup()?;
        read()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cleanup_keeps_another_clients_protocol_and_discovery() {
        let target = Target {
            system: crate::system_proxy::types::ManualProxy {
                host: "127.0.0.1".into(),
                port: 7897,
                enable: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let before = ConnectionSettings {
            flags: PROXY_TYPE_PROXY | PROXY_TYPE_AUTO_DETECT,
            strings: [
                "http=127.0.0.1:7897;https=other.test:8080".into(),
                "local".into(),
                String::new(),
            ],
        };
        let after = without_owned(&before, &[target]);
        assert_eq!(after.strings[0], "https=other.test:8080");
        assert_eq!(after.flags, before.flags);
    }
}
