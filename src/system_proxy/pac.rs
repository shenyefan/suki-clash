//! Loopback-only PAC endpoint; Hyper owns HTTP parsing and response framing.
use http_body_util::Full;
use hyper::{
    Request, Response, StatusCode,
    body::{Bytes, Incoming},
    service::service_fn,
};
use hyper_util::rt::TokioIo;
use std::{convert::Infallible, time::Duration};
pub(super) struct Server {
    url: String,
    script: tokio::sync::watch::Sender<String>,
    task: tokio::task::JoinHandle<()>,
}
impl Server {
    pub(super) async fn start() -> Result<Self, String> {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|error| format!("无法启动 PAC 服务：{error}"))?;
        let port = listener
            .local_addr()
            .map_err(crate::logging::error_message)?
            .port();
        let url = format!("http://127.0.0.1:{port}/proxy.pac");
        let (script, contents) = tokio::sync::watch::channel(String::new());
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let contents = contents.clone();
                let service = service_fn(move |request: Request<Incoming>| {
                    let allowed = request.method() == hyper::Method::GET
                        && request.uri().path() == "/proxy.pac";
                    let script = if allowed {
                        contents.borrow().clone()
                    } else {
                        String::new()
                    };
                    async move {
                        let mut response = Response::new(Full::new(Bytes::from(script)));
                        if !allowed {
                            *response.status_mut() = StatusCode::NOT_FOUND;
                        }
                        response.headers_mut().insert(
                            hyper::header::CONTENT_TYPE,
                            hyper::header::HeaderValue::from_static(
                                "application/x-ns-proxy-autoconfig",
                            ),
                        );
                        response.headers_mut().insert(
                            hyper::header::CACHE_CONTROL,
                            hyper::header::HeaderValue::from_static("no-store"),
                        );
                        Ok::<_, Infallible>(response)
                    }
                });
                let mut connection = hyper::server::conn::http1::Builder::new();
                connection.keep_alive(false).max_buf_size(8192);
                let _ = tokio::time::timeout(
                    Duration::from_secs(2),
                    connection.serve_connection(TokioIo::new(stream), service),
                )
                .await;
            }
        });
        Ok(Self { url, script, task })
    }
    pub(super) fn url(&self) -> String {
        self.url.clone()
    }
    pub(super) fn contents(&self) -> String {
        self.script.borrow().clone()
    }
    pub(super) fn set(&self, script: String) {
        self.script.send_replace(script);
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
