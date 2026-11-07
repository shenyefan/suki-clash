use http_body_util::{BodyExt as _, Full};
use hyper::{Method, Request, body::Bytes, client::conn::http1};
use hyper_util::rt::TokioIo;
use serde::Deserialize;
#[cfg(unix)]
use std::path::PathBuf;
use std::{io, time::Duration};
use tokio::io::{AsyncRead, AsyncWrite};

const RESPONSE_LIMIT: usize = 8 * 1024 * 1024;
pub(super) const CONFIGURATION_READY_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub(super) enum Endpoint {
    #[cfg(unix)]
    Unix(PathBuf),
    #[cfg(windows)]
    Pipe(String),
}

#[derive(Clone)]
pub(super) struct Client {
    endpoint: Endpoint,
    // Deliberately no Debug implementation: authentication never enters diagnostics.
    secret: String,
    scopes: std::collections::BTreeMap<String, crate::proxies::runtime::GroupScope>,
}

#[derive(Deserialize)]
struct Version {
    version: String,
}

pub(super) trait Ipc: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Ipc for T {}
pub(super) type Socket = tokio_tungstenite::WebSocketStream<Box<dyn Ipc>>;
impl Client {
    async fn connect(&self) -> Result<Box<dyn Ipc>, String> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        loop {
            let result: io::Result<Box<dyn Ipc>> = match &self.endpoint {
                #[cfg(unix)]
                Endpoint::Unix(path) => tokio::net::UnixStream::connect(path)
                    .await
                    .map(|stream| Box::new(stream) as Box<dyn Ipc>),
                #[cfg(windows)]
                Endpoint::Pipe(path) => tokio::net::windows::named_pipe::ClientOptions::new()
                    .open(path)
                    .map(|stream| Box::new(stream) as Box<dyn Ipc>),
            };
            match result {
                Ok(stream) => return Ok(stream),
                Err(error) => {
                    let transient = matches!(
                        error.kind(),
                        io::ErrorKind::NotFound
                            | io::ErrorKind::ConnectionRefused
                            | io::ErrorKind::ConnectionAborted
                    );
                    #[cfg(windows)]
                    let transient = transient
                        || error.raw_os_error()
                            == Some(windows_sys::Win32::Foundation::ERROR_PIPE_BUSY as i32);
                    if !transient || tokio::time::Instant::now() >= deadline {
                        return Err(format!("连接内核本地通道失败：{error}"));
                    }
                    // Retry only opening the channel. No request has been sent,
                    // so writes cannot be repeated after a lost acknowledgement.
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    }
    pub(super) async fn websocket(&self, path: &str) -> Result<Socket, String> {
        use tokio_tungstenite::tungstenite::{
            client::IntoClientRequest, http::HeaderValue, protocol::WebSocketConfig,
        };
        let work = async {
            let stream = self.connect().await?;
            let mut request = format!("ws://localhost{path}")
                .into_client_request()
                .map_err(|error| {
                    crate::logging::diagnostic_error("无法构造内核 WebSocket 请求", error)
                })?;
            let mut authorization = HeaderValue::from_str(&format!("Bearer {}", self.secret))
                .map_err(|error| crate::logging::rejected("内核鉴权配置无效", error))?;
            authorization.set_sensitive(true);
            request.headers_mut().insert("Authorization", authorization);
            let config = WebSocketConfig::default()
                .max_message_size(Some(RESPONSE_LIMIT))
                .max_frame_size(Some(RESPONSE_LIMIT));
            let (socket, _) =
                tokio_tungstenite::client_async_with_config(request, stream, Some(config))
                    .await
                    .map_err(|error| {
                        log::debug!(
                            "内核 WebSocket 握手失败（{}）",
                            crate::logging::error_chain(&error)
                        );
                        match error {
                            tokio_tungstenite::tungstenite::Error::Http(response) => {
                                format!("内核 WebSocket 返回 HTTP {}", response.status().as_u16())
                            }
                            _ => "内核 WebSocket 握手失败".into(),
                        }
                    })?;
            Ok(socket)
        };
        tokio::time::timeout(Duration::from_secs(5), work)
            .await
            .map_err(|error| {
                crate::logging::diagnostic_error("内核 WebSocket 连接超时".to_owned(), error)
            })?
    }
    pub(super) async fn close_connections(&self, id: Option<&str>) -> Result<(), String> {
        let path = id
            .map(|id| format!("/connections/{}", super::resources::escaped_name(id)))
            .unwrap_or_else(|| "/connections".into());
        self.request(Method::DELETE, &path, Duration::from_secs(5))
            .await
            .map(|_| ())
    }
    pub(super) async fn json(&self, path: &str) -> Result<serde_json::Value, String> {
        serde_json::from_slice(&self.get(path).await?)
            .map_err(|error| crate::logging::diagnostic_error("内核 JSON 响应无效".into(), error))
    }
    pub(super) async fn rules(
        &self,
        generation: u64,
        epoch: u64,
    ) -> Result<crate::traffic::rules::Snapshot, String> {
        crate::traffic::rules::Snapshot::parse(&self.get("/rules").await?, generation, epoch)
    }
    pub(super) async fn proxies(
        &self,
        order: &[String],
    ) -> Result<crate::proxies::runtime::Proxies, String> {
        let (proxies, providers) =
            tokio::join!(self.get("/proxies"), self.get("/providers/proxies"));
        let mut proxies = crate::proxies::runtime::Proxies::parse(&proxies?, order)?;
        proxies.scopes = self.scopes.clone();
        match providers {
            Ok(bytes) => {
                if let Err(error) = proxies.include_providers(&bytes) {
                    proxies.provider_error = Some(error);
                }
            }
            Err(error) => proxies.provider_error = Some(error),
        }
        proxies.resolve_members();
        Ok(proxies)
    }
    pub(super) fn with_proxy_config(mut self, config: &serde_yaml_ng::Value) -> Self {
        let all_providers = config
            .get("proxy-providers")
            .and_then(serde_yaml_ng::Value::as_mapping)
            .into_iter()
            .flat_map(|providers| providers.keys())
            .filter_map(|value| value.as_str().map(str::to_owned))
            .collect::<std::collections::BTreeSet<_>>();
        self.scopes = config
            .get("proxy-groups")
            .and_then(serde_yaml_ng::Value::as_sequence)
            .into_iter()
            .flatten()
            .filter_map(|group| {
                let name = group.get("name")?.as_str()?.to_owned();
                let providers = if ["include-all", "include-all-providers"].iter().any(|key| {
                    group.get(*key).and_then(serde_yaml_ng::Value::as_bool) == Some(true)
                }) {
                    all_providers.iter().cloned().collect()
                } else {
                    group
                        .get("use")
                        .and_then(serde_yaml_ng::Value::as_sequence)
                        .into_iter()
                        .flatten()
                        .filter_map(|value| value.as_str().map(str::to_owned))
                        .collect()
                };
                let exclude_types = group
                    .get("exclude-type")
                    .and_then(serde_yaml_ng::Value::as_str)
                    .unwrap_or_default()
                    .split('|')
                    .map(str::trim)
                    .filter(|kind| !kind.is_empty())
                    .map(str::to_owned)
                    .collect();
                Some((
                    name,
                    crate::proxies::runtime::GroupScope {
                        providers,
                        exclude_types,
                    },
                ))
            })
            .collect();
        self
    }
    pub(super) async fn select_proxy(&self, group: &str, node: Option<&str>) -> Result<(), String> {
        let path = format!("/proxies/{}", super::resources::escaped_name(group));
        if let Some(node) = node {
            self.request_body(
                Method::PUT,
                &path,
                Duration::from_secs(5),
                serde_json::to_vec(&serde_json::json!({"name":node}))
                    .map_err(|error| crate::logging::rejected("节点名称无效", error))?,
            )
            .await?;
        } else {
            self.request(Method::DELETE, &path, Duration::from_secs(5))
                .await?;
        }
        Ok(())
    }
    pub(super) async fn patch_mode(&self, mode: crate::session::ProxyMode) -> Result<(), String> {
        self.request_body(
            Method::PATCH,
            "/configs",
            Duration::from_secs(5),
            serde_json::to_vec(&serde_json::json!({"mode":mode.label()}))
                .map_err(|error| crate::logging::rejected("模式无效", error))?,
        )
        .await
        .map(|_| ())
    }
    pub(super) async fn node_delay(
        &self,
        name: &str,
        provider: Option<&str>,
        url: &str,
        timeout: u64,
    ) -> Result<u32, crate::proxies::runtime::MeasurementError> {
        use crate::proxies::runtime::MeasurementError;
        let query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("url", url)
            .append_pair("timeout", &timeout.to_string())
            .finish();
        let name = super::resources::escaped_name(name);
        let path = match provider {
            Some(provider) => format!(
                "/providers/proxies/{}/{name}/healthcheck?{query}",
                super::resources::escaped_name(provider)
            ),
            None => format!("/proxies/{name}/delay?{query}"),
        };
        let (status, body) = tokio::time::timeout(Duration::from_millis(timeout), async {
            self.exchange_response(self.connect().await?, Method::GET, &path, Vec::new())
                .await
        })
        .await
        .map_err(|error| {
            log::debug!("节点测速超时（{error}）");
            MeasurementError::Timeout
        })?
        .map_err(MeasurementError::Failed)?;
        if status == hyper::StatusCode::GATEWAY_TIMEOUT
            || status == hyper::StatusCode::REQUEST_TIMEOUT
        {
            return Err(MeasurementError::Timeout);
        }
        if !status.is_success() {
            return Err(MeasurementError::Failed(format!(
                "内核本地 API 返回 HTTP {}",
                status.as_u16()
            )));
        }
        #[derive(Deserialize)]
        struct Delay {
            delay: u32,
        }
        serde_json::from_slice::<Delay>(&body)
            .map(|value| value.delay)
            .map_err(|error| {
                MeasurementError::Failed(crate::logging::diagnostic_error(
                    "延迟测试响应无效".into(),
                    error,
                ))
            })
    }
    pub(super) async fn tun_enabled(&self) -> Result<bool, String> {
        let config = self.json("/configs").await?;
        Self::read_tun_enabled(&config)
    }
    fn read_tun_enabled(config: &serde_json::Value) -> Result<bool, String> {
        config
            .get("tun")
            .and_then(|tun| tun.get("enable"))
            .and_then(serde_json::Value::as_bool)
            .ok_or_else(|| "内核未返回虚拟网卡实际状态".into())
    }
    pub(super) async fn verify_tun(&self, expected: &serde_yaml_ng::Value) -> Result<bool, String> {
        let desired = crate::configuration::tun::enabled(expected)?;
        let config = self.json("/configs").await?;
        let actual = Self::read_tun_enabled(&config)?;
        if desired != actual {
            return Err(format!(
                "内核虚拟网卡状态与请求不一致（期望 {desired}，实际 {actual}）"
            ));
        }
        #[cfg(target_os = "macos")]
        if actual
            && let Some(expected_mode) = expected
                .get("tun")
                .and_then(|tun| tun.get("dns-mode"))
                .and_then(serde_yaml_ng::Value::as_str)
                .filter(|mode| !mode.is_empty())
            && config
                .get("tun")
                .and_then(|tun| tun.get("dns-mode"))
                .and_then(serde_json::Value::as_str)
                != Some(expected_mode)
        {
            return Err("内核未确认虚拟网卡 DNS 模式，请更新内核或修正配置".into());
        }
        Ok(actual)
    }

    /// The IPC endpoint can precede asynchronous TUN adapter initialization.
    /// Confirm the actual state within a bounded window before committing or rolling back.
    pub(super) async fn wait_configuration_ready(
        &self,
        expected: &serde_yaml_ng::Value,
        budget: Duration,
    ) -> Result<bool, String> {
        let mut last_error = None;
        let ready = tokio::time::timeout(budget, async {
            loop {
                let result = async {
                    self.verify_listeners(expected).await?;
                    self.verify_tun(expected).await
                }
                .await;
                match result {
                    Ok(actual) => return actual,
                    Err(error) => last_error = Some(error),
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await;
        ready.map_err(|_| last_error.unwrap_or_else(|| "内核配置就绪检查超时".into()))
    }
    pub(super) async fn reload_file(&self, path: &str) -> Result<(), String> {
        let body = serde_json::to_vec(&serde_json::json!({"path":path}))
            .map_err(|error| crate::logging::diagnostic_error("无法编码配置路径", error))?;
        self.request_body(
            Method::PUT,
            "/configs?force=true",
            Duration::from_secs(15),
            body,
        )
        .await
        .map(|_| ())
    }
    pub(super) async fn reload(&self, yaml: &str) -> Result<(), String> {
        let body = serde_json::to_vec(&serde_json::json!({"payload": yaml}))
            .map_err(|error| crate::logging::diagnostic_error("无法编码热更新请求", error))?;
        self.request_body(
            Method::PUT,
            "/configs?force=true",
            Duration::from_secs(15),
            body,
        )
        .await
        .map(|_| ())
    }
    pub(super) async fn proxy_names(&self) -> Result<Vec<String>, String> {
        let body = self.get("/proxies").await?;
        let value: serde_json::Value = serde_json::from_slice(&body)
            .map_err(|error| crate::logging::diagnostic_error("代理列表响应无效", error))?;
        let proxies = value
            .get("proxies")
            .and_then(serde_json::Value::as_object)
            .ok_or("代理列表为空或无效")?;
        Ok(proxies.keys().cloned().collect())
    }
    pub(super) async fn verify_listeners(
        &self,
        expected: &serde_yaml_ng::Value,
    ) -> Result<(), String> {
        let response = self.get("/configs").await?;
        let actual: serde_json::Value = serde_json::from_slice(&response)
            .map_err(|error| crate::logging::diagnostic_error("内核监听配置回读无效", error))?;
        for key in [
            "mixed-port",
            "port",
            "socks-port",
            "redir-port",
            "tproxy-port",
        ] {
            let expected = expected
                .get(key)
                .and_then(serde_yaml_ng::Value::as_u64)
                .unwrap_or(0);
            let actual = actual
                .get(key)
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            if expected != actual {
                return Err(format!(
                    "{key} 未按候选配置加载（期望 {expected}，实际 {actual}）"
                ));
            }
        }
        for key in [
            "ipv6",
            "allow-lan",
            "tcp-concurrent",
            "find-process-mode",
            "log-level",
        ] {
            if let Some(expected) = expected.get(key) {
                let expected = serde_json::to_value(expected)
                    .map_err(|error| crate::logging::diagnostic_error("无法编码回读设置", error))?;
                if actual.get(key) != Some(&expected) {
                    return Err(format!("{key} 未按候选配置加载"));
                }
            }
        }
        Ok(())
    }
    pub(super) async fn flush_fake_ip(&self) -> Result<(), String> {
        self.request(Method::POST, "/cache/fakeip/flush", Duration::from_secs(5))
            .await
            .map(|_| ())
    }
    pub(super) async fn providers(
        &self,
        kind: super::ProviderKind,
    ) -> Result<Vec<super::resources::Provider>, String> {
        let bytes = self
            .request(Method::GET, kind.path(), Duration::from_secs(5))
            .await?;
        super::resources::parse(&bytes, kind)
    }
    pub(super) async fn update_provider(
        &self,
        kind: super::ProviderKind,
        name: &str,
    ) -> Result<(), String> {
        let path = format!("{}/{}", kind.path(), super::resources::escaped_name(name));
        self.request(Method::PUT, &path, Duration::from_secs(120))
            .await
            .map(|_| ())
    }
    pub(super) async fn update_geo(&self) -> Result<(), String> {
        self.request(Method::POST, "/configs/geo", Duration::from_secs(75))
            .await
            .map(|_| ())
    }
    pub(super) fn set_endpoint(&mut self, endpoint: Endpoint) {
        self.endpoint = endpoint;
    }
    pub(super) fn new(endpoint: Endpoint, secret: String) -> Self {
        Self {
            endpoint,
            secret,
            scopes: Default::default(),
        }
    }

    pub(super) async fn version(&self) -> Result<String, String> {
        let response = self.get("/version").await?;
        let response: Version = serde_json::from_slice(&response).map_err(|error| {
            crate::logging::diagnostic_error("内核版本响应格式无效".to_owned(), error)
        })?;
        if response.version.trim().is_empty() || response.version.len() > 128 {
            return Err("内核版本响应无效".into());
        }
        Ok(response.version)
    }

    async fn get(&self, path: &str) -> Result<Vec<u8>, String> {
        self.request(Method::GET, path, Duration::from_secs(1))
            .await
    }
    async fn request(
        &self,
        method: Method,
        path: &str,
        timeout: Duration,
    ) -> Result<Vec<u8>, String> {
        self.request_body(method, path, timeout, Vec::new()).await
    }

    async fn request_body(
        &self,
        method: Method,
        path: &str,
        timeout: Duration,
        payload: Vec<u8>,
    ) -> Result<Vec<u8>, String> {
        let stream = self.connect().await?;
        tokio::time::timeout(timeout, self.exchange(stream, method, path, payload))
            .await
            .map_err(|_| "内核本地 API 请求超时；服务端操作可能仍在执行，请勿重复提交".to_owned())?
    }

    async fn exchange<S>(
        &self,
        stream: S,
        method: Method,
        path: &str,
        payload: Vec<u8>,
    ) -> Result<Vec<u8>, String>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let (status, body) = self
            .exchange_response(stream, method, path, payload)
            .await?;
        if !status.is_success() {
            return Err(format!("内核本地 API 返回 HTTP {}", status.as_u16()));
        }
        Ok(body)
    }
    async fn exchange_response<S>(
        &self,
        stream: S,
        method: Method,
        path: &str,
        payload: Vec<u8>,
    ) -> Result<(hyper::StatusCode, Vec<u8>), String>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let (mut sender, connection) =
            http1::handshake(TokioIo::new(stream))
                .await
                .map_err(|error| {
                    format!(
                        "内核本地 HTTP 握手失败：{}",
                        crate::logging::error_chain(&error)
                    )
                })?;
        // Drive the connection in the same future. Timeout/cancellation drops both
        // halves; no detached connection task can survive its generation.
        let request = async {
            let request = Request::builder()
                .method(method)
                .uri(path)
                .header("Host", "localhost")
                .header("Authorization", format!("Bearer {}", self.secret))
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(payload)))
                .map_err(|error| {
                    crate::logging::diagnostic_error("无法构造内核请求".to_owned(), error)
                })?;
            let response = sender.send_request(request).await.map_err(|error| {
                format!(
                    "内核本地 API 传输失败：{}",
                    crate::logging::error_chain(&error)
                )
            })?;
            let status = response.status();
            let mut body = response.into_body();
            let mut bytes = Vec::new();
            while let Some(frame) = body.frame().await {
                let frame = frame.map_err(|error| {
                    format!("内核响应读取失败：{}", crate::logging::error_chain(&error))
                })?;
                if let Ok(data) = frame.into_data() {
                    if bytes.len().saturating_add(data.len()) > RESPONSE_LIMIT {
                        return Err("内核响应超过大小限制".into());
                    }
                    bytes.extend_from_slice(&data);
                }
            }
            Ok((status, bytes))
        };
        tokio::pin!(request);
        tokio::pin!(connection);
        tokio::select! {
            biased;
            result = &mut request => result,
            result = &mut connection => {
                result.map_err(|error| format!("内核本地连接中断：{error}"))?;
                request.await
            }
        }
    }
}

pub(super) fn random_token() -> Result<String, io::Error> {
    let mut bytes = [0; 24];
    getrandom::fill(&mut bytes).map_err(io::Error::other)?;
    Ok(bytes.iter().map(|value| format!("{value:02x}")).collect())
}

#[cfg(all(test, unix))]
mod tests {
    use super::{Client, Endpoint};
    use std::time::Duration;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    #[tokio::test]
    async fn delayed_channel_waits_before_sending_and_never_replays_a_write() {
        let root = tempfile::tempdir().unwrap();
        let socket = root.path().join("delayed.sock");
        let server_path = socket.clone();
        let server = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let listener = tokio::net::UnixListener::bind(server_path).unwrap();
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            let read = stream.read(&mut request).await.unwrap();
            assert!(request[..read].starts_with(b"PATCH /configs "));
            // The write reached the core, but its acknowledgement was lost.
            drop(stream);
            assert!(
                tokio::time::timeout(Duration::from_millis(200), listener.accept())
                    .await
                    .is_err()
            );
        });
        let client = Client::new(Endpoint::Unix(socket), String::new());
        assert!(
            client
                .patch_mode(crate::session::ProxyMode::Direct)
                .await
                .is_err()
        );
        server.await.unwrap();
        let result = tokio::time::timeout(Duration::from_secs(4), client.version())
            .await
            .unwrap();
        assert!(result.unwrap_err().contains("连接内核本地通道失败"));
    }

