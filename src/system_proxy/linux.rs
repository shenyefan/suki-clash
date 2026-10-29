//! Desktop settings only: GNOME-compatible GSettings sessions and KDE 5/6.
//! No environment-variable or command-line proxy fallback.
use super::super::{Target, authority};
use crate::system_proxy::types::{Endpoint, OsSnapshot, PacProxy};

fn kde() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split(':')
        .any(|s| s.eq_ignore_ascii_case("KDE"))
}
fn command(name: &str, args: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new(name)
        .args(args)
        .output()
        .map_err(|error| format!("无法执行 {name}：{error}"))?;
    if !output.status.success() {
        return Err(format!(
            "{name} 未完成系统代理操作（{}）；请检查桌面会话和设置写入权限",
            output.status
        ));
    }
    String::from_utf8(output.stdout)
        .map(|s| s.trim().to_owned())
        .map_err(|_| format!("{name} 返回了无效文本"))
}
fn kde_tool(write: bool) -> Result<&'static str, String> {
    match (
        std::env::var("KDE_SESSION_VERSION")
            .unwrap_or_default()
            .as_str(),
        write,
    ) {
        ("6", true) => Ok("kwriteconfig6"),
        ("6", false) => Ok("kreadconfig6"),
        ("5", true) => Ok("kwriteconfig5"),
        ("5", false) => Ok("kreadconfig5"),
        _ => Err("系统代理仅支持 KDE 5/6 会话".into()),
    }
}
fn get(key: &str) -> Result<String, String> {
    if kde() {
        command(
            kde_tool(false)?,
            &[
                "--file",
                "kioslaverc",
                "--group",
                "Proxy Settings",
                "--key",
                key,
            ],
        )
    } else {
        command("gsettings", &["get", "org.gnome.system.proxy", key])
    }
}
fn set(key: &str, value: &str) -> Result<(), String> {
    if kde() {
        command(
            kde_tool(true)?,
            &[
                "--file",
                "kioslaverc",
                "--group",
                "Proxy Settings",
                "--key",
                key,
                value,
            ],
        )?;
        if key == "ProxyType" {
            // KIO consumes a session-bus signal after kioslaverc is committed.
            zbus::block_on(async {
                let bus = zbus::Connection::session().await?;
                bus.emit_signal(
                    None::<&str>,
                    "/KIO/Scheduler",
                    "org.kde.KIO.Scheduler",
                    "reparseSlaveConfiguration",
                    &("",),
                )
                .await
            })
            .map_err(|error| format!("KDE 代理设置已写入，但刷新通知失败：{error}"))?;
        }
    } else {
        command("gsettings", &["set", "org.gnome.system.proxy", key, value])?;
    }
    Ok(())
}
// GSettings prints string GVariants using GLib quoting, including escaped quotes.
fn strings(text: &str) -> Result<Vec<String>, String> {
    let mut result = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch.is_whitespace() || matches!(ch, '[' | ']' | ',') {
            continue;
        }
        if ch == '@' {
            while chars.peek().is_some_and(|c| !c.is_whitespace()) {
                chars.next();
            }
            continue;
        }
        if !matches!(ch, '\'' | '"') {
            return Err("GSettings 代理字段类型无效".into());
        }
        let mut value = String::new();
        let mut closed = false;
        while let Some(next) = chars.next() {
            if next == ch {
                closed = true;
                break;
            }
            if next == '\\' {
                let escaped = chars.next().ok_or("GSettings 字符串无效")?;
                value.push(match escaped {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    '\\' => '\\',
                    '\'' => '\'',
                    '"' => '"',
                    _ => return Err("GSettings 字符串转义无效".into()),
                });
            } else {
                value.push(next);
            }
        }
        if !closed {
            return Err("GSettings 字符串未闭合".into());
        }
        result.push(value);
    }
    Ok(result)
}
fn string(text: &str) -> Result<String, String> {
    let mut values = strings(text)?;
    if values.len() != 1 {
        return Err("GSettings 代理字段需为字符串".into());
    }
    Ok(values.remove(0))
}
fn quote(value: &str) -> String {
    format!(
        "'{}'",
        value
            .replace('\\', "\\\\")
            .replace('\'', "\\'")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
    )
}
pub(super) fn read() -> Result<OsSnapshot, String> {
    let is_kde = kde();
    if !is_kde
        && !std::env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .split(':')
            .any(|s| s.eq_ignore_ascii_case("GNOME"))
    {
        return Err("系统代理仅支持 GNOME 和 KDE 5/6 桌面会话".into());
    }
    let mode = get(if is_kde { "ProxyType" } else { "mode" })?;
    let mode = if is_kde {
        if mode.is_empty() { "0".into() } else { mode }
    } else {
        string(&mode)?
    };
    let manual = mode == if is_kde { "1" } else { "manual" };
    let auto = mode == if is_kde { "2" } else { "auto" };
    if ![
        if is_kde { "0" } else { "none" },
        if is_kde { "1" } else { "manual" },
        if is_kde { "2" } else { "auto" },
    ]
    .contains(&mode.as_str())
    {
        return Err("桌面正在使用不支持的系统代理模式".into());
    }
    let endpoint = |protocol: &str| -> Result<Endpoint, String> {
        let (host, port) = if is_kde {
            let address = get(&format!("{protocol}Proxy"))?;
            if address.is_empty() {
                (String::new(), 0)
            } else {
                let url = url::Url::parse(&address).map_err(|_| "KDE 代理地址无效")?;
                (
                    url.host_str()
                        .unwrap_or_default()
                        .trim_matches(['[', ']'])
                        .into(),
                    url.port().unwrap_or(0),
                )
            }
        } else {
            let schema = format!("org.gnome.system.proxy.{protocol}");
            let host = string(&command("gsettings", &["get", &schema, "host"])?)?;
            let port = command("gsettings", &["get", &schema, "port"])?
                .parse::<u16>()
                .map_err(|_| "GSettings 代理端口无效")?;
            (host, port)
        };
        Ok(Endpoint {
            enable: manual && !host.is_empty() && port > 0,
            switched_on: manual && !host.is_empty(),
            host,
            port,
        })
    };
    let bypass = get(if is_kde { "NoProxyFor" } else { "ignore-hosts" })?;
    let bypass = if is_kde {
        bypass
    } else {
        strings(&bypass)?.join(",")
    };
    let pac = get(if is_kde {
        "Proxy Config Script"
    } else {
        "autoconfig-url"
    })?;
    let pac = if is_kde { pac } else { string(&pac)? };
    let mut snapshot = OsSnapshot {
        http: endpoint("http")?,
        https: endpoint("https")?,
        socks: endpoint("socks")?,
        auto: PacProxy {
            enable: auto,
            url: pac,
        },
        auto_switched_on: auto,
        bypass,
        auto_detect: false,
    };
    // A manual switch pointing nowhere is not a verified disabled state.
    if manual
        && !snapshot.http.switched_on
        && !snapshot.https.switched_on
        && !snapshot.socks.switched_on
    {
        snapshot.http.switched_on = true;
    }
    Ok(snapshot)
}
pub(super) fn write(target: &Target) -> Result<(), String> {
    let is_kde = kde();
    // Set mode last; never activate partly populated endpoints.
    if target.system.enable {
        for protocol in ["http", "https", "socks"] {
            if is_kde {
                set(
                    &format!("{protocol}Proxy"),
                    &format!(
                        "{}://{}",
                        if protocol == "socks" { "socks" } else { "http" },
                        authority(&target.system.host, target.system.port)
                    ),
                )?;
            } else {
                let schema = format!("org.gnome.system.proxy.{protocol}");
                command(
                    "gsettings",
                    &["set", &schema, "host", &quote(&target.system.host)],
                )?;
                command(
                    "gsettings",
                    &["set", &schema, "port", &target.system.port.to_string()],
                )?;
            }
        }
        if is_kde {
            set("NoProxyFor", &target.system.bypass)?;
        } else {
            command(
                "gsettings",
                &["set", "org.gnome.system.proxy", "use-same-proxy", "false"],
            )?;
            command(
                "gsettings",
                &[
                    "set",
                    "org.gnome.system.proxy.http",
                    "use-authentication",
                    "false",
                ],
            )?;
            let bypass = super::super::entries(&target.system.bypass)
                .into_iter()
                .map(quote)
                .collect::<Vec<_>>()
                .join(",");
            set("ignore-hosts", &format!("[{bypass}]"))?;
        }
    }
    if target.auto.enable {
        set(
            if is_kde {
                "Proxy Config Script"
            } else {
                "autoconfig-url"
            },
            &if is_kde {
                target.auto.url.clone()
            } else {
                quote(&target.auto.url)
            },
        )?;
    }
    let mode = if target.system.enable {
        if is_kde { "1" } else { "'manual'" }
    } else if target.auto.enable {
        if is_kde { "2" } else { "'auto'" }
    } else if is_kde {
        "0"
    } else {
        "'none'"
    };
    set(if is_kde { "ProxyType" } else { "mode" }, mode)
}

