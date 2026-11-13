use std::{path::Path, time::Duration};

use anyhow::Result;
use hyper::{Method, Request, StatusCode, body::Bytes};
use once_cell::sync::Lazy;
use tokio::sync::RwLock;

mod transport;

#[cfg(all(windows, any(not(feature = "test"), test)))]
mod windows_identity;

use crate::{
    AuthenticatedRequest, AuthenticatedSessionRequest, IPC_AUTH_EXPECT, IPC_PATH, IpcCommand,
    OwnerCredentials, OwnerSessionProof, ProtocolInfo, ProtocolVersion, RuntimeBundle,
    RuntimeFileOutcome, RuntimeFileRequest, ServiceStatusSnapshot, StageRuntimeOutcome,
    StartClashRequest, StartClashResult, WriterConfig,
    core::structure::{JsonConvert, Response},
};

/// Status observation may recover after transport, handler or service availability failures.
#[derive(Debug)]
pub enum RequestError {
    Unreachable(String),
    Rejected {
        code: u16,
        message: String,
    },
    InvalidResponse(String),
    InvalidRequest(String),
    AccessDenied(String),
    ProtocolMismatch {
        expected: ProtocolVersion,
        actual: ProtocolInfo,
    },
}
impl std::fmt::Display for RequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable(message) if message == "响应超时" => {
                write!(f, "后台服务响应超时")
            }
            Self::Unreachable(message) => write!(f, "后台服务暂不可用：{message}"),
            Self::Rejected { code, message } => write!(f, "后台服务拒绝操作（{code}）：{message}"),
            Self::InvalidResponse(message) => write!(f, "后台服务响应无效：{message}"),
            Self::InvalidRequest(message) => write!(f, "后台服务请求无效：{message}"),
            Self::AccessDenied(message) => write!(f, "后台服务连接被拒绝：{message}"),
            Self::ProtocolMismatch { expected, actual } => write!(
                f,
                "后台服务协议不一致（需要 {}，当前 {}，服务要求客户端修订 {}），请修复服务",
                expected.header_value(),
                actual.protocol.header_value(),
                actual.min_client_revision
            ),
        }
    }
}
impl std::error::Error for RequestError {}
impl From<std::io::Error> for RequestError {
    fn from(error: std::io::Error) -> Self {
        match error.kind() {
            std::io::ErrorKind::PermissionDenied => Self::AccessDenied(error.to_string()),
            std::io::ErrorKind::InvalidInput => Self::InvalidRequest(error.to_string()),
            std::io::ErrorKind::InvalidData => Self::InvalidResponse(error.to_string()),
            _ => Self::Unreachable(error.to_string()),
        }
    }
}
pub fn is_unreachable(error: &anyhow::Error) -> bool {
    matches!(
        error.downcast_ref::<RequestError>(),
        Some(RequestError::Unreachable(_))
    )
}

/// Only read observations may retry these HTTP failures; they do not prove
/// the service/core stopped and must not authorize write replay or takeover.
pub fn is_read_unavailable(error: &anyhow::Error) -> bool {
    matches!(
        error.downcast_ref::<RequestError>(),
        Some(
            RequestError::Unreachable(_)
                // ipc::dispatch emits 408/500; server::service_unavailable emits 503.
                | RequestError::Rejected { code: 408 | 500 | 503, .. }
        )
    )
}

#[cfg(windows)]
pub async fn service_transport_expected() -> Result<bool, String> {
    tokio::task::spawn_blocking(|| {
        use windows_service::service::{ServiceAccess, ServiceState};
        use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

        let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
            .map_err(|error| error.to_string())?;
        let service =
            match manager.open_service(crate::WINDOWS_SERVICE_NAME, ServiceAccess::QUERY_STATUS) {
                Ok(service) => service,
                Err(windows_service::Error::Winapi(error))
                    if matches!(error.raw_os_error(), Some(1060 | 1072)) =>
                {
                    return Ok(false);
                }
                Err(error) => return Err(error.to_string()),
            };
        let status = service.query_status().map_err(|error| error.to_string())?;
        Ok(!matches!(
            status.current_state,
            ServiceState::Stopped | ServiceState::StopPending
        ))
    })
    .await
    .map_err(|error| error.to_string())?
}

