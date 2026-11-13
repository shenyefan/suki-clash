use super::SettingsPage;
use crate::storage::Values;
use crate::{integration as storage, ui};
use gpui_kit::*;

impl SettingsPage {
    pub(super) fn local_action(
        &mut self,
        action: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match action {
            "UWP 应用代理" | "选择应用" => super::uwp::open(window, cx),
            "检查应用更新" => super::update::open(window, cx),
            "创建本地备份" => crate::core::request(
                crate::core::Command::LocalBackup(crate::core::local_backup::Command::Create),
                window,
                cx,
            ),
            "本地备份历史" => super::local_backup::open(window, cx),
            "配置 Gist 备份" => super::gist::configure(window, cx),
            "创建 Gist 备份" => crate::core::request(
                crate::core::Command::Gist(crate::core::gist::Command::Create),
                window,
                cx,
            ),
            "Gist 备份历史" => super::gist::history(window, cx),
            "安装服务" | "更新服务" | "修复服务" | "卸载服务" => {
                let action = match action {
                    "安装服务" => crate::core::ServiceAction::Install,
                    "更新服务" => crate::core::ServiceAction::Update,
                    "修复服务" => crate::core::ServiceAction::Repair,
                    "卸载服务" => crate::core::ServiceAction::Uninstall,
                    _ => return false,
                };
                if matches!(action, crate::core::ServiceAction::Uninstall) {
                    use gpui_kit::component::WindowExt;
                    window.open_alert_dialog(cx, move |dialog, window, cx| {
                        let action = action.clone();
                        ui::modal_alert(dialog, window, cx)
                            .title("卸载服务并关闭虚拟网卡？")
                            .description("系统代理设置保留；虚拟网卡将关闭。")
                            .footer(ui::dialog_footer("卸载服务"))
                            .on_ok(move |_, window, cx| {
                                crate::core::request(
                                    crate::core::Command::PlatformService(action.clone()),
                                    window,
                                    cx,
                                );
                                true
                            })
                    });
                } else {
                    crate::core::request(crate::core::Command::PlatformService(action), window, cx);
                }
            }
            "重建防火墙规则" => crate::core::request(
                crate::core::Command::WindowsNetwork(
                    suki_service::windows_network::Request::RebuildFirewall,
                ),
                window,
                cx,
            ),
            "导出诊断信息" => {
                let name = format!(
                    "suki-diagnostics-{}.zip",
                    chrono::Local::now().format("%Y%m%dT%H%M%S")
                );
                let picker = cx.prompt_for_new_path(
                    &dirs::download_dir().unwrap_or_else(std::env::temp_dir),
                    Some(&name),
                );
                self.pending_action = Some(cx.spawn_in(window, async move |owner, cx| {
                    let result = picker.await;
                    let _ = owner.update_in(cx, |this, window, cx| {
                        this.pending_action = None;
                        match result {
                            Ok(Ok(Some(target))) => {
                                let reveal = target.clone();
                                if let Err(error) = crate::core::execute(
                                    crate::core::Command::Diagnostics(target),
                                    window,
                                    cx,
                                    move |result, _, cx| {
                                        if result.is_ok() {
                                            cx.reveal_path(&reveal);
                                        }
                                    },
                                ) {
                                    ui::show_error(error, window, cx);
                                }
                            }
                            Ok(Ok(None)) => {}
                            Ok(Err(error)) => ui::show_error(error.to_string(), window, cx),
                            Err(error) => ui::show_error(error.to_string(), window, cx),
                        }
                        cx.notify();
                    });
                }));
            }
            "查看运行配置" => {
                if let Err(error) = crate::core::execute(
                    crate::core::Command::RuntimeConfig,
                    window,
                    cx,
                    |result, window, cx| {
                        if result.is_ok() {
                            let content = crate::core::controller(cx)
                                .read(cx)
                                .snapshot()
                                .runtime_yaml()
                                .map(str::to_owned);
                            if let Some(content) = content {
                                let editor = cx
                                    .new(|cx| ui::editor::state("yaml", content, true, window, cx));
                                ui::editor::expand(
                                    editor,
                                    "运行配置".into(),
                                    true,
                                    None,
                                    window,
                                    cx,
                                );
                            }
                        }
                    },
                ) {
                    ui::show_error(error, window, cx);
                }
            }
            "复制环境变量" => copy_environment(window, cx),
            _ => return false,
        }
        true
    }
}