pub(in crate::system_proxy) struct Checkpoint {
    snapshot: OsSnapshot,
    same_proxy: String,
    authentication: String,
}
pub(super) fn capture() -> Result<(OsSnapshot, Checkpoint), String> {
    let snapshot = read()?;
    let (same_proxy, authentication) = if kde() {
        (String::new(), String::new())
    } else {
        (
            command(
                "gsettings",
                &["get", "org.gnome.system.proxy", "use-same-proxy"],
            )?,
            command(
                "gsettings",
                &["get", "org.gnome.system.proxy.http", "use-authentication"],
            )?,
        )
    };
    Ok((
        snapshot.clone(),
        Checkpoint {
            snapshot,
            same_proxy,
            authentication,
        },
    ))
}
pub(super) fn restore(checkpoint: &Checkpoint) -> Result<(), String> {
    let snapshot = &checkpoint.snapshot;
    if !kde() {
        command(
            "gsettings",
            &[
                "set",
                "org.gnome.system.proxy",
                "use-same-proxy",
                &checkpoint.same_proxy,
            ],
        )?;
        command(
            "gsettings",
            &[
                "set",
                "org.gnome.system.proxy.http",
                "use-authentication",
                &checkpoint.authentication,
            ],
        )?;
    }
    let is_kde = kde();
    for (endpoint, protocol) in [
        (&snapshot.http, "http"),
        (&snapshot.https, "https"),
        (&snapshot.socks, "socks"),
    ] {
        if is_kde {
            let address = if endpoint.host.is_empty() {
                String::new()
            } else {
                format!(
                    "{}://{}",
                    if protocol == "socks" { "socks" } else { "http" },
                    authority(&endpoint.host, endpoint.port)
                )
            };
            set(&format!("{protocol}Proxy"), &address)?;
        } else {
            let schema = format!("org.gnome.system.proxy.{protocol}");
            command(
                "gsettings",
                &["set", &schema, "host", &quote(&endpoint.host)],
            )?;
            command(
                "gsettings",
                &["set", &schema, "port", &endpoint.port.to_string()],
            )?;
        }
    }
    if is_kde {
        set("NoProxyFor", &snapshot.bypass)?;
        set("Proxy Config Script", &snapshot.auto.url)?;
    } else {
        let bypass = super::super::entries(&snapshot.bypass)
            .into_iter()
            .map(quote)
            .collect::<Vec<_>>()
            .join(",");
        set("ignore-hosts", &format!("[{bypass}]"))?;
        set("autoconfig-url", &quote(&snapshot.auto.url))?;
    }
    let manual =
        snapshot.http.switched_on || snapshot.https.switched_on || snapshot.socks.switched_on;
    set(
        if is_kde { "ProxyType" } else { "mode" },
        if snapshot.auto_switched_on {
            if is_kde { "2" } else { "'auto'" }
        } else if manual {
            if is_kde { "1" } else { "'manual'" }
        } else if is_kde {
            "0"
        } else {
            "'none'"
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gvariant_strings_keep_quoting_and_empty_arrays() {
        assert_eq!(string(&quote("a'b\\c")).unwrap(), "a'b\\c");
        assert_eq!(
            strings("['localhost', '::1']").unwrap(),
            ["localhost", "::1"]
        );
        assert!(strings("@as []").unwrap().is_empty());
        assert!(strings("'unterminated").is_err());
        assert!(string("true").is_err());
    }
}
