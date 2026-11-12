//! HTTP over the platform's private local stream; Hyper owns HTTP parsing and framing.
use anyhow::Result;
use http_body_util::{BodyExt as _, Full, Limited};
use hyper::{
    Request, Response, StatusCode,
    body::{Body as _, Bytes, Incoming},
    service::service_fn,
};
use hyper_util::rt::{TokioIo, TokioTimer};
use serde::de::DeserializeOwned;
use std::{convert::Infallible, path::Path, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::oneshot,
    task::JoinSet,
};

pub(super) type HttpResponse = Response<Full<Bytes>>;
pub(super) const HANDLER_TIMEOUT: Duration = Duration::from_secs(25);
const READ_TIMEOUT: Duration = Duration::from_secs(5);
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(40);
const MAX_BODY: usize = 10 * 1024 * 1024;
const MAX_HEADER_BYTES: usize = 4096;
const MAX_CONNECTIONS: usize = 128;

pub(super) struct RequestContext {
    pub(super) method: hyper::Method,
    pub(super) uri: hyper::Uri,
    pub(super) headers: hyper::HeaderMap,
    pub(super) body: Bytes,
    #[cfg(unix)]
    pub(super) peer: Option<(u32, u32)>,
}
impl RequestContext {
    pub(super) fn json<T: DeserializeOwned>(&self) -> serde_json::Result<T> {
        serde_json::from_slice(&self.body)
    }
}

#[cfg(unix)]
pub(super) type Listener = tokio::net::UnixListener;
#[cfg(windows)]
pub(super) struct Listener {
    pending: tokio::net::windows::named_pipe::NamedPipeServer,
    path: std::path::PathBuf,
    descriptor: &'static str,
}
#[cfg(windows)]
impl Listener {
    pub(super) fn bind(path: &Path, descriptor: &'static str) -> Result<Self> {
        Ok(Self {
            pending: super::windows_security::create_control_pipe(path, descriptor, true)?,
            path: path.to_owned(),
            descriptor,
        })
    }
    async fn accept(&mut self) -> Result<tokio::net::windows::named_pipe::NamedPipeServer> {
        self.pending.connect().await?;
        let next =
            super::windows_security::create_control_pipe(&self.path, self.descriptor, false)?;
        Ok(std::mem::replace(&mut self.pending, next))
    }
}

#[cfg(unix)]
pub(super) fn bind(path: &Path) -> Result<Listener> {
    use std::os::unix::fs::PermissionsExt as _;
    let listener = Listener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o666))?;
    Ok(listener)
}

