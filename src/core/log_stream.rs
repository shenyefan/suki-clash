//! One supervised, generation-scoped logs connection, independent of the UI.
use super::{
    monitor::Target,
    output::{LogSettings, Output},
};
use crate::{
    logging::{Feed, Level},
    storage::AppPaths,
};
use futures_util::{SinkExt as _, StreamExt as _};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::watch;
#[derive(Clone)]
pub(super) struct Source {
    pub(super) service: Option<(
        suki_service::OwnerCredentials,
        suki_service::OwnerSessionProof,
    )>,
    pub(super) settings: watch::Receiver<LogSettings>,
    pub(super) pipe_suppressed: Arc<AtomicBool>,
}
struct PipeGuard(Arc<AtomicBool>);
impl Drop for PipeGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
#[derive(serde::Deserialize)]
struct Log {
    #[serde(rename = "type")]
    level: String,
    payload: String,
}
fn line(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() > 64 * 1024 {
        return Err("内核日志消息超过 64 KiB".into());
    }
    let log: Log = serde_json::from_slice(bytes)
        .map_err(|error| crate::logging::diagnostic_error("内核日志响应无效", error))?;
    let payload = serde_json::to_string(&log.payload)
        .map_err(|error| crate::logging::diagnostic_error("无法编码内核日志", error))?;
    Ok(format!(
        "level={} msg={payload}",
        Level::parse(&log.level).label().to_ascii_lowercase()
    ))
}
#[derive(Default)]
struct ServiceCursor {
    scope: Option<(u64, u64)>,
    after: u64,
    stream_started_at: Option<std::time::SystemTime>,
}
impl ServiceCursor {
    fn bind(&mut self, generation: u64, service_generation: u64) {
        if self.scope != Some((generation, service_generation)) {
            self.scope = Some((generation, service_generation));
            self.after = 0;
            self.stream_started_at = None;
        }
    }
    fn publish(
        &mut self,
        batch: suki_service::CoreLogBatch,
        output: &mut Output,
        secrets: &[String],
        feed: &Feed,
    ) -> Result<(), String> {
        if self.scope.map(|scope| scope.1) != Some(batch.generation) {
            return Err("服务日志会话已变更".into());
        }
        if self.stream_started_at != Some(batch.stream_started_at) {
            self.stream_started_at = Some(batch.stream_started_at);
            self.after = 0;
        }
        if self.after != 0 && batch.oldest > self.after.saturating_add(1) {
            feed.emit(
                Level::Warning,
                "logging",
                "后台服务日志窗口已覆盖尚未读取的记录，部分内核日志缺失",
            );
        }
        for entry in batch.entries {
            if entry.sequence > self.after {
                output.write(entry.line.as_bytes(), secrets, entry.stderr);
                self.after = entry.sequence;
            }
        }
        self.after = self.after.max(batch.next);
        Ok(())
    }
}
pub(super) async fn run(
    paths: AppPaths,
    feed: Feed,
    mut targets: watch::Receiver<Option<Target>>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut backoff = Duration::from_millis(250);
    let mut failure = None;
    let mut failures = 0u8;
    let mut scope = None;
    let mut service_cursor = ServiceCursor::default();
    'supervisor: loop {
        if *shutdown.borrow() {
            break;
        }
        let current = targets.borrow_and_update().clone();
        let Some(target) = current else {
            tokio::select! {_=shutdown.changed()=>break,changed=targets.changed()=>{if changed.is_err(){break;}}}
            continue;
        };
        if scope != Some((target.generation, target.epoch)) {
            scope = Some((target.generation, target.epoch));
            failures = 0;
            failure = None;
        }
        let Some(mut source) = target.logs else {
            tokio::select! {_=shutdown.changed()=>break,changed=targets.changed()=>{if changed.is_err(){break;}}}
            continue;
        };
        if let Some((credentials, proof)) = &source.service {
            service_cursor.bind(target.generation, proof.generation);
            let mut output = Output::new(feed.clone(), Default::default());
            output.configure(&paths, source.settings.borrow().1);
            let mut polling = tokio::time::interval(Duration::from_millis(250));
            polling.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let result: Result<(), String> = loop {
                tokio::select! { biased;
                    _ = shutdown.changed() => break 'supervisor,
                    changed = targets.changed() => { if changed.is_err() { break 'supervisor; } continue 'supervisor; },
                    changed = source.settings.changed() => {
                        if changed.is_err() { break Err("内核日志设置通道已关闭".into()); }
                        output.configure(&paths, source.settings.borrow_and_update().1);
                    },
                    _ = polling.tick() => {
                        let batch = tokio::select! { biased;
                            _ = shutdown.changed() => break 'supervisor,
                            changed = targets.changed() => { if changed.is_err() { break 'supervisor; } continue 'supervisor; },
                            result = suki_service::get_clash_logs(credentials, proof, suki_service::CoreLogRequest { after: service_cursor.after, stream_started_at: service_cursor.stream_started_at }) => result,
                        };
                        match batch {
                            Ok(response) if response.code == 0 => match response.data {
                                Some(batch) => {
                                    if let Err(error) = service_cursor.publish(batch, &mut output, &source.settings.borrow().0, &feed) { break Err(error); }
                                    failure = None; failures = 0; backoff = Duration::from_millis(250);
                                },
                                None => break Err("后台服务未返回日志窗口".into()),
                            },
                            Ok(response) => break Err(format!("后台服务拒绝日志读取请求，状态码 {}（{}）", response.code, response.message)),
                            Err(error) => break Err(format!("后台服务日志连接中断（{error}）")),
                        }
                    },
                }
            };
            if let Err(error) = result {
                if failure.as_ref() != Some(&error) {
                    feed.emit(Level::Warning, "logging", &error);
                    failure = Some(error);
                }
                failures += 1;
                if failures >= 3 {
                    feed.emit(
                        Level::Warning,
                        "logging",
                        "后台服务日志连续读取失败，已停止自动重连",
                    );
                    tokio::select! { _ = shutdown.changed() => break, changed = targets.changed() => { if changed.is_err() { break; } } }
                    continue;
                }
                tokio::select! { _ = shutdown.changed() => break, changed = targets.changed() => { if changed.is_err() { break; } }, _ = tokio::time::sleep(backoff) => {} }
                backoff = (backoff * 2).min(Duration::from_secs(8));
                continue;
            }
        }
        let connection = tokio::select! {biased;_=shutdown.changed()=>break,changed=targets.changed()=>{if changed.is_err(){break;}continue;},result=target.client.websocket("/logs?level=debug")=>result};
        let result: Result<(), String> = match connection {
            Err(error) => Err(error),
            Ok(mut socket) => {
                source.pipe_suppressed.store(true, Ordering::Release);
                let _guard = PipeGuard(source.pipe_suppressed.clone());
                let mut output = Output::new(feed.clone(), Default::default());
                output.configure(&paths, source.settings.borrow().1);
                let mut heartbeat = tokio::time::interval(Duration::from_secs(10));
                heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                let mut alive = tokio::time::Instant::now();
                let connected_at = tokio::time::Instant::now();
                loop {
                    use tokio_tungstenite::tungstenite::Message;
                    tokio::select! {biased;
                        _=shutdown.changed()=>{let _=tokio::time::timeout(Duration::from_millis(250),socket.close(None)).await;break 'supervisor;},
                        changed=targets.changed()=>{let _=tokio::time::timeout(Duration::from_millis(250),socket.close(None)).await;if changed.is_err(){break 'supervisor;}backoff=Duration::from_millis(250);continue 'supervisor;},
                        changed=source.settings.changed()=>{if changed.is_err(){break Err("内核日志设置通道已关闭".into());}let limits=source.settings.borrow_and_update().1;output.configure(&paths,limits);},
                        _=heartbeat.tick()=>{
                            // Mihomo only writes log frames; it does not read client Ping frames.
                            // A quiet log stream is healthy while its private control API responds.
                            if alive.elapsed() >= Duration::from_secs(30) {
                                let health = tokio::select! { biased;
                                    _ = shutdown.changed() => break 'supervisor,
                                    changed = targets.changed() => { if changed.is_err() { break 'supervisor; } continue 'supervisor; },
                                    result = target.client.version() => result,
                                };
                                if let Err(error) = health { break Err(error); }
                                alive = tokio::time::Instant::now();
                                failures = 0;
                                failure = None;
                            }
                        },
                        next=socket.next()=>match next{
                            Some(Ok(Message::Text(bytes)))=>{alive=tokio::time::Instant::now();match line(bytes.as_bytes()){Ok(line)=>output.write(line.as_bytes(),&source.settings.borrow().0,false),Err(error)=>break Err(error)}if connected_at.elapsed()>=Duration::from_secs(10){failures=0;}failure=None;backoff=Duration::from_millis(250);},
                            Some(Ok(Message::Binary(bytes)))=>{alive=tokio::time::Instant::now();match line(&bytes){Ok(line)=>output.write(line.as_bytes(),&source.settings.borrow().0,false),Err(error)=>break Err(error)}},
                            Some(Ok(Message::Ping(bytes)))=>{alive=tokio::time::Instant::now();if !matches!(tokio::time::timeout(Duration::from_secs(2),socket.send(Message::Pong(bytes))).await,Ok(Ok(()))){break Err("内核日志心跳响应失败".into());}},
                            Some(Ok(Message::Pong(_)))=>{alive=tokio::time::Instant::now();if connected_at.elapsed()>=Duration::from_secs(10){failures=0;}},
                            Some(Ok(Message::Frame(_)))=>{},
                            _=>break Err("内核日志连接中断".into()),
                        }
                    }
                }
            }
        };
        if let Err(error) = result
            && failure.as_ref() != Some(&error)
        {
            feed.emit(Level::Warning, "logging", &error);
            failure = Some(error);
        }
        failures += 1;
        if failures >= 3 {
            feed.emit(
                Level::Warning,
                "logging",
                "内核日志连接连续失败，已停止自动重连",
            );
            tokio::select! {biased;_=shutdown.changed()=>break,changed=targets.changed()=>if changed.is_err(){break;}}
            failures = 0;
            backoff = Duration::from_millis(250);
            continue;
        }
        tokio::select! {_=shutdown.changed()=>break,changed=targets.changed()=>{if changed.is_err(){break;}failures=0;},_=tokio::time::sleep(backoff)=>{}}
        backoff = (backoff * 2).min(Duration::from_secs(8));
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn service_history_cursor_preserves_repeated_lines_and_rejects_old_sessions() {
        use super::*;
        let feed = Feed::default();
        let mut output = Output::new(feed.clone(), Default::default());
        let mut cursor = ServiceCursor::default();
        cursor.bind(1, 7);
        let batch = suki_service::CoreLogBatch {
            stream_started_at: std::time::UNIX_EPOCH,
            generation: 7,
            oldest: 1,
            next: 2,
            entries: vec![
                suki_service::CoreLogEntry {
                    stderr: false,
                    sequence: 1,
                    line: "level=info msg=\"startup token\"".into(),
                },
                suki_service::CoreLogEntry {
                    stderr: false,
                    sequence: 2,
                    line: "level=info msg=\"startup token\"".into(),
                },
            ],
        };
        cursor
            .publish(batch.clone(), &mut output, &["token".into()], &feed)
            .unwrap();
        cursor
            .publish(batch, &mut output, &["token".into()], &feed)
            .unwrap();
        assert_eq!(feed.since(0).len(), 2);
        assert!(
            feed.since(0)
                .iter()
                .all(|record| !record.message.contains("token"))
        );
        cursor.bind(1, 7); // A new configuration epoch keeps the same service output cursor.
        assert_eq!(cursor.after, 2);
        cursor
            .publish(
                suki_service::CoreLogBatch {
                    stream_started_at: std::time::UNIX_EPOCH,
                    generation: 7,
                    oldest: 5,
                    next: 5,
                    entries: vec![suki_service::CoreLogEntry {
                        stderr: false,
                        sequence: 5,
                        line: "level=info msg=\"live\"".into(),
                    }],
                },
                &mut output,
                &[],
                &feed,
            )
            .unwrap();
        assert_eq!(feed.since(0).len(), 4); // explicit gap + actual live line
        cursor.bind(2, 8);
        assert_eq!(cursor.after, 0);
        assert!(
            cursor
                .publish(
                    suki_service::CoreLogBatch {
                        stream_started_at: std::time::UNIX_EPOCH,
                        generation: 7,
                        oldest: 1,
                        next: 6,
                        entries: vec![suki_service::CoreLogEntry {
                            stderr: false,
                            sequence: 6,
                            line: "stale".into()
                        }]
                    },
                    &mut output,
                    &[],
                    &feed
                )
                .is_err()
        );
        assert_eq!(feed.since(0).len(), 4);
    }
    #[test]
    fn websocket_log_levels_and_quoted_payload_are_preserved() {
        let text =
            super::line(br#"{"type":"warning","payload":"[DNS] failed: \"offline\""}"#).unwrap();
        let record = crate::logging::core_record(&text, false);
        assert_eq!(record.level, crate::logging::Level::Warning);
        assert_eq!(record.message, "[DNS] failed: \"offline\"");
        assert!(super::line(b"invalid").is_err());
    }
}

#[cfg(all(test, unix))]
mod supervision_tests {
    use super::*;
    #[tokio::test]
    async fn quiet_write_only_log_stream_survives_without_pong() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio_tungstenite::tungstenite::Message;
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        paths.create().unwrap();
        let path = root.path().join("quiet.sock");
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            // Match Mihomo: hold the connection without reading client control frames.
            tokio::time::timeout(Duration::from_secs(40), async {
                let (mut health, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                loop {
                    request.push(health.read_u8().await.unwrap());
                    if request.ends_with(b"\r\n\r\n") { break; }
                }
                assert!(request.starts_with(b"GET /version "));
                let body = r#"{"version":"v1.test"}"#;
                health.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
                health.shutdown().await.unwrap();
                ws.send(Message::Text(r#"{"type":"info","payload":"quiet stream survived"}"#.into())).await.unwrap();
            }).await.unwrap();
            ws
        });
        let (_settings, updates) = watch::channel((Vec::new(), (7, 1024 * 1024)));
        let suppressed = Arc::new(AtomicBool::new(false));
        let target = Target {
            client: super::super::client::Client::new(
                super::super::client::Endpoint::Unix(path),
                "secret".into(),
            ),
            logs: Some(Source {
                service: None,
                settings: updates,
                pipe_suppressed: suppressed.clone(),
            }),
            http: Default::default(),
            generation: 1,
            epoch: 1,
            order: vec![],
            proxy_port: 0,
            proxy_auth: None,
            dns_mode: None,
            proxy_error: None,
        };
        let (_targets, targets) = watch::channel(Some(target));
        let (shutdown, cancel) = watch::channel(false);
        let feed = Feed::default();
        let worker = tokio::spawn(run(paths, feed.clone(), targets, cancel));
        let socket = server.await.unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if feed
                    .since(0)
                    .iter()
                    .any(|record| record.message == "quiet stream survived")
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(
            !feed
                .since(0)
                .iter()
                .any(|record| record.level == Level::Warning)
        );
        assert!(suppressed.load(Ordering::Acquire));
        shutdown.send_replace(true);
        worker.await.unwrap();
        drop(socket);
        assert!(!suppressed.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn failed_log_connections_stop_after_three_and_publish_the_cause_once() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        paths.create().unwrap();
        let path = root.path().join("unavailable.sock");
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let (_settings, updates) = watch::channel((Vec::new(), (7, 1024 * 1024)));
        let suppressed = Arc::new(AtomicBool::new(false));
        let current = Target {
            client: super::super::client::Client::new(
                super::super::client::Endpoint::Unix(path),
                "secret".into(),
            ),
            logs: Some(Source {
                service: None,
                settings: updates,
                pipe_suppressed: suppressed.clone(),
            }),
            http: Default::default(),
            generation: 1,
            epoch: 1,
            order: vec![],
            proxy_port: 0,
            proxy_auth: None,
            dns_mode: None,
            proxy_error: None,
        };
        let (targets, receiver) = watch::channel(Some(current.clone()));
        let (shutdown, cancel) = watch::channel(false);
        let feed = Feed::default();
        let task = tokio::spawn(run(paths, feed.clone(), receiver, cancel));
        for _ in 0..3 {
            let (socket, _) = tokio::time::timeout(Duration::from_secs(3), listener.accept())
                .await
                .unwrap()
                .unwrap();
            drop(socket);
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(700), listener.accept())
                .await
                .is_err()
        );
        let records = feed.since(0);
        assert_eq!(
            records
                .iter()
                .filter(|record| record.message.contains("握手失败"))
                .count(),
            1
        );
        assert!(
            records
                .iter()
                .any(|record| record.message.contains("停止自动重连"))
        );
        assert!(!suppressed.load(Ordering::Acquire));
        let mut next = current;
        next.epoch += 1;
        targets.send_replace(Some(next));
        let (socket, _) = tokio::time::timeout(Duration::from_secs(2), listener.accept())
            .await
            .unwrap()
            .unwrap();
        drop(socket);
        shutdown.send_replace(true);
        task.await.unwrap();
    }
    #[tokio::test]
    async fn stream_handoff_redacts_payload_and_returns_pipe_ownership_on_shutdown() {
        use tokio_tungstenite::tungstenite::Message;
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        paths.create().unwrap();
        let socket = root.path().join("logs.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            ws.send(Message::Text(
                r#"{"type":"warning","payload":"[DNS] private-token"}"#.into(),
            ))
            .await
            .unwrap();
            while let Some(message) = ws.next().await {
                match message {
                    Ok(Message::Ping(payload)) => {
                        ws.send(Message::Pong(payload)).await.unwrap();
                    }
                    Ok(Message::Close(_)) | Err(_) => break,
                    _ => {}
                }
            }
        });
        let (settings, updates) = watch::channel((vec!["private-token".into()], (7, 1024 * 1024)));
        let suppressed = Arc::new(AtomicBool::new(false));
        let target = Target {
            client: super::super::client::Client::new(
                super::super::client::Endpoint::Unix(socket),
                "local-secret".into(),
            ),
            logs: Some(Source {
                service: None,
                settings: updates,
                pipe_suppressed: suppressed.clone(),
            }),
            http: Default::default(),
            generation: 2,
            epoch: 3,
            order: vec![],
            proxy_port: 0,
            proxy_auth: None,
            dns_mode: None,
            proxy_error: None,
        };
        let (_targets, targets) = watch::channel(Some(target));
        let (shutdown, cancel) = watch::channel(false);
        let feed = Feed::default();
        let task = tokio::spawn(run(paths.clone(), feed.clone(), targets, cancel));
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if feed
                    .since(0)
                    .iter()
                    .any(|record| record.message.contains("[REDACTED]"))
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(suppressed.load(Ordering::Acquire));
        assert!(
            feed.since(0)
                .iter()
                .all(|record| !record.message.contains("private-token"))
        );
        shutdown.send_replace(true);
        task.await.unwrap();
        server.await.unwrap();
        assert!(!suppressed.load(Ordering::Acquire));
        drop(settings);
        let saved = std::fs::read_dir(paths.directory(crate::storage::DirectoryKind::Logs))
            .unwrap()
            .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
            .collect::<String>();
        assert!(saved.contains("[REDACTED]"));
        assert!(!saved.contains("private-token"));
    }
}