pub fn response_data<T>(reply: Response<T>) -> Result<T> {
    if reply.code != 0 {
        return Err(RequestError::Rejected {
            code: reply.code,
            message: reply.message,
        }
        .into());
    }
    reply
        .data
        .ok_or_else(|| RequestError::InvalidResponse("服务未返回结果".into()).into())
}

fn ensure_success(status: StatusCode, body: &[u8]) -> Result<()> {
    if !status.is_success() {
        let message = serde_json::from_slice::<Response<serde_json::Value>>(body)
            .ok()
            .map(|reply| reply.message)
            .filter(|message| !message.is_empty())
            .unwrap_or_else(|| "HTTP 请求被拒绝".into());
        return Err(RequestError::Rejected {
            code: status.as_u16(),
            message,
        }
        .into());
    }
    Ok(())
}
fn decode<R: for<'de> serde::Deserialize<'de>>(
    status: StatusCode,
    body: &[u8],
) -> Result<Response<R>> {
    ensure_success(status, body)?;
    serde_json::from_slice(body)
        .map_err(|error| RequestError::InvalidResponse(error.to_string()).into())
}

static REQUEST_TIMEOUT: Lazy<RwLock<Option<Duration>>> = Lazy::new(|| RwLock::new(None));
const IPC_AUTH_HEADER_KEY: &str = "X-IPC-Magic";
const LIFECYCLE_TIMEOUT: Duration = Duration::from_secs(30);
// Large cache chunks need a longer timeout than control messages.
const RUNTIME_FILE_TIMEOUT: Duration = Duration::from_secs(15);

fn protected(request: hyper::http::request::Builder) -> hyper::http::request::Builder {
    request.header(
        crate::SERVICE_PROTOCOL_HEADER,
        ProtocolVersion::current().header_value(),
    )
}

fn request(verb: Verb, command: IpcCommand) -> hyper::http::request::Builder {
    Request::builder()
        .method(match verb {
            Verb::Get => Method::GET,
            Verb::Post => Method::POST,
            Verb::Put => Method::PUT,
            Verb::Delete => Method::DELETE,
        })
        .uri(command.as_ref())
        .header("Host", "localhost")
        .header("Content-Type", "application/json")
        .header("Connection", "close")
}

async fn send<R: for<'de> serde::Deserialize<'de>>(
    request: hyper::http::request::Builder,
    body: Vec<u8>,
    timeout: Option<Duration>,
) -> Result<Response<R>> {
    let request = request
        .body(transport::Body::new(Bytes::from(body)))
        .map_err(|error| RequestError::InvalidRequest(error.to_string()))?;
    let timeout = match timeout {
        Some(timeout) => timeout,
        None => request_timeout().await,
    };
    let (status, body) = transport::send(Path::new(IPC_PATH), request, timeout).await?;
    decode(status, &body)
}

async fn request_timeout() -> Duration {
    REQUEST_TIMEOUT
        .read()
        .await
        .unwrap_or(Duration::from_secs(2))
}

pub async fn set_request_timeout(timeout: Option<Duration>) {
    *REQUEST_TIMEOUT.write().await = timeout;
}

#[cfg(feature = "standalone")]
pub(crate) async fn ping_at(path: &Path, timeout: Duration) -> Result<()> {
    let request = request(Verb::Get, IpcCommand::Magic)
        .header(IPC_AUTH_HEADER_KEY, IPC_AUTH_EXPECT)
        .body(transport::Body::new(Bytes::new()))
        .map_err(|error| RequestError::InvalidRequest(error.to_string()))?;
    let (status, body) = transport::send(path, request, timeout).await?;
    ensure_success(status, &body)
}

#[derive(Clone, Copy)]
enum Verb {
    Get,
    Post,
    Put,
    Delete,
}

