//! One request per connection. Readiness owners decide whether an unavailable service is retried.
use super::RequestError;
use http_body_util::{BodyExt as _, Full};
use hyper::{Request, StatusCode, body::Bytes, client::conn::http1};
use hyper_util::rt::TokioIo;
use std::{path::Path, time::Duration};

pub(super) type Body = Full<Bytes>;

pub(super) async fn send(
    path: &Path,
    request: Request<Body>,
    timeout: Duration,
) -> Result<(StatusCode, Vec<u8>), RequestError> {
    let mut received = None;
    let work = async {
        log::trace!("连接后台服务 IPC：{}", path.display());
        #[cfg(unix)]
        let stream = tokio::net::UnixStream::connect(path)
            .await
            .map_err(RequestError::from)?;
        #[cfg(windows)]
        let stream = {
            // The local pipe transport can briefly be busy during the accept/create-next
            // listener gap. Only retry opening: the request has not been sent.
            let stream = loop {
                match tokio::net::windows::named_pipe::ClientOptions::new().open(path) {
                    Ok(stream) => break stream,
                    Err(error) if error.raw_os_error() == Some(231) => {
                        tokio::time::sleep(Duration::from_millis(5)).await;
                    }
                    Err(error) => return Err(RequestError::from(error)),
                }
            };
            #[cfg(not(feature = "test"))]
            super::windows_identity::verify_pipe(&stream).await?;
            stream
        };
        let (mut sender, connection) = http1::handshake(TokioIo::new(stream))
            .await
            .map_err(transport_error)?;
        let response = async {
            let response = sender
                .send_request(request)
                .await
                .map_err(transport_error)?;
            let status = response.status();
            received = Some(status);
            let mut body = response.into_body();
            let mut bytes = Vec::new();
            while let Some(frame) = body.frame().await {
                let frame =
                    frame.map_err(|error| RequestError::InvalidResponse(error.to_string()))?;
                if let Ok(data) = frame.into_data() {
                    // Service log windows and runtime chunks share the application's IPC limit.
                    if bytes.len().saturating_add(data.len()) > 8 * 1024 * 1024 {
                        return Err(RequestError::InvalidResponse("响应超过 8 MiB".into()));
                    }
                    bytes.extend_from_slice(&data);
                }
            }
            Ok((status, bytes))
        };
        // Cancellation drops both halves; no detached connection or automatic request replay.
        tokio::pin!(response, connection);
        tokio::select! { biased;
            result = &mut response => result,
            result = &mut connection => {
                result.map_err(transport_error)?;
                response.await
            }
        }
    };
    let result = tokio::time::timeout(timeout, work)
        .await
        .unwrap_or_else(|_| Err(RequestError::Unreachable("响应超时".into())));
    match (result, received) {
        (Err(_), Some(status)) if !status.is_success() => Err(RequestError::Rejected {
            code: status.as_u16(),
            message: "HTTP 请求被拒绝，响应未完整返回".into(),
        }),
        (Err(RequestError::Unreachable(message)), Some(_)) => {
            Err(RequestError::InvalidResponse(message))
        }
        (result, _) => result,
    }
}

fn transport_error(error: hyper::Error) -> RequestError {
    if error.is_parse() || error.is_user() {
        RequestError::InvalidResponse(error.to_string())
    } else {
        RequestError::Unreachable(error.to_string())
    }
}

#[cfg(all(test, windows, feature = "test"))]
mod windows_tests {
    use super::*;
    use tokio::{
        io::{AsyncReadExt as _, AsyncWriteExt as _},
        net::windows::named_pipe::{ClientOptions, ServerOptions},
    };

    fn request() -> Request<Body> {
        Request::builder()
            .method("PUT")
            .uri("/stop")
            .header("Host", "localhost")
            .header("Connection", "close")
            .body(Body::new(Bytes::from_static(b"{}")))
            .unwrap()
    }

    #[tokio::test]
    async fn busy_pipe_waits_for_the_next_listener_before_sending_once() {
        let pipe = format!(r"\\.\pipe\suki-busy-open-test-{}", std::process::id());
        let first = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&pipe)
            .unwrap();
        let occupied = ClientOptions::new().open(&pipe).unwrap();
        first.connect().await.unwrap();
        assert_eq!(
            ClientOptions::new().open(&pipe).unwrap_err().raw_os_error(),
            Some(231)
        );
        let pending = tokio::spawn({
            let pipe = pipe.clone();
            async move { send(Path::new(&pipe), request(), Duration::from_secs(1)).await }
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            !pending.is_finished(),
            "a busy listener must not fail the operation immediately"
        );
        let mut next = ServerOptions::new().create(&pipe).unwrap();
        tokio::time::timeout(Duration::from_secs(1), next.connect())
            .await
            .unwrap()
            .unwrap();
        let mut bytes = [0; 2048];
        let count = next.read(&mut bytes).await.unwrap();
        assert!(bytes[..count].starts_with(b"PUT /stop "));
        next.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
            .await
            .unwrap();
        let (status, body) = pending.await.unwrap().unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, b"{}");
        let unused = ServerOptions::new().create(&pipe).unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(100), unused.connect())
                .await
                .is_err()
        );
        drop((occupied, first));
    }

    #[tokio::test]
    async fn permanently_busy_pipe_uses_the_request_deadline() {
        let pipe = format!(r"\\.\pipe\suki-busy-deadline-test-{}", std::process::id());
        let first = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&pipe)
            .unwrap();
        let occupied = ClientOptions::new().open(&pipe).unwrap();
        first.connect().await.unwrap();
        let started = tokio::time::Instant::now();
        let error = send(Path::new(&pipe), request(), Duration::from_millis(80))
            .await
            .unwrap_err();
        assert!(matches!(error, RequestError::Unreachable(ref message) if message == "响应超时"));
        assert!(started.elapsed() >= Duration::from_millis(70));
        assert!(started.elapsed() < Duration::from_secs(1));
        drop((occupied, first));
    }

    #[tokio::test]
    async fn a_lost_write_acknowledgement_never_reopens_the_pipe() {
        let pipe = format!(r"\\.\pipe\suki-lost-ack-test-{}", std::process::id());
        let mut first = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&pipe)
            .unwrap();
        let pending = tokio::spawn({
            let pipe = pipe.clone();
            async move { send(Path::new(&pipe), request(), Duration::from_secs(1)).await }
        });
        first.connect().await.unwrap();
        let unused = ServerOptions::new().create(&pipe).unwrap();
        let mut bytes = [0; 2048];
        let count = first.read(&mut bytes).await.unwrap();
        assert!(bytes[..count].starts_with(b"PUT /stop "));
        drop(first);
        assert!(matches!(
            pending.await.unwrap(),
            Err(RequestError::Unreachable(_))
        ));
        assert!(
            tokio::time::timeout(Duration::from_millis(100), unused.connect())
                .await
                .is_err()
        );
    }
}
