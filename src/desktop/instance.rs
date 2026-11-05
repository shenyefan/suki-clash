//! One per-user instance lock and an authenticated loopback command endpoint.
use crate::{
    network::{Http, Transport},
    storage::{AppPaths, DirectoryKind, atomic_write},
};
use http_body_util::{BodyExt as _, Full, Limited};
use hyper::{
    Request, Response, StatusCode,
    body::{Bytes, Incoming},
    service::service_fn,
};
use hyper_util::rt::TokioIo;
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    time::{Duration, Instant},
};
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    port: u16,
    token: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Activation {
    pub(crate) uri: Option<String>,
}
pub(crate) enum Disposition {
    Primary(Service),
    Secondary,
}
pub(crate) struct Service {
    pub(crate) sender: async_channel::Sender<Activation>,
    pub(crate) events: async_channel::Receiver<Activation>,
    shutdown: tokio::sync::watch::Sender<bool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Service {
    pub(crate) fn acquire(paths: &AppPaths, uri: Option<String>) -> Result<Disposition, String> {
        let directory = paths.directory(DirectoryKind::Cache);
        std::fs::create_dir_all(&directory).map_err(crate::logging::error_message)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = options
            .open(directory.join("instance.lock"))
            .map_err(|error| format!("无法打开实例锁：{error}"))?;
        let record_path = directory.join("instance.json");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(crate::logging::error_message)?;
        let started = Instant::now();
        let http = Http::default();
        loop {
            match lock.try_lock() {
                Ok(()) => break,
                Err(std::fs::TryLockError::WouldBlock) => {}
                Err(error) => return Err(format!("无法取得实例锁：{error}")),
            }
            let record = std::fs::read(&record_path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Record>(&bytes).ok());
            if let Some(record) = record {
                let notified = runtime.block_on(async {
                    http.client(&Transport::Direct, false)?
                        .post(format!("http://127.0.0.1:{}/activate", record.port))
                        .bearer_auth(&record.token)
                        .timeout(Duration::from_millis(500))
                        .body(
                            serde_json::to_vec(&Activation { uri: uri.clone() }).map_err(
                                |error| crate::logging::diagnostic_error("无法编码实例请求", error),
                            )?,
                        )
                        .header(hyper::header::CONTENT_TYPE, "application/json")
                        .send()
                        .await
                        .map(|response| response.status().is_success())
                        .map_err(|error| {
                            crate::logging::diagnostic_error("无法通知首实例".to_string(), error)
                        })
                });
                if notified == Ok(true) {
                    return Ok(Disposition::Secondary);
                }
            }
            if started.elapsed() > Duration::from_secs(20) {
                return Err("已有实例正在运行，但控制服务未就绪；未启动第二个内核".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .map_err(crate::logging::error_message)?;
        listener
            .set_nonblocking(true)
            .map_err(crate::logging::error_message)?;
        let port = listener
            .local_addr()
            .map_err(crate::logging::error_message)?
            .port();
        let mut token = [0u8; 32];
        getrandom::fill(&mut token).map_err(crate::logging::error_message)?;
        let record = Record {
            port,
            token: token.iter().map(|v| format!("{v:02x}")).collect(),
        };
        atomic_write(
            &record_path,
            &serde_json::to_vec(&record).map_err(crate::logging::error_message)?,
        )
        .map_err(crate::logging::error_message)?;
        let (sender, events) = async_channel::bounded(8);
        if uri.is_some() {
            sender
                .try_send(Activation { uri })
                .map_err(|error| crate::logging::diagnostic_error("启动导入队列不可用", error))?;
        }
        let (shutdown, mut cancelled) = tokio::sync::watch::channel(false);
        let commands = sender.clone();
        let worker=std::thread::Builder::new().name("suki-instance".into()).spawn(move|| {
            let _lock:File=lock;
            runtime.block_on(async move {
                let listener=match tokio::net::TcpListener::from_std(listener){Ok(listener)=>listener,Err(_)=>return};
                let mut connections=tokio::task::JoinSet::new();
                loop {
                    tokio::select!{biased;
                        _=cancelled.changed()=>break,
                        Some(_)=connections.join_next(),if !connections.is_empty()=>{},
                        connection=listener.accept(),if connections.len()<16=>{
                            let Ok((stream,_))=connection else{break;};
                            let token=record.token.clone();let sender=commands.clone();
                            connections.spawn(async move {
                                let service=service_fn(move|request|handle(request,token.clone(),sender.clone()));
                                let _=tokio::time::timeout(Duration::from_secs(3),hyper::server::conn::http1::Builder::new().keep_alive(false).max_buf_size(8192).serve_connection(TokioIo::new(stream),service)).await;
                            });
                        }
                    }
                }
                connections.abort_all();while connections.join_next().await.is_some(){}
            });
            let _=std::fs::remove_file(record_path);
        }).map_err(crate::logging::error_message)?;
        Ok(Disposition::Primary(Self {
            sender,
            events,
            shutdown,
            worker: Some(worker),
        }))
    }
    pub(crate) fn shutdown(&mut self) -> Option<std::thread::JoinHandle<()>> {
        self.shutdown.send_replace(true);
        self.worker.take()
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        self.shutdown.send_replace(true);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
async fn handle(
    request: Request<Incoming>,
    token: String,
    sender: async_channel::Sender<Activation>,
) -> Result<Response<Full<Bytes>>, std::convert::Infallible> {
    let status = if request.method() != hyper::Method::POST || request.uri().path() != "/activate" {
        StatusCode::NOT_FOUND
    } else if request
        .headers()
        .get(hyper::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        != Some(format!("Bearer {token}").as_str())
    {
        StatusCode::UNAUTHORIZED
    } else {
        match Limited::new(request.into_body(), 8192).collect().await {
            Ok(body) => match serde_json::from_slice::<Activation>(&body.to_bytes()) {
                Ok(command)
                    if command
                        .uri
                        .as_ref()
                        .is_none_or(|uri| parse_uri(uri).is_ok()) =>
                {
                    if sender.try_send(command).is_ok() {
                        StatusCode::ACCEPTED
                    } else {
                        StatusCode::SERVICE_UNAVAILABLE
                    }
                }
                _ => StatusCode::BAD_REQUEST,
            },
            Err(_) => StatusCode::PAYLOAD_TOO_LARGE,
        }
    };
    let mut response = Response::new(Full::new(Bytes::new()));
    *response.status_mut() = status;
    Ok(response)
}
pub(crate) fn parse_uri(uri: &str) -> Result<crate::profiles::model::SubscriptionImport, String> {
    if uri.len() > 8192 {
        return Err("订阅导入链接过长".into());
    }
    let parsed = url::Url::parse(uri)
        .map_err(|error| crate::logging::diagnostic_error("订阅导入链接无效", error))?;
    if !matches!(parsed.scheme(), "clash" | "suki-clash")
        || parsed.host_str() != Some("install-config")
    {
        return Err("不支持的订阅导入链接".into());
    }
    let values = parsed
        .query_pairs()
        .filter(|(name, _)| name == "url")
        .map(|(_, value)| value.into_owned())
        .collect::<Vec<_>>();
    if values.len() != 1 || !crate::profiles::model::valid_remote_url(&values[0]) {
        return Err("导入链接需包含一个有效 HTTP/HTTPS 订阅地址".into());
    }
    let names = parsed
        .query_pairs()
        .filter(|(key, _)| key == "name")
        .map(|(_, value)| value.into_owned())
        .collect::<Vec<_>>();
    if names.len() > 1
        || names
            .first()
            .is_some_and(|name| name.len() > 256 || name.chars().any(char::is_control))
    {
        return Err("导入链接名称无效或重复".into());
    }
    Ok(crate::profiles::model::SubscriptionImport {
        url: values[0].clone(),
        name: names.into_iter().next().filter(|name| !name.is_empty()),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scheme_validation_rejects_ambiguous_or_non_http_imports() {
        assert_eq!(
            parse_uri("clash://install-config?url=https%3A%2F%2Fexample.test%2Fsub").unwrap(),
            crate::profiles::model::SubscriptionImport {
                url: "https://example.test/sub".into(),
                name: None
            }
        );
        assert_eq!(parse_uri("suki-clash://install-config?url=https://example.test/sub&name=%E8%BF%9C%E7%A8%8B%E8%AE%A2%E9%98%85").unwrap().name.as_deref(), Some("远程订阅"));
        for uri in [
            "clash://install-config?url=https://example.test/sub&name=a&name=b",
            "clash://install-config?url=file:///tmp/config",
            "clash://install-config?url=https://a.test&url=https://b.test",
            "https://example.test/",
        ] {
            assert!(parse_uri(uri).is_err());
        }
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    #[test]
    fn endpoint_authenticates_and_rejects_invalid_or_oversized_imports_before_queueing() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        let Disposition::Primary(primary) = Service::acquire(&paths, None).unwrap() else {
            panic!("primary")
        };
        let record: Record = serde_json::from_slice(
            &std::fs::read(paths.directory(DirectoryKind::Cache).join("instance.json")).unwrap(),
        )
        .unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let http = Http::default();
            let client = http.client(&Transport::Direct, false).unwrap();
            let endpoint = format!("http://127.0.0.1:{}/activate", record.port);
            let send = |token: String, body: String| {
                client
                    .post(&endpoint)
                    .bearer_auth(token)
                    .body(body)
                    .timeout(Duration::from_secs(2))
                    .send()
            };
            assert_eq!(
                send("wrong".into(), "{}".into()).await.unwrap().status(),
                StatusCode::UNAUTHORIZED
            );
            assert_eq!(
                send(
                    record.token.clone(),
                    r#"{"uri":"file:///private/config"}"#.into()
                )
                .await
                .unwrap()
                .status(),
                StatusCode::BAD_REQUEST
            );
            assert_eq!(
                send(record.token.clone(), "x".repeat(8193))
                    .await
                    .unwrap()
                    .status(),
                StatusCode::PAYLOAD_TOO_LARGE
            );
            assert!(primary.events.try_recv().is_err());
            assert_eq!(
                send(record.token.clone(), r#"{"uri":null}"#.into())
                    .await
                    .unwrap()
                    .status(),
                StatusCode::ACCEPTED
            );
        });
        assert!(primary.events.recv_blocking().unwrap().uri.is_none());
    }
    #[test]
    fn second_instance_notifies_primary_and_cannot_claim_the_lock() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        let Disposition::Primary(primary) = Service::acquire(&paths, None).unwrap() else {
            panic!("expected primary")
        };
        assert!(matches!(
            Service::acquire(
                &paths,
                Some("clash://install-config?url=https://example.test/sub".into())
            )
            .unwrap(),
            Disposition::Secondary
        ));
        assert!(
            primary
                .events
                .recv_blocking()
                .unwrap()
                .uri
                .unwrap()
                .contains("example.test")
        );
        drop(primary);
        assert!(matches!(
            Service::acquire(&paths, None).unwrap(),
            Disposition::Primary(_)
        ));
    }
}