/// Sends a versioned, authenticated request using the route's required envelope.
/// `session` selects owner-only or active-session authentication.
async fn protected_call<P, R>(
    verb: Verb,
    command: IpcCommand,
    credentials: &OwnerCredentials,
    session: Option<&OwnerSessionProof>,
    payload: P,
    timeout: Option<Duration>,
) -> Result<Response<R>>
where
    P: serde::Serialize + for<'de> serde::Deserialize<'de>,
    R: for<'de> serde::Deserialize<'de>,
{
    let body = match session {
        None => AuthenticatedRequest {
            credentials: credentials.clone(),
            payload,
        }
        .to_json_value()?,
        Some(session) => AuthenticatedSessionRequest {
            credentials: credentials.clone(),
            session: session.clone(),
            payload,
        }
        .to_json_value()?,
    };
    send(
        protected(request(verb, command)),
        serde_json::to_vec(&body)
            .map_err(|error| RequestError::InvalidRequest(error.to_string()))?,
        timeout,
    )
    .await
}

pub async fn get_version() -> Result<Response<ProtocolInfo>> {
    send(
        request(Verb::Get, IpcCommand::GetVersion).header(IPC_AUTH_HEADER_KEY, IPC_AUTH_EXPECT),
        Vec::new(),
        None,
    )
    .await
}

pub fn validate_protocol_response(reply: Response<ProtocolInfo>) -> Result<ProtocolInfo> {
    let actual = response_data(reply)?;
    let expected = ProtocolVersion::current();
    if !actual.supports_client(expected, crate::MIN_REQUIRED_SERVICE_REVISION) {
        return Err(RequestError::ProtocolMismatch { expected, actual }.into());
    }
    Ok(actual)
}

pub async fn get_status(credentials: &OwnerCredentials) -> Result<Response<ServiceStatusSnapshot>> {
    protected_call(Verb::Get, IpcCommand::Status, credentials, None, (), None).await
}

pub async fn start_clash(
    credentials: &OwnerCredentials,
    body: &StartClashRequest,
) -> Result<Response<StartClashResult>> {
    protected_call(
        Verb::Post,
        IpcCommand::StartClash,
        credentials,
        None,
        body.clone(),
        Some(LIFECYCLE_TIMEOUT),
    )
    .await
}

pub async fn get_clash_logs(
    credentials: &OwnerCredentials,
    session: &OwnerSessionProof,
    request: crate::CoreLogRequest,
) -> Result<Response<crate::CoreLogBatch>> {
    protected_call(
        Verb::Get,
        IpcCommand::GetClashLogs,
        credentials,
        Some(session),
        request,
        Some(LIFECYCLE_TIMEOUT),
    )
    .await
}

pub async fn get_clash_log_snapshot(credentials: &OwnerCredentials) -> Result<Response<String>> {
    protected_call(
        Verb::Get,
        IpcCommand::GetClashLogSnapshot,
        credentials,
        None,
        (),
        None,
    )
    .await
}

/// Bounded service log tail, available to authenticated clients even with the core stopped.
pub async fn get_service_log_snapshot(credentials: &OwnerCredentials) -> Result<Response<String>> {
    protected_call(
        Verb::Get,
        IpcCommand::GetServiceLogSnapshot,
        credentials,
        None,
        (),
        None,
    )
    .await
}

pub async fn stop_clash(
    credentials: &OwnerCredentials,
    session: &OwnerSessionProof,
) -> Result<Response<()>> {
    protected_call(
        Verb::Delete,
        IpcCommand::StopClash,
        credentials,
        Some(session),
        (),
        Some(LIFECYCLE_TIMEOUT),
    )
    .await
}

/// Stages `body` into the running core's generation without restarting it.
/// Call only when [`ProtocolInfo::supports_runtime_staging`] is true; `RestartRequired` is a
/// successful response that asks the caller to fall back to stop and start.
pub async fn stage_runtime(
    credentials: &OwnerCredentials,
    session: &OwnerSessionProof,
    body: &RuntimeBundle,
) -> Result<Response<StageRuntimeOutcome>> {
    protected_call(
        Verb::Put,
        IpcCommand::StageRuntime,
        credentials,
        Some(session),
        body.clone(),
        Some(LIFECYCLE_TIMEOUT),
    )
    .await
}