pub(super) async fn serve(listener: Listener, mut shutdown: oneshot::Receiver<()>) -> Result<()> {
    #[cfg(windows)]
    let mut listener = listener;
    let mut connections = JoinSet::new();
    let result = loop {
        tokio::select! {
            biased;
            _ = &mut shutdown => break Ok(()),
            result = connections.join_next(), if !connections.is_empty() => {
                if let Some(Err(error)) = result {
                    tracing::error!("本地通道连接任务异常：{error}");
                }
            }
            accepted = listener.accept(), if connections.len() < MAX_CONNECTIONS => {
                let accepted = match accepted {
                    Ok(accepted) => accepted,
                    Err(error) => break Err(anyhow::anyhow!(error)),
                };
                #[cfg(unix)]
                let (stream, _) = accepted;
                #[cfg(windows)]
                let stream = accepted;
                #[cfg(unix)]
                let peer = match stream.peer_cred() {
                    Ok(peer) => Some((peer.uid(), peer.gid())),
                    Err(error) => {
                        tracing::warn!("读取本地通道对端身份失败：{error}");
                        continue;
                    }
                };
                connections.spawn(async move {
                    let result = serve_connection(stream, service_fn(move |request| {
                        dispatch(request, #[cfg(unix)] peer)
                    })).await;
                    if let Err(error) = result {
                        tracing::warn!("本地通道连接失败：{error:#}");
                    }
                });
            }
        }
    };
    // Finish pending replies within the shutdown budget, then await cancellation so
    // every pipe instance is released before the supervisor binds a replacement.
    let _ = tokio::time::timeout(Duration::from_secs(3), async {
        while connections.join_next().await.is_some() {}
    })
    .await;
    connections.shutdown().await;
    result
}

async fn serve_connection<I, S>(stream: I, service: S) -> Result<()>
where
    I: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    S: hyper::service::Service<Request<Incoming>, Response = HttpResponse, Error = Infallible>,
    S::Future: Send,
{
    let mut builder = hyper::server::conn::http1::Builder::new();
    builder
        .keep_alive(false)
        .timer(TokioTimer::new())
        .header_read_timeout(READ_TIMEOUT)
        .max_buf_size(8192);
    tokio::time::timeout(
        CONNECTION_TIMEOUT,
        builder.serve_connection(TokioIo::new(stream), service),
    )
    .await??;
    Ok(())
}

async fn dispatch(
    request: Request<Incoming>,
    #[cfg(unix)] peer: Option<(u32, u32)>,
) -> Result<HttpResponse, Infallible> {
    let (parts, body) = request.into_parts();
    let method = parts.method.clone();
    let path = parts.uri.path().to_owned();
    let started = std::time::Instant::now();
    let header_bytes = parts
        .headers
        .iter()
        .map(|(key, value)| key.as_str().len() + value.len() + 4)
        .sum::<usize>();
    if header_bytes > MAX_HEADER_BYTES {
        return Ok(text_response(
            StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE,
            "Request headers too large",
        ));
    }
    if body.size_hint().lower() > MAX_BODY as u64 {
        return Ok(text_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            "Request body too large",
        ));
    }
    let body =
        match tokio::time::timeout(READ_TIMEOUT, Limited::new(body, MAX_BODY).collect()).await {
            Ok(Ok(body)) => body.to_bytes(),
            Ok(Err(error)) => {
                tracing::warn!("读取本地请求失败：{method} {path}；{error}");
                return Ok(text_response(
                    if error.is::<http_body_util::LengthLimitError>() {
                        StatusCode::PAYLOAD_TOO_LARGE
                    } else {
                        StatusCode::BAD_REQUEST
                    },
                    "Invalid request body",
                ));
            }
            Err(_) => {
                tracing::warn!("读取本地请求超时：{method} {path}");
                return Ok(text_response(
                    StatusCode::REQUEST_TIMEOUT,
                    "Request body timed out",
                ));
            }
        };
    let context = RequestContext {
        method: parts.method,
        uri: parts.uri,
        headers: parts.headers,
        body,
        #[cfg(unix)]
        peer,
    };
    let response =
        match tokio::time::timeout(HANDLER_TIMEOUT, super::server::handle_request(context)).await {
            Ok(Ok(response)) => response,
            Ok(Err(error)) => {
                tracing::error!("处理本地请求失败：{method} {path}；{error:#}");
                text_response(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
            }
            Err(_) => {
                tracing::error!("处理本地请求超时：{method} {path}");
                text_response(StatusCode::REQUEST_TIMEOUT, "Handler timed out")
            }
        };
    tracing::trace!(
        elapsed_ms = started.elapsed().as_millis(),
        status = response.status().as_u16(),
        "{method} {path}"
    );
    Ok(response)
}

pub(super) fn text_response(status: StatusCode, message: &'static str) -> HttpResponse {
    let mut response = Response::new(Full::new(Bytes::from_static(message.as_bytes())));
    *response.status_mut() = status;
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    async fn exchange(request: Vec<u8>) -> Vec<u8> {
        let (mut client, server) = tokio::io::duplex(64 * 1024);
        let task = tokio::spawn(serve_connection(
            server,
            service_fn(|request| {
                dispatch(
                    request,
                    #[cfg(unix)]
                    Some((0, 0)),
                )
            }),
        ));
        let writer = tokio::spawn(async move {
            client.write_all(&request).await.unwrap();
            let mut response = Vec::new();
            client.read_to_end(&mut response).await.unwrap();
            response
        });
        let response = tokio::time::timeout(Duration::from_secs(5), writer)
            .await
            .unwrap()
            .unwrap();
        task.await.unwrap().unwrap();
        response
    }

    fn request(method: &str, path: &str, headers: &str, body: &[u8]) -> Vec<u8> {
        let mut request = format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n{headers}\r\n",
            body.len()
        )
        .into_bytes();
        request.extend_from_slice(body);
        request
    }

    #[tokio::test]
    async fn http_boundary_preserves_auth_routing_protocol_and_json_errors() {
        let auth = format!("X-IPC-Magic: {}\r\n", crate::IPC_AUTH_EXPECT);
        let protocol = format!(
            "{auth}{}: {}.{}\r\n",
            crate::SERVICE_PROTOCOL_HEADER,
            crate::PROTOCOL_EPOCH,
            crate::PROTOCOL_REVISION
        );
        for (method, path, headers, body, expected) in [
            ("GET", "/magic", auth.as_str(), b"".as_slice(), 200),
            ("GET", "/version", auth.as_str(), b"".as_slice(), 200),
            ("GET", "/magic", "", b"".as_slice(), 401),
            ("POST", "/magic", auth.as_str(), b"".as_slice(), 404),
            ("GET", "/missing", auth.as_str(), b"".as_slice(), 404),
            ("GET", "/service/log-snapshot", "", b"".as_slice(), 422),
            (
                "GET",
                "/service/log-snapshot",
                auth.as_str(),
                b"{}".as_slice(),
                422,
            ),
            (
                "GET",
                "/service/log-snapshot",
                protocol.as_str(),
                b"{}".as_slice(),
                400,
            ),
            ("POST", "/installation", auth.as_str(), b"{".as_slice(), 422),
            (
                "POST",
                "/installation",
                protocol.as_str(),
                b"{".as_slice(),
                400,
            ),
        ] {
            let response = exchange(request(method, path, headers, body)).await;
            let response = String::from_utf8(response).unwrap();
            assert!(
                response.starts_with(&format!("HTTP/1.1 {expected} ")),
                "{response}"
            );
            if path == "/magic" && expected == 200 {
                assert!(response.ends_with("Tunglies!"));
            }
            if path == "/version" {
                assert!(response.contains("application/json"));
                assert!(response.contains("\"epoch\""));
            }
        }
    }

    #[tokio::test]
    async fn http_boundary_limits_headers_and_body() {
        let headers = format!("X-Large: {}\r\n", "a".repeat(MAX_HEADER_BYTES));
        let response = exchange(request("GET", "/magic", &headers, b"")).await;
        assert!(response.starts_with(b"HTTP/1.1 431 "));
        // A declared oversized body is rejected before waiting for the body bytes.
        let response = exchange(
            format!(
                "POST /installation HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n",
                MAX_BODY + 1
            )
            .into_bytes(),
        )
        .await;
        assert!(response.starts_with(b"HTTP/1.1 413 "));
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn pipe_accepts_other_clients_while_idle_and_releases_listener_on_shutdown() {
        let path = std::path::PathBuf::from(format!(
            r"\\.\pipe\suki-hyper-ipc-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let descriptor = "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;AU)";
        let listener = Listener::bind(&path, descriptor).unwrap();
        let (stop, shutdown) = oneshot::channel();
        let server = tokio::spawn(serve(listener, shutdown));
        let idle = tokio::net::windows::named_pipe::ClientOptions::new()
            .open(&path)
            .unwrap();
        crate::client::ping_at(&path, Duration::from_secs(2))
            .await
            .unwrap();
        stop.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(4), server)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        drop(Listener::bind(&path, descriptor).unwrap());
        drop(idle);
    }
}
