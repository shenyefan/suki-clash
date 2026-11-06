//! Reusable HTTP clients owned by the application worker, separated by transport policy.
use crate::storage::AppPaths;
use reqwest::{Client, Proxy, redirect::Policy};
use std::sync::{Arc, Mutex};
use std::time::Duration;
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum Transport {
    Direct,
    Proxy {
        url: String,
        authentication: Option<String>,
    },
    System,
}
/// GitHub hosts whose public downloads may be routed through the configured download proxy.
const GITHUB_HOSTS: [&str; 7] = [
    "github.com",
    "api.github.com",
    "gist.github.com",
    "codeload.github.com",
    "raw.githubusercontent.com",
    "gist.githubusercontent.com",
    "objects.githubusercontent.com",
];
/// Prefixes the proxy before public GitHub URLs; authenticated requests never leave GitHub.
pub(crate) fn github_proxy_url(
    paths: &AppPaths,
    url: &str,
    authorization: &str,
) -> Result<String, String> {
    let github = authorization.is_empty()
        && url::Url::parse(url).is_ok_and(|parsed| {
            parsed.scheme() == "https"
                && parsed
                    .host_str()
                    .is_some_and(|host| GITHUB_HOSTS.contains(&host))
        });
    if !github {
        return Ok(url.to_owned());
    }
    let repository = crate::storage::Repository::open(paths.clone())
        .map_err(|error| crate::logging::diagnostic_error("无法读取下载设置", error))?;
    let settings =
        crate::preferences::values(&repository, crate::preferences::SettingsKind::General)
            .map_err(|error| crate::logging::diagnostic_error("无法读取下载设置", error))?;
    let proxy = settings.get("GitHubProxy").map_or("", |value| value.trim());
    Ok(if proxy.is_empty() {
        url.to_owned()
    } else {
        format!("{}/{url}", proxy.trim_end_matches('/'))
    })
}

/// Subscription servers recognize Clash Verge first; retain this application's identity as well.
pub(crate) const DEFAULT_USER_AGENT: &str = concat!(
    "clash-verge/v",
    env!("CARGO_PKG_VERSION"),
    " Suki-Clash/",
    env!("CARGO_PKG_VERSION")
);
type ClientCache = Vec<(Transport, bool, bool, Client)>;
#[derive(Clone, Default)]
pub(crate) struct Http {
    clients: Arc<Mutex<ClientCache>>,
}
impl Http {
    pub(crate) fn client(&self, transport: &Transport, https_only: bool) -> Result<Client, String> {
        self.client_with_tls(transport, https_only, false)
    }
    pub(crate) fn client_with_tls(
        &self,
        transport: &Transport,
        https_only: bool,
        accept_invalid_certs: bool,
    ) -> Result<Client, String> {
        let mut clients = self
            .clients
            .lock()
            .map_err(|error| crate::logging::diagnostic_error("HTTP 客户端状态不可用", error))?;
        // System proxy configuration may change outside this application. Resolve it for each task.
        if !matches!(transport, Transport::System)
            && let Some((_, _, _, client)) = clients.iter().find(|(route, secure, invalid, _)| {
                route == transport && *secure == https_only && *invalid == accept_invalid_certs
            })
        {
            return Ok(client.clone());
        }
        let mut builder = Client::builder()
            .user_agent(DEFAULT_USER_AGENT)
            .connect_timeout(Duration::from_secs(15))
            .https_only(https_only)
            .tls_danger_accept_invalid_certs(accept_invalid_certs)
            .redirect(Policy::custom(move |attempt| {
                if attempt.previous().len() >= 5 {
                    attempt.error("重定向次数超过上限")
                } else if !matches!(attempt.url().scheme(), "http" | "https")
                    || (https_only && attempt.url().scheme() != "https")
                    || !attempt.url().username().is_empty()
                    || attempt.url().password().is_some()
                {
                    attempt.error("重定向地址无效")
                } else {
                    attempt.follow()
                }
            }));
        match transport {
            Transport::Direct => builder = builder.no_proxy(),
            Transport::System => {}
            Transport::Proxy {
                url,
                authentication,
            } => {
                let mut proxy = Proxy::all(url).map_err(|error| {
                    log::debug!(
                        "HTTP 代理地址无效（{}）",
                        crate::logging::error_chain(&error)
                    );
                    "HTTP 代理地址无效"
                })?;
                if let Some(authentication) = authentication {
                    let (user, password) =
                        authentication.split_once(':').ok_or("代理凭据格式无效")?;
                    proxy = proxy.basic_auth(user, password);
                }
                builder = builder.no_proxy().proxy(proxy);
            }
        }
        let client = builder.build().map_err(|error| {
            log::debug!(
                "创建 HTTP 客户端失败（{}）",
                crate::logging::error_chain(&error)
            );
            "无法创建 HTTP 客户端"
        })?;
        if !matches!(transport, Transport::System) {
            if clients.len() >= 8 {
                clients.remove(0);
            }
            clients.push((
                transport.clone(),
                https_only,
                accept_invalid_certs,
                client.clone(),
            ));
        }
        Ok(client)
    }
}
pub(crate) fn error(error: reqwest::Error) -> String {
    // The owning workflow reports the failure once; retain transport details for diagnosis.
    log::debug!("HTTP 请求失败（{}）", crate::logging::error_chain(&error));
    if error.is_timeout() {
        "HTTP 请求超时".into()
    } else if let Some(status) = error.status() {
        format!("服务器返回 HTTP {}", status.as_u16())
    } else if error.is_connect() {
        "HTTP 连接失败，请检查网络、代理或证书".into()
    } else if error.is_redirect() {
        "HTTP 重定向失败".into()
    } else if error.is_body() || error.is_decode() {
        "HTTP 响应读取失败".into()
    } else {
        "HTTP 请求失败".into()
    }
}