/// Shared by settings and the tray. Copy only configured protocol listeners.
pub(crate) fn copy_environment(window: &mut Window, cx: &mut App) {
    let values = storage::read("settings", cx);
    let host = storage::read("sysproxy", cx)
        .get("SystemProxyHost")
        .cloned()
        .unwrap_or_else(|| "127.0.0.1".into());
    let ports = storage::read("core", cx);
    match environment(
        &host,
        &ports,
        values
            .get("EnvTypeIndex")
            .map(String::as_str)
            .unwrap_or("Bash"),
    ) {
        Ok(output) => cx.write_to_clipboard(ClipboardItem::new_string(output)),
        Err(error) => ui::show_error(error, window, cx),
    }
}
fn environment(host: &str, ports: &Values, shell: &str) -> Result<String, String> {
    let port = |name: &str| {
        ports
            .get(name)
            .and_then(|value| value.parse::<u16>().ok())
            .filter(|port| *port != 0)
    };
    let mixed = port("MixedPort");
    let http = mixed.or_else(|| port("HttpPort"));
    let socks = mixed.or_else(|| port("SocksPort"));
    if http.is_none() && socks.is_none() {
        return Err("未配置 HTTP 或 SOCKS 代理监听端口".into());
    }
    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    let http = http.map(|port| format!("http://{host}:{port}"));
    let socks = socks.map(|port| format!("socks5://{host}:{port}"));
    let entries = [
        ("http_proxy", http.as_deref()),
        ("https_proxy", http.as_deref()),
        ("all_proxy", socks.as_deref()),
    ];
    let commands = entries
        .into_iter()
        .map(|(name, value)| match (shell, value) {
            ("PowerShell", Some(value)) => format!("$env:{name}='{value}'"),
            ("PowerShell", None) => format!("Remove-Item Env:{name} -ErrorAction SilentlyContinue"),
            ("CMD", Some(value)) => format!("set {name}={value}"),
            ("CMD", None) => format!("set {name}="),
            ("Fish", Some(value)) => format!("set -gx {name} '{value}'"),
            ("Fish", None) => format!("set -e {name}"),
            ("Nushell", Some(value)) => format!("$env.{name} = '{value}'"),
            ("Nushell", None) => format!("hide-env --ignore-errors {name}"),
            (_, Some(value)) => format!("export {name}='{value}'"),
            (_, None) => format!("unset {name}"),
        })
        .collect::<Vec<_>>();
    Ok(commands.join(if shell == "CMD" { "\r\n" } else { "; " }))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn environment_uses_available_protocol_ports_and_clears_unavailable_variables() {
        let ports = Values::from([
            ("MixedPort".into(), "0".into()),
            ("HttpPort".into(), "8080".into()),
            ("SocksPort".into(), "0".into()),
        ]);
        let text = environment("::1", &ports, "Bash").unwrap();
        assert!(text.contains("http://[::1]:8080"));
        assert!(text.contains("unset all_proxy"));
        assert!(!text.contains("socks5://"));
        for shell in ["PowerShell", "CMD", "Fish", "Nushell"] {
            assert!(
                !environment("127.0.0.1", &ports, shell)
                    .unwrap()
                    .contains(":0")
            );
        }
        let ports = Values::from([("SocksPort".into(), "1080".into())]);
        let text = environment("localhost", &ports, "Fish").unwrap();
        assert!(text.contains("socks5://localhost:1080"));
        assert!(text.contains("set -e http_proxy"));
        assert!(environment("localhost", &Default::default(), "Bash").is_err());
    }
}