/// Requires [`ProtocolInfo::supports_runtime_file_read`]; advance `offset` until `len` is reached.
pub async fn read_runtime_file(
    credentials: &OwnerCredentials,
    session: &OwnerSessionProof,
    body: &RuntimeFileRequest,
) -> Result<Response<RuntimeFileOutcome>> {
    protected_call(
        Verb::Get,
        IpcCommand::ReadRuntimeFile,
        credentials,
        Some(session),
        body.clone(),
        Some(RUNTIME_FILE_TIMEOUT),
    )
    .await
}

pub async fn update_writer(
    credentials: &OwnerCredentials,
    session: &OwnerSessionProof,
    body: &WriterConfig,
) -> Result<Response<()>> {
    protected_call(
        Verb::Put,
        IpcCommand::UpdateWriter,
        credentials,
        Some(session),
        body.clone(),
        None,
    )
    .await
}

pub async fn set_system_proxy(
    credentials: &OwnerCredentials,
    session: &OwnerSessionProof,
    body: &crate::proxy_owner::Operation,
) -> Result<Response<crate::proxy_owner::Observation>> {
    protected_call(
        Verb::Put,
        IpcCommand::SetSystemProxy,
        credentials,
        Some(session),
        body.clone(),
        None,
    )
    .await
}

/// Inspects approved cores and global occupancy without exposing another owner's session.
pub async fn inspect_installation(
    requirements: &[crate::CoreRequirement],
) -> Result<crate::InstallationStatus> {
    inspect_installation_with_digest(requirements, false).await
}

pub(crate) async fn inspect_installation_with_digest(
    requirements: &[crate::CoreRequirement],
    include_service_digest: bool,
) -> Result<crate::InstallationStatus> {
    let response = send(
        protected(request(Verb::Post, IpcCommand::InspectInstallation))
            .header(IPC_AUTH_HEADER_KEY, IPC_AUTH_EXPECT)
            .header(
                "X-Service-Digest",
                if include_service_digest {
                    "true"
                } else {
                    "false"
                },
            ),
        serde_json::to_vec(requirements)
            .map_err(|error| RequestError::InvalidRequest(error.to_string()))?,
        Some(LIFECYCLE_TIMEOUT),
    )
    .await?;
    response_data(response)
}

pub async fn windows_network(
    credentials: &OwnerCredentials,
    request: crate::windows_network::Request,
) -> Result<Response<Vec<crate::windows_network::UwpApp>>> {
    protected_call(
        Verb::Post,
        IpcCommand::WindowsNetwork,
        credentials,
        None,
        request,
        Some(LIFECYCLE_TIMEOUT),
    )
    .await
}

