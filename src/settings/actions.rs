use super::SettingsPage;
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
            "复制版本号" => cx.write_to_clipboard(ClipboardItem::new_string(format!(
                "Suki Clash v{}",
                env!("CARGO_PKG_VERSION")
            ))),
            "复制环境变量" => {
                let values = self.values(cx);
                if let Some(error) = super::schema::validate(&self.sections, &values) {
                    ui::show_error(error, window, cx);
                    return true;
                }
                let host = storage::read("sysproxy", cx)
                    .get("SystemProxyHost")
                    .cloned()
                    .unwrap_or_else(|| "127.0.0.1".into());
                let port = storage::read("core", cx)
                    .get("MixedPort")
                    .cloned()
                    .unwrap_or_else(|| "7890".into());
                let host = if host.contains(':') && !host.starts_with('[') {
                    format!("[{host}]")
                } else {
                    host
                };
                let http = format!("http://{host}:{port}");
                let socks = format!("socks5://{host}:{port}");
                let output = match values
                    .get("EnvTypeIndex")
                    .map(String::as_str)
                    .unwrap_or("Bash")
                {
                    "PowerShell" => format!(
                        "$env:http_proxy='{http}'; $env:https_proxy='{http}'; $env:all_proxy='{socks}'"
                    ),
                    "CMD" => format!(
                        "set http_proxy={http}\r\nset https_proxy={http}\r\nset all_proxy={socks}"
                    ),
                    "Fish" => format!(
                        "set -gx http_proxy '{http}'; set -gx https_proxy '{http}'; set -gx all_proxy '{socks}'"
                    ),
                    "Nushell" => format!(
                        "$env.http_proxy = '{http}'; $env.https_proxy = '{http}'; $env.all_proxy = '{socks}'"
                    ),
                    _ => format!(
                        "export http_proxy='{http}' https_proxy='{http}' all_proxy='{socks}'"
                    ),
                };
                cx.write_to_clipboard(ClipboardItem::new_string(output));
            }
            "复制代理地址" | "复制 PAC 地址" => {
                // These values must come from the OS/runtime adapter, not a configured guess.
                crate::integration::request_with(action, self.command_values(cx), window, cx);
            }
            _ => return false,
        }
        true
    }
}