#[cfg(test)]
mod client_policy_tests {
    use super::*;
    #[test]
    fn github_proxy_prefixes_public_github_urls_only() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        for url in [
            "https://github.com/MetaCubeX/mihomo/releases/download/v1/a.gz",
            "https://raw.githubusercontent.com/a/b/main/c.yaml",
            "https://gist.githubusercontent.com/a/b/raw/c",
            "https://api.github.com/repos/MetaCubeX/mihomo/tags?per_page=100",
            "https://github.com/shenyefan/suki-clash/releases/latest/download/releases.linux-x64.json",
        ] {
            assert_eq!(
                github_proxy_url(&paths, url, "").unwrap(),
                format!("https://get.syf.sh/{url}")
            );
        }
        for (url, authorization) in [
            ("https://example.com/a.yaml", ""),
            ("https://github.com.evil.test/a", ""),
            ("https://api.github.com/gists", "token"),
        ] {
            assert_eq!(github_proxy_url(&paths, url, authorization).unwrap(), url);
        }
    }
    #[test]
    fn github_proxy_uses_current_settings_including_disabled_and_custom_prefix() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        let repository = crate::storage::Repository::open(paths.clone()).unwrap();
        let namespace = crate::storage::Namespace::parse("settings").unwrap();
        let url = "https://github.com/a/b/releases/download/v1/core.gz";
        for prefix in ["https://mirror.test/base/", ""] {
            let mut settings =
                crate::preferences::defaults(crate::preferences::SettingsKind::General);
            settings.insert("GitHubProxy".into(), prefix.into());
            repository.save(&namespace, &settings).unwrap();
            let expected = if prefix.is_empty() {
                url.to_owned()
            } else {
                format!("{}{url}", prefix)
            };
            assert_eq!(github_proxy_url(&paths, url, "").unwrap(), expected);
        }
    }
    #[test]
    fn explicit_insecure_subscription_client_never_changes_default_tls_policy() {
        let http = Http::default();
        http.client_with_tls(&Transport::Direct, false, true)
            .unwrap();
        http.client(&Transport::Direct, false).unwrap();
        http.client(&Transport::Direct, false).unwrap();
        let clients = http.clients.lock().unwrap();
        assert_eq!(clients.len(), 2);
        assert!(clients[0].2);
        assert!(!clients[1].2);
    }
}