    #[tokio::test]
    async fn measurement_classification_uses_status_and_deadline_not_error_text() {
        use crate::proxies::runtime::MeasurementError;
        for (status, body, expected) in [
            (200, r#"{"delay":15000}"#, Ok(15000)),
            (200, r#"{"delay":0}"#, Ok(0)),
            (
                504,
                r#"{"message":"anything"}"#,
                Err(MeasurementError::Timeout),
            ),
            (
                500,
                r#"{"message":"超时"}"#,
                Err(MeasurementError::Failed(
                    "内核本地 API 返回 HTTP 500".into(),
                )),
            ),
        ] {
            let root = tempfile::tempdir_in("/tmp").unwrap();
            let endpoint = root.path().join("api.sock");
            let listener = tokio::net::UnixListener::bind(&endpoint).unwrap();
            let server = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut bytes = [0; 4096];
                let received = stream.read(&mut bytes).await.unwrap();
                assert!(received > 0);
                stream.write_all(format!("HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
            });
            let client = Client::new(Endpoint::Unix(endpoint), String::new());
            assert_eq!(
                client
                    .node_delay("node", None, "https://example.test", 1000)
                    .await,
                expected
            );
            server.await.unwrap();
        }
        let root = tempfile::tempdir_in("/tmp").unwrap();
        let endpoint = root.path().join("api.sock");
        let listener = tokio::net::UnixListener::bind(&endpoint).unwrap();
        let server = tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.unwrap();
            std::future::pending::<()>().await;
        });
        let client = Client::new(Endpoint::Unix(endpoint), String::new());
        assert_eq!(
            client
                .node_delay("node", None, "https://example.test", 100)
                .await,
            Err(MeasurementError::Timeout)
        );
        server.abort();
        let _ = server.await;
    }

