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
            let stream = tokio::net::windows::named_pipe::ClientOptions::new()
                .open(path)
                .map_err(RequestError::from)?;
            #[cfg(not(feature = "test"))]
            super::windows_identity::verify_pipe(&stream)?;
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
        .unwrap_or_else(|_| Err(RequestError::Unreachable("等待服务响应超时".into())));
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
