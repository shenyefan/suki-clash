//! Read active WinINET flags, not the presence of stale AutoConfigURL registry values.
use crate::system_proxy::types::{Endpoint, OsSnapshot, PacProxy};
use windows_sys::Win32::{Foundation::GlobalFree, Networking::WinInet::*};

pub(in crate::system_proxy) struct Checkpoint {
    flags: u32,
    strings: [String; 3],
}
pub(super) fn read() -> Result<OsSnapshot, String> {
    capture().map(|(state, _)| state)
}
pub(super) fn capture() -> Result<(OsSnapshot, Checkpoint), String> {
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
                .map_err(|_| "WinINET 代理地址无效")?;
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
        Checkpoint {
            flags,
            strings: strings.try_into().map_err(|_| "WinINET 字段数量无效")?,
        },
    ))
}

pub(super) fn write(target: &super::super::Target) -> Result<(), String> {
    if !target.system.enable && !target.auto.enable {
        let (_, mut checkpoint) = capture()?;
        checkpoint.flags = PROXY_TYPE_DIRECT;
        return restore(&checkpoint);
    }
    let flags = PROXY_TYPE_DIRECT
        | if target.system.enable {
            PROXY_TYPE_PROXY
        } else if target.auto.enable {
            PROXY_TYPE_AUTO_PROXY_URL
        } else {
            0
        };
    let server = super::super::authority(&target.system.host, target.system.port);
    restore(&Checkpoint {
        flags,
        strings: [
            server,
            target.system.bypass.clone(),
            target.auto.url.clone(),
        ],
    })
}
pub(super) fn restore(checkpoint: &Checkpoint) -> Result<(), String> {
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