    #[test]
    fn provider_exclusions_follow_pipe_delimited_types_when_resolving_duplicates() {
        let config = serde_yaml_ng::from_str(
            "proxy-groups:\n- name: g\n  use: [a, b, c]\n  exclude-type: ss|trojan\n",
        )
        .unwrap();
        let client = Client::new(Endpoint::Unix("/unused.sock".into()), String::new())
            .with_proxy_config(&config);
        let mut proxies = crate::proxies::runtime::Proxies::parse(
            br#"{"proxies":{"g":{"name":"g","type":"Selector","all":["n"]}}}"#,
            &[],
        )
        .unwrap();
        proxies.scopes = client.scopes;
        proxies.include_providers(br#"{"providers":{"a":{"proxies":[{"name":"n","type":"ss"}]},"b":{"proxies":[{"name":"n","type":"Trojan"}]},"c":{"proxies":[{"name":"n","type":"Socks5"}]}}}"#).unwrap();
        assert_eq!(
            proxies
                .member_proxy(&proxies.members["g"][0])
                .unwrap()
                .provider
                .as_deref(),
            Some("c")
        );
    }

    async fn respond(
        status: u16,
        body: &[u8],
        framing: &str,
        request_path: &'static str,
    ) -> (tempfile::TempDir, Client, tokio::task::JoinHandle<()>) {
        let root = tempfile::Builder::new()
            .prefix("suki-api-")
            .tempdir_in("/tmp")
            .unwrap();
        let path = root.path().join("api.sock");
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let response = format!("HTTP/1.1 {status} Test\r\n{framing}\r\n\r\n").into_bytes();
        let body = body.to_vec();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = vec![0; 4096];
            let read = stream.read(&mut bytes).await.unwrap();
            let request = String::from_utf8_lossy(&bytes[..read]);
            assert!(request.starts_with(&format!("GET {request_path} HTTP/1.1")));
            assert!(
                request
                    .to_lowercase()
                    .contains("authorization: bearer test-secret")
            );
            stream.write_all(&response).await.unwrap();
            stream.write_all(&body).await.unwrap();
        });
        (
            root,
            Client::new(Endpoint::Unix(path), "test-secret".into()),
            server,
        )
    }

    #[tokio::test]
    async fn version_uses_private_http_transport_and_supports_chunked_bodies() {
        let body = r#"{"version":"v1.test"}"#;
        let chunked = format!("{:x}\r\n{body}\r\n0\r\n\r\n", body.len());
        let (_root, client, server) = respond(
            200,
            chunked.as_bytes(),
            "Transfer-Encoding: chunked",
            "/version",
        )
        .await;
        assert_eq!(client.version().await.unwrap(), "v1.test");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn bad_status_and_invalid_bodies_do_not_expose_response_secrets() {
        for (status, body) in [
            (403, "secret-response"),
            (200, "secret-response"),
            (200, "{\"version\":\"\"}"),
        ] {
            let (_root, client, server) = respond(
                status,
                body.as_bytes(),
                &format!("Content-Length: {}", body.len()),
                "/version",
            )
            .await;
            let error = client.version().await.unwrap_err();
            assert!(!error.contains("secret-response"));
            assert!(!error.contains("test-secret"));
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn readiness_waits_for_listeners_and_tun_and_bounds_persistent_mismatch() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        for (field, becomes_ready) in [("tun", true), ("mixed-port", true), ("tun", false)] {
            let root = tempfile::tempdir().unwrap();
            let socket = root.path().join("tun-ready.sock");
            let listener = tokio::net::UnixListener::bind(&socket).unwrap();
            let requests = Arc::new(AtomicUsize::new(0));
            let count = requests.clone();
            let server = tokio::spawn(async move {
                loop {
                    let (mut stream, _) = listener.accept().await.unwrap();
                    let mut request = [0; 4096];
                    let read = stream.read(&mut request).await.unwrap();
                    assert!(request[..read].starts_with(b"GET /configs "));
                    let ready = becomes_ready && count.fetch_add(1, Ordering::SeqCst) >= 2;
                    let body = serde_json::json!({
                        "tun": {"enable": field != "tun" || ready},
                        "mixed-port": if field == "mixed-port" && !ready {0} else {7897},
                    })
                    .to_string();
                    stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
                }
            });
            let client = Client::new(Endpoint::Unix(socket), String::new());
            let expected =
                serde_yaml_ng::from_str("mixed-port: 7897\ntun: {enable: true}").unwrap();
            let result = tokio::time::timeout(
                Duration::from_secs(5),
                client.wait_configuration_ready(&expected, Duration::from_secs(1)),
            )
            .await
            .unwrap();
            if becomes_ready {
                assert!(result.unwrap());
                assert!(requests.load(Ordering::SeqCst) >= 3);
            } else {
                let error = result.unwrap_err();
                assert!(error.contains("期望 true，实际 false"), "{error}");
            }
            server.abort();
        }
    }

    #[tokio::test]
    async fn oversized_response_is_rejected() {
        let body = vec![b'x'; super::RESPONSE_LIMIT + 1];
        let (_root, client, server) = respond(
            200,
            &body,
            &format!("Content-Length: {}", body.len()),
            "/version",
        )
        .await;
        assert!(client.version().await.unwrap_err().contains("大小限制"));
        server.await.unwrap();
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn macos_tun_dns_requires_actual_mode_readback_not_only_unknown_field_acceptance() {
        let expected: serde_yaml_ng::Value =
            serde_yaml_ng::from_str("tun: {enable: true, dns-mode: native}").unwrap();
        for (body, succeeds) in [
            (r#"{"tun":{"enable":true,"dns-mode":"native"}}"#, true),
            (r#"{"tun":{"enable":true,"dns-mode":"disabled"}}"#, false),
            (r#"{"tun":{"enable":true}}"#, false),
            (r#"{"tun":{"enable":false,"dns-mode":"native"}}"#, false),
        ] {
            let (_root, client, server) = respond(
                200,
                body.as_bytes(),
                &format!("Content-Length: {}", body.len()),
                "/configs",
            )
            .await;
            assert_eq!(client.verify_tun(&expected).await.is_ok(), succeeds);
            server.await.unwrap();
        }
        // Turning TUN off must not require the feature that an older core ignored.
        let expected: serde_yaml_ng::Value =
            serde_yaml_ng::from_str("tun: {enable: false}").unwrap();
        let body = r#"{"tun":{"enable":false}}"#;
        let (_root, client, server) = respond(
            200,
            body.as_bytes(),
            &format!("Content-Length: {}", body.len()),
            "/configs",
        )
        .await;
        assert!(!client.verify_tun(&expected).await.unwrap());
        server.await.unwrap();
    }
}

#[cfg(all(test, windows))]
mod pipe_tests {
    use super::*;
    use tokio::net::windows::named_pipe::{ClientOptions, ServerOptions};
    #[tokio::test]
    async fn busy_pipe_connects_when_the_next_server_instance_is_available() {
        let path = format!(r"\\.\pipe\suki-client-test-{}", random_token().unwrap());
        let server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&path)
            .unwrap();
        let first = ClientOptions::new().open(&path).unwrap();
        server.connect().await.unwrap();
        let client = Client::new(Endpoint::Pipe(path.clone()), String::new());
        let waiting = tokio::spawn(async move { client.connect().await });
        tokio::time::sleep(Duration::from_millis(75)).await;
        assert!(!waiting.is_finished());
        drop(first);
        drop(server);
        let server = ServerOptions::new().create(&path).unwrap();
        let connection = waiting.await.unwrap().unwrap();
        server.connect().await.unwrap();
        drop(connection);
    }
}