#[cfg(test)]
mod failure_tests {
    use super::*;
    #[test]
    fn actual_service_observation_failures_are_recoverable_but_request_rejections_are_final() {
        for code in [400, 401, 403, 404, 408, 409, 413, 422, 431, 500, 503] {
            let error = decode::<()>(StatusCode::from_u16(code).unwrap(), b"").unwrap_err();
            assert!(!is_unreachable(&error));
            assert_eq!(is_read_unavailable(&error), matches!(code, 408 | 500 | 503));
        }
    }
    #[test]
    fn service_rejections_missing_results_and_transport_failures_stay_distinct() {
        let rejected = response_data::<()>(Response {
            code: 403,
            message: "owner denied".into(),
            data: None,
        })
        .unwrap_err();
        assert!(matches!(
            rejected.downcast_ref::<RequestError>(),
            Some(RequestError::Rejected { code: 403, .. })
        ));
        assert!(rejected.to_string().contains("owner denied"));
        assert!(!is_unreachable(&rejected));
        let absent = response_data::<()>(Response {
            code: 0,
            message: String::new(),
            data: None,
        })
        .unwrap_err();
        assert!(!is_unreachable(&absent));
        let unavailable = anyhow::Error::new(RequestError::from(std::io::Error::from(
            std::io::ErrorKind::NotFound,
        )));
        assert!(is_unreachable(&unavailable));
        let denied = anyhow::Error::new(RequestError::from(std::io::Error::from(
            std::io::ErrorKind::PermissionDenied,
        )));
        assert!(!is_unreachable(&denied));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn readable_headers_remain_terminal_when_the_body_is_interrupted_or_times_out() {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
        for status in [200, 403] {
            for stall in [false, true] {
                let root = tempfile::tempdir().unwrap();
                let path = root.path().join("partial.sock");
                let listener = tokio::net::UnixListener::bind(&path).unwrap();
                let server = tokio::spawn(async move {
                    let (mut stream, _) = listener.accept().await.unwrap();
                    let mut bytes = [0; 2048];
                    assert!(stream.read(&mut bytes).await.unwrap() > 0);
                    stream.write_all(format!("HTTP/1.1 {status} test\r\nContent-Length: 100\r\nConnection: close\r\n\r\n{{").as_bytes()).await.unwrap();
                    if stall {
                        assert_eq!(stream.read(&mut bytes).await.unwrap(), 0);
                    }
                });
                let request = Request::builder()
                    .uri("/version")
                    .header("Host", "localhost")
                    .body(transport::Body::new(Bytes::new()))
                    .unwrap();
                let error = transport::send(&path, request, Duration::from_millis(100))
                    .await
                    .unwrap_err();
                match status {
                    403 => assert!(matches!(error, RequestError::Rejected { code: 403, .. })),
                    _ => assert!(matches!(error, RequestError::InvalidResponse(_))),
                }
                server.await.unwrap();
            }
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_lost_write_acknowledgement_is_unreachable_and_never_replayed() {
        use tokio::io::AsyncReadExt as _;
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("lost-ack.sock");
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = [0; 2048];
            let read = stream.read(&mut bytes).await.unwrap();
            assert!(bytes[..read].starts_with(b"PUT /stop "));
            drop(stream);
            assert!(
                tokio::time::timeout(Duration::from_millis(250), listener.accept())
                    .await
                    .is_err()
            );
        });
        let request = Request::builder()
            .method(Method::PUT)
            .uri("/stop")
            .header("Host", "localhost")
            .body(transport::Body::new(Bytes::from_static(b"{}")))
            .unwrap();
        let error: anyhow::Error = transport::send(&path, request, Duration::from_secs(1))
            .await
            .unwrap_err()
            .into();
        assert!(is_unreachable(&error));
        server.await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn http_failures_keep_their_classification_without_exposing_body_secrets() {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
        for (status, body) in [
            (403, r#"{"code":403,"message":"owner denied","data":null}"#),
            (
                500,
                r#"{"code":500,"message":"execution rejected","data":null}"#,
            ),
            (200, "secret-body"),
        ] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("response.sock");
            let listener = tokio::net::UnixListener::bind(&path).unwrap();
            let server = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut bytes = [0; 2048];
                assert!(stream.read(&mut bytes).await.unwrap() > 0);
                stream.write_all(format!("HTTP/1.1 {status} test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            });
            let (status, reply) = transport::send(
                &path,
                Request::builder()
                    .method(Method::GET)
                    .uri("/version")
                    .header("Host", "localhost")
                    .body(transport::Body::new(Bytes::new()))
                    .unwrap(),
                Duration::from_secs(1),
            )
            .await
            .unwrap();
            let error = decode::<ProtocolInfo>(status, &reply).unwrap_err();
            assert!(!is_unreachable(&error));
            assert_eq!(is_read_unavailable(&error), status == 500);
            assert!(!error.to_string().contains("secret-body"));
            if status == 403 {
                assert!(error.to_string().contains("owner denied"));
            }
            server.await.unwrap();
        }
    }
}
