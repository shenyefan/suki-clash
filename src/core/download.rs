//! Bounded HTTP transfers. Dropping a transfer cancels the request and removes its temporary file.
pub(super) use crate::network::Transport as SubscriptionTransport;
use crate::{
    network::Http,
    storage::{AppPaths, DirectoryKind},
};
use reqwest::{
    RequestBuilder,
    header::{AUTHORIZATION, HeaderMap, HeaderValue},
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use tempfile::TempDir;
use tokio::io::AsyncWriteExt as _;
pub(super) const ARCHIVE_LIMIT: u64 = 256 * 1024 * 1024;
pub(super) struct Transfer {
    request: Option<RequestBuilder>,
    path: PathBuf,
    limit: u64,
    timeout: Duration,
    allow_empty: bool,
    headers: HeaderMap,
    _directory: TempDir,
}
impl Transfer {
    pub(super) fn start(
        http: &Http,
        paths: &AppPaths,
        url: &str,
        limit: u64,
    ) -> Result<Self, String> {
        let parsed = url::Url::parse(url)
            .map_err(|error| crate::logging::diagnostic_error("内核下载地址无效", error))?;
        if parsed.scheme() != "https"
            || !matches!(parsed.host_str(), Some("github.com" | "api.github.com"))
        {
            return Err("仅允许官方 GitHub HTTPS 下载地址".into());
        }
        Self::request(
            http,
            paths,
            Request {
                url,
                agent: crate::network::DEFAULT_USER_AGENT,
                authorization: "",
                timeout: Duration::from_secs(180),
                limit,
                allow_empty: false,
            },
            &SubscriptionTransport::System,
            true,
            false,
        )
    }
    pub(super) fn icon(http: &Http, paths: &AppPaths, url: &str) -> Result<Self, String> {
        Self::request(
            http,
            paths,
            Request {
                url,
                agent: crate::network::DEFAULT_USER_AGENT,
                authorization: "",
                timeout: Duration::from_secs(15),
                limit: 2 * 1024 * 1024,
                allow_empty: false,
            },
            &SubscriptionTransport::System,
            false,
            false,
        )
    }
    pub(super) fn resource(
        http: &Http,
        paths: &AppPaths,
        url: &str,
        limit: u64,
    ) -> Result<Self, String> {
        Self::request(
            http,
            paths,
            Request {
                url,
                agent: crate::network::DEFAULT_USER_AGENT,
                authorization: "",
                timeout: Duration::from_secs(180),
                limit,
                allow_empty: false,
            },
            &SubscriptionTransport::System,
            false,
            false,
        )
    }
    pub(crate) fn subscription(
        http: &Http,
        paths: &AppPaths,
        document: &crate::profiles::model::Document,
        agent: &str,
        transport: &SubscriptionTransport,
    ) -> Result<Self, String> {
        let timeout = Duration::from_secs(
            document
                .timeout
                .parse()
                .map_err(|error| crate::logging::diagnostic_error("下载超时无效", error))?,
        );
        Self::request(
            http,
            paths,
            Request {
                url: &document.url,
                agent: if document.user_agent.is_empty() {
                    agent
                } else {
                    &document.user_agent
                },
                authorization: &document.token,
                timeout,
                limit: crate::configuration::INPUT_LIMIT,
                allow_empty: false,
            },
            transport,
            false,
            document.accept_invalid_certs,
        )
    }
    pub(super) fn probe(
        http: &Http,
        paths: &AppPaths,
        url: &str,
        timeout: Duration,
        transport: &SubscriptionTransport,
    ) -> Result<Self, String> {
        Self::request(
            http,
            paths,
            Request {
                url,
                agent: crate::network::DEFAULT_USER_AGENT,
                authorization: "",
                timeout,
                limit: 64 * 1024,
                allow_empty: true,
            },
            transport,
            false,
            false,
        )
    }
    fn request(
        http: &Http,
        paths: &AppPaths,
        request: Request<'_>,
        transport: &SubscriptionTransport,
        https_only: bool,
        accept_invalid_certs: bool,
    ) -> Result<Self, String> {
        if !crate::profiles::model::valid_remote_url(request.url) {
            return Err("HTTP 请求地址无效".into());
        }
        if !(Duration::from_millis(100)..=Duration::from_secs(3600)).contains(&request.timeout) {
            return Err("HTTP 请求超时需为 100 毫秒–3600 秒".into());
        }
        if request.agent.len() > 8192 || request.authorization.len() > 8192 {
            return Err("HTTP 请求标头超过长度上限".into());
        }
        let agent = header(request.agent)?;
        let client = if accept_invalid_certs {
            http.client_with_tls(transport, https_only, true)?
        } else {
            http.client(transport, https_only)?
        };
        let timeout = request.timeout;
        let url = crate::network::github_proxy_url(paths, request.url, request.authorization)?;
        let mut builder = client
            .get(url)
            .header(reqwest::header::USER_AGENT, agent)
            .timeout(timeout);
        if !request.authorization.is_empty() {
            let mut value = header(request.authorization)?;
            value.set_sensitive(true);
            builder = builder.header(AUTHORIZATION, value);
        }
        let directory = tempfile::Builder::new()
            .prefix("http-")
            .tempdir_in(paths.directory(DirectoryKind::Cache))
            .map_err(|error| crate::logging::diagnostic_error("无法创建 HTTP 临时目录", error))?;
        Ok(Self {
            request: Some(builder),
            path: directory.path().join("response"),
            limit: request.limit,
            timeout,
            allow_empty: request.allow_empty,
            headers: HeaderMap::new(),
            _directory: directory,
        })
    }
    pub(crate) fn response_headers(&self) -> HeaderMap {
        self.headers.clone()
    }
    pub(super) async fn finish(
        &mut self,
        mut progress: impl FnMut(u64, Option<u64>),
    ) -> Result<Vec<u8>, String> {
        let request = self.request.take().ok_or("HTTP 请求已执行或取消")?;
        let work = async {
            let mut response = request
                .send()
                .await
                .map_err(crate::network::error)?
                .error_for_status()
                .map_err(crate::network::error)?;
            if response
                .headers()
                .iter()
                .map(|(name, value)| name.as_str().len() + value.as_bytes().len())
                .sum::<usize>()
                > 64 * 1024
            {
                return Err("HTTP 响应头超过上限".into());
            }
            self.headers = response.headers().clone();
            let total = response.content_length();
            if total.is_some_and(|size| size > self.limit) {
                return Err("下载超过大小上限".into());
            }
            progress(0, total);
            let mut file = tokio::fs::File::create(&self.path)
                .await
                .map_err(|error| crate::logging::diagnostic_error("无法创建下载临时文件", error))?;
            let mut bytes = Vec::new();
            let mut last = Instant::now();
            while let Some(chunk) = response.chunk().await.map_err(crate::network::error)? {
                if (bytes.len() as u64).saturating_add(chunk.len() as u64) > self.limit {
                    return Err("下载超过大小上限".into());
                }
                file.write_all(&chunk).await.map_err(|error| {
                    crate::logging::diagnostic_error("下载临时文件写入失败", error)
                })?;
                bytes.extend_from_slice(&chunk);
                if last.elapsed() >= Duration::from_millis(100) {
                    progress(bytes.len() as u64, total);
                    last = Instant::now();
                }
            }
            if !self.allow_empty && bytes.is_empty() {
                return Err("下载内容为空".into());
            }
            file.sync_all()
                .await
                .map_err(|error| crate::logging::diagnostic_error("下载临时文件同步失败", error))?;
            progress(bytes.len() as u64, total);
            Ok(bytes)
        };
        tokio::time::timeout(self.timeout, work)
            .await
            .map_err(|error| crate::logging::diagnostic_error("HTTP 请求超时".to_owned(), error))?
    }
}
struct Request<'a> {
    url: &'a str,
    agent: &'a str,
    authorization: &'a str,
    timeout: Duration,
    limit: u64,
    allow_empty: bool,
}
fn header(value: &str) -> Result<HeaderValue, String> {
    if value.chars().any(char::is_control) {
        return Err("HTTP 请求标头含无效字符".into());
    }
    HeaderValue::from_str(value)
        .map_err(|error| crate::logging::diagnostic_error("HTTP 请求标头无效".into(), error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{io::AsyncReadExt, net::TcpListener};
    async fn response(bytes: &[u8]) -> (String, tokio::task::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let bytes = bytes.to_vec();
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 1024];
            while !request.windows(4).any(|value| value == b"\r\n\r\n") {
                let read = socket.read(&mut buffer).await.unwrap();
                assert!(read > 0);
                request.extend_from_slice(&buffer[..read]);
            }
            socket.write_all(&bytes).await.unwrap();
            String::from_utf8(request).unwrap()
        });
        (format!("http://{address}/file"), task)
    }
    #[tokio::test]
    async fn stream_limits_apply_to_chunked_bodies_and_progress_finishes_at_received_bytes() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        paths.create().unwrap();
        let http = Http::default();
        let (url,server)=response(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n3\r\nabc\r\n3\r\ndef\r\n0\r\n\r\n").await;
        let mut transfer = Transfer::request(
            &http,
            &paths,
            Request {
                url: &url,
                agent: crate::network::DEFAULT_USER_AGENT,
                authorization: "",
                timeout: Duration::from_secs(2),
                limit: 6,
                allow_empty: false,
            },
            &SubscriptionTransport::Direct,
            false,
            false,
        )
        .unwrap();
        let mut progress = Vec::new();
        assert_eq!(
            transfer
                .finish(|received, total| progress.push((received, total)))
                .await
                .unwrap(),
            b"abcdef"
        );
        assert_eq!(progress.last(), Some(&(6, None)));
        server.await.unwrap();
        let (url,server)=response(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n7\r\nabcdefg\r\n0\r\n\r\n").await;
        let mut transfer = Transfer::request(
            &http,
            &paths,
            Request {
                url: &url,
                agent: crate::network::DEFAULT_USER_AGENT,
                authorization: "",
                timeout: Duration::from_secs(2),
                limit: 6,
                allow_empty: false,
            },
            &SubscriptionTransport::Direct,
            false,
            false,
        )
        .unwrap();
        assert!(
            transfer
                .finish(|_, _| {})
                .await
                .unwrap_err()
                .contains("大小上限")
        );
        server.await.unwrap();
    }
    #[tokio::test]
    async fn redirects_strip_cross_origin_credentials_and_publish_only_final_headers() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        paths.create().unwrap();
        let http = Http::default();
        let (final_url,final_server)=response(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nProfile-Update-Interval: 6\r\nConnection: close\r\n\r\nok").await;
        let redirect = format!(
            "HTTP/1.1 302 Found\r\nLocation: {final_url}\r\nProfile-Update-Interval: 1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        let (url, first_server) = response(redirect.as_bytes()).await;
        let mut transfer = Transfer::request(
            &http,
            &paths,
            Request {
                url: &url,
                agent: crate::network::DEFAULT_USER_AGENT,
                authorization: "Bearer secret",
                timeout: Duration::from_secs(2),
                limit: 1024,
                allow_empty: false,
            },
            &SubscriptionTransport::Direct,
            false,
            false,
        )
        .unwrap();
        assert_eq!(transfer.finish(|_, _| {}).await.unwrap(), b"ok");
        assert!(
            first_server
                .await
                .unwrap()
                .to_lowercase()
                .contains("authorization: bearer secret")
        );
        assert!(
            !final_server
                .await
                .unwrap()
                .to_lowercase()
                .contains("authorization:")
        );
        assert_eq!(transfer.response_headers()["profile-update-interval"], "6");
    }
    #[tokio::test]
    async fn status_errors_redact_request_and_response_credentials_and_invalid_headers_never_start()
    {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        paths.create().unwrap();
        let http = Http::default();
        let (url, server) = response(
            b"HTTP/1.1 403 Forbidden\r\nContent-Length: 6\r\nConnection: close\r\n\r\nsecret",
        )
        .await;
        let url = format!("{url}?token=secret");
        let mut transfer = Transfer::request(
            &http,
            &paths,
            Request {
                url: &url,
                agent: crate::network::DEFAULT_USER_AGENT,
                authorization: "Bearer secret",
                timeout: Duration::from_secs(2),
                limit: 1024,
                allow_empty: false,
            },
            &SubscriptionTransport::Direct,
            false,
            false,
        )
        .unwrap();
        let error = transfer.finish(|_, _| {}).await.unwrap_err();
        assert!(error.contains("403"));
        assert!(!error.contains("secret"));
        server.await.unwrap();
        for agent in ["bad\nheader: inject", "bad\rheader", "bad\0", "bad\tvalue"] {
            assert!(
                Transfer::request(
                    &http,
                    &paths,
                    Request {
                        url: "http://127.0.0.1/file",
                        agent,
                        authorization: "",
                        timeout: Duration::from_secs(2),
                        limit: 1024,
                        allow_empty: false
                    },
                    &SubscriptionTransport::Direct,
                    false,
                    false
                )
                .is_err()
            );
        }
    }
    #[tokio::test]
    async fn cancelling_a_request_closes_its_socket_and_removes_temporary_files() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        paths.create().unwrap();
        let http = Http::default();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/wait", listener.local_addr().unwrap());
        let mut transfer = Transfer::request(
            &http,
            &paths,
            Request {
                url: &url,
                agent: crate::network::DEFAULT_USER_AGENT,
                authorization: "",
                timeout: Duration::from_secs(2),
                limit: 1024,
                allow_empty: false,
            },
            &SubscriptionTransport::Direct,
            false,
            false,
        )
        .unwrap();
        let directory = transfer._directory.path().to_path_buf();
        let task = tokio::spawn(async move { transfer.finish(|_, _| {}).await });
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buffer = [0; 4096];
        assert!(socket.read(&mut buffer).await.unwrap() > 0);
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(!directory.exists());
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), socket.read(&mut buffer))
                .await
                .unwrap()
                .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn unsafe_redirects_fail_without_sending_credentials_or_retrying() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        paths.create().unwrap();
        for destination in [
            "ftp://127.0.0.1/private",
            "http://user:password@127.0.0.1/private",
        ] {
            let reply = format!(
                "HTTP/1.1 302 Found\r\nLocation: {destination}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            let (url, server) = response(reply.as_bytes()).await;
            let mut transfer = Transfer::request(
                &Http::default(),
                &paths,
                Request {
                    url: &url,
                    agent: crate::network::DEFAULT_USER_AGENT,
                    authorization: "Bearer private-token",
                    timeout: Duration::from_secs(2),
                    limit: 1024,
                    allow_empty: false,
                },
                &SubscriptionTransport::Direct,
                false,
                false,
            )
            .unwrap();
            let failure = transfer.finish(|_, _| {}).await.unwrap_err();
            assert_eq!(failure, "HTTP 重定向失败");
            assert!(!failure.contains("private-token"));
            assert!(!failure.contains("password"));
            assert!(server.await.unwrap().starts_with("GET /file "));
            assert_eq!(
                transfer.finish(|_, _| {}).await.unwrap_err(),
                "HTTP 请求已执行或取消"
            );
        }
    }
}
