//! Reusable HTTP clients owned by the application worker, separated by transport policy.
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
        let mut clients = self.clients.lock().map_err(|_| "HTTP 客户端状态不可用")?;
        // System proxy configuration may change outside this application. Resolve it for each task.
        if !matches!(transport, Transport::System)
            && let Some((_, _, _, client)) = clients.iter().find(|(route, secure, invalid, _)| {
                route == transport && *secure == https_only && *invalid == accept_invalid_certs
            })
        {
            return Ok(client.clone());
        }
        let mut builder = Client::builder()
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
                let mut proxy = Proxy::all(url).map_err(|_| "HTTP 代理地址无效")?;
                if let Some(authentication) = authentication {
                    let (user, password) =
                        authentication.split_once(':').ok_or("代理凭据格式无效")?;
                    proxy = proxy.basic_auth(user, password);
                }
                builder = builder.no_proxy().proxy(proxy);
            }
        }
        let client = builder.build().map_err(|_| "无法创建 HTTP 客户端")?;
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
