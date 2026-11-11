//! CV-style supervised Mihomo streams: bounded latest value, stale detection and scoped reconnect.
use super::{client::Socket, monitor::Target};
use futures_util::{SinkExt, StreamExt};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{sync::watch, task::JoinHandle};

/// Shared by log and data subscriptions; requests are read-only and each attempt is bounded.
#[derive(Default)]
pub(super) struct Reconnect {
    failures: usize,
}
impl Reconnect {
    pub(super) fn failed(&mut self) -> (bool, Duration) {
        const DELAYS: [u64; 6] = [1, 2, 4, 8, 15, 30];
        let first = self.failures == 0;
        let delay = DELAYS[self.failures.min(DELAYS.len() - 1)];
        self.failures = (self.failures + 1).min(DELAYS.len());
        (first, Duration::from_secs(delay))
    }
    pub(super) fn recovered(&mut self) -> bool {
        let failed = self.failures != 0;
        self.failures = 0;
        failed
    }
}

#[cfg(test)]
mod retry_policy_tests {
    use super::*;
    #[test]
    fn retry_delay_caps_without_stopping_and_resets_after_recovery() {
        let mut retry = Reconnect::default();
        for (index, seconds) in [1, 2, 4, 8, 15, 30, 30, 30].into_iter().enumerate() {
            assert_eq!(retry.failed(), (index == 0, Duration::from_secs(seconds)));
        }
        assert!(retry.recovered());
        assert_eq!(retry.failed(), (true, Duration::from_secs(1)));
    }
}

#[cfg(all(test, windows))]
mod reconnect_tests {
    use super::super::{
        client::{Client, Endpoint},
        log_stream,
    };
    use super::*;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::windows::named_pipe::ServerOptions,
    };

    fn target(path: String) -> Target {
        Target {
            logs: None,
            http: Default::default(),
            client: Client::new(Endpoint::Pipe(path), "secret".into()),
            generation: 1,
            epoch: 1,
            order: vec![],
            proxy_port: 0,
            proxy_auth: None,
            dns_mode: None,
            proxy_error: None,
        }
    }

    #[tokio::test]
    async fn data_and_logs_recover_after_four_failures_without_target_change() {
        for logs in [false, true] {
            let path = format!(r"\\.\pipe\suki-reconnect-{}-{logs}", std::process::id());
            let mut listener = ServerOptions::new()
                .first_pipe_instance(true)
                .create(&path)
                .unwrap();
            let mut current = target(path.clone());
            let server = tokio::spawn(async move {
                for _ in 0..4 {
                    listener.connect().await.unwrap();
                    let next = ServerOptions::new().create(&path).unwrap();
                    drop(listener);
                    listener = next;
                }
                listener.connect().await.unwrap();
                let mut socket = tokio_tungstenite::accept_async(listener).await.unwrap();
                let message = if logs {
                    r#"{"type":"info","payload":"recovered log"}"#
                } else {
                    r#"{"up":1,"down":2}"#
                };
                socket
                    .send(tokio_tungstenite::tungstenite::Message::Text(
                        message.into(),
                    ))
                    .await
                    .unwrap();
                while socket.next().await.is_some() {}
            });
            let root = tempfile::tempdir().unwrap();
            let paths = crate::storage::AppPaths::isolated(root.path());
            paths.create().unwrap();
            let feed = crate::logging::Feed::default();
            let (_settings, settings) = watch::channel((vec![], (7, 1024 * 1024)));
            if logs {
                current.logs = Some(log_stream::Source {
                    service: None,
                    settings,
                    pipe_suppressed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                });
            }
            let (_targets, targets) = watch::channel(Some(current));
            let (shutdown, cancel) = watch::channel(false);
            let (sender, mut events) = watch::channel(State::default());
            let worker = if logs {
                tokio::spawn(log_stream::run(paths, feed.clone(), targets, cancel))
            } else {
                tokio::spawn(run("/traffic", targets, cancel, sender))
            };
            tokio::time::timeout(Duration::from_secs(25), async {
                if logs {
                    loop {
                        if feed
                            .since(0)
                            .iter()
                            .any(|record| record.message == "recovered log")
                        {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                } else {
                    loop {
                        events.changed().await.unwrap();
                        if events.borrow_and_update().packet.is_some() {
                            break;
                        }
                    }
                }
            })
            .await
            .unwrap();
            if logs {
                let records = feed.since(0);
                assert_eq!(
                    records
                        .iter()
                        .filter(|record| record.level == crate::logging::Level::Warning)
                        .count(),
                    1
                );
                assert!(
                    records
                        .iter()
                        .any(|record| record.message == "内核日志获取已恢复")
                );
            }
            shutdown.send_replace(true);
            tokio::time::timeout(Duration::from_secs(2), worker)
                .await
                .unwrap()
                .unwrap();
            server.abort();
            let _ = server.await;
        }
    }

    #[tokio::test]
    async fn authentication_rejection_waits_for_target_change() {
        let path = format!(r"\\.\pipe\suki-rejected-{}", std::process::id());
        let listener = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&path)
            .unwrap();
        let mut current = target(path.clone());
        let (targets, receiver) = watch::channel(Some(current.clone()));
        let (shutdown, cancel) = watch::channel(false);
        let mut feed = Feed::start("/traffic", receiver, cancel);
        listener.connect().await.unwrap();
        let next = ServerOptions::new().create(&path).unwrap();
        let mut listener = listener;
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            request.push(listener.read_u8().await.unwrap());
        }
        listener
            .write_all(
                b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        listener.shutdown().await.unwrap();
        drop(listener);
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                feed.events.changed().await.unwrap();
                if feed.events.borrow_and_update().error.is_some() {
                    break;
                }
            }
        })
        .await
        .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(1500), next.connect())
                .await
                .is_err()
        );
        current.epoch += 1;
        targets.send_replace(Some(current));
        tokio::time::timeout(Duration::from_secs(2), next.connect())
            .await
            .unwrap()
            .unwrap();
        shutdown.send_replace(true);
        feed.finish().await;
    }
}
#[derive(Clone)]
pub(super) struct Packet {
    pub(super) generation: u64,
    pub(super) epoch: u64,
    pub(super) bytes: hyper::body::Bytes,
    pub(super) at: Instant,
}
#[derive(Clone, Default)]
pub(super) struct State {
    pub(super) generation: u64,
    pub(super) epoch: u64,
    pub(super) packet: Option<Arc<Packet>>,
    pub(super) error: Option<String>,
}
pub(super) struct Feed {
    pub(super) events: watch::Receiver<State>,
    task: Option<JoinHandle<()>>,
}
impl Feed {
    pub(super) fn start(
        path: &'static str,
        target: watch::Receiver<Option<Target>>,
        shutdown: watch::Receiver<bool>,
    ) -> Self {
        let (sender, events) = watch::channel(State::default());
        let task = tokio::spawn(run(path, target, shutdown, sender));
        Self {
            events,
            task: Some(task),
        }
    }
    pub(super) async fn finish(mut self) {
        if let Some(mut task) = self.task.take()
            && tokio::time::timeout(Duration::from_millis(500), &mut task)
                .await
                .is_err()
        {
            task.abort();
            let _ = task.await;
        }
    }
}
impl Drop for Feed {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}
async fn close(socket: &mut Socket) {
    let _ = tokio::time::timeout(
        Duration::from_millis(250),
        socket.close(Some(tokio_tungstenite::tungstenite::protocol::CloseFrame {
            code: tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Normal,
            reason: "".into(),
        })),
    )
    .await;
}
async fn run(
    path: &'static str,
    mut target: watch::Receiver<Option<Target>>,
    mut shutdown: watch::Receiver<bool>,
    sender: watch::Sender<State>,
) {
    let mut reconnect = Reconnect::default();
    let mut scope = None;
    'supervisor: loop {
        if *shutdown.borrow() {
            break;
        }
        let current = target.borrow().clone();
        let Some(current) = current else {
            sender.send_replace(State::default());
            tokio::select! {biased;_=shutdown.changed()=>break,changed=target.changed()=>if changed.is_err(){break;}}
            continue;
        };
        if scope != Some((current.generation, current.epoch)) {
            sender.send_replace(State::default());
            scope = Some((current.generation, current.epoch));
            reconnect = Reconnect::default();
        }
        let mut rejected = false;
        let connection = tokio::select! {biased;_=shutdown.changed()=>break,changed=target.changed()=>{if changed.is_err(){break;}continue;},result=current.client.websocket(path)=>result};
        let result: Result<(), String> = match connection {
            Err(error) => {
                rejected = error.rejected;
                Err(error.message)
            }
            Ok(mut socket) => {
                let connected_at = Instant::now();
                let mut last_data = Instant::now();
                loop {
                    if last_data.elapsed() >= Duration::from_secs(15) {
                        close(&mut socket).await;
                        break Err("内核 WebSocket 数据超时".into());
                    }
                    let next = tokio::select! {
                        biased;
                        _=shutdown.changed()=>{close(&mut socket).await;break 'supervisor;},
                        changed=target.changed()=>{close(&mut socket).await;if changed.is_err(){break 'supervisor;}continue 'supervisor;},
                        next=tokio::time::timeout(Duration::from_secs(15).saturating_sub(last_data.elapsed()),socket.next())=>next,
                    };
                    use tokio_tungstenite::tungstenite::Message;
                    let bytes = match next {
                        Ok(Some(Ok(Message::Text(text)))) => {
                            hyper::body::Bytes::copy_from_slice(text.as_bytes())
                        }
                        Ok(Some(Ok(Message::Binary(bytes)))) => bytes,
                        Ok(Some(Ok(Message::Ping(_)))) => {
                            if let Err(error) = socket.flush().await {
                                break Err(crate::logging::diagnostic_error(
                                    "内核 WebSocket 心跳发送失败".into(),
                                    crate::logging::error_chain(&error),
                                ));
                            }
                            continue;
                        }
                        Ok(Some(Ok(Message::Pong(_)))) => continue,
                        Ok(Some(Ok(Message::Frame(_)))) => continue,
                        Ok(Some(Ok(Message::Close(_)))) | Ok(None) => {
                            break Err("内核 WebSocket 已关闭".into());
                        }
                        Ok(Some(Err(error))) if super::client::SocketError::is_rejected(&error) => {
                            rejected = true;
                            break Err(crate::logging::rejected(
                                "内核 WebSocket 协议错误".into(),
                                error,
                            ));
                        }
                        Ok(Some(Err(error))) => {
                            break Err(crate::logging::diagnostic_error(
                                "内核 WebSocket 传输失败".into(),
                                crate::logging::error_chain(&error),
                            ));
                        }
                        Err(_) => {
                            close(&mut socket).await;
                            break Err("内核 WebSocket 数据超时".into());
                        }
                    };
                    last_data = Instant::now();
                    if connected_at.elapsed() >= Duration::from_secs(10) {
                        reconnect.recovered();
                    }
                    if sender.borrow().error.is_some() {
                        log::trace!("内核流已恢复，接口 {path}");
                    }
                    sender.send_replace(State {
                        generation: current.generation,
                        epoch: current.epoch,
                        packet: Some(Arc::new(Packet {
                            generation: current.generation,
                            epoch: current.epoch,
                            bytes,
                            at: Instant::now(),
                        })),
                        error: None,
                    });
                }
            }
        };
        let (_, delay) = reconnect.failed();
        if let Err(error) = result {
            log::trace!("内核流获取失败，接口 {path}，原因 {error}");
            sender.send_replace(State {
                generation: current.generation,
                epoch: current.epoch,
                packet: None,
                error: Some(if rejected {
                    error
                } else {
                    format!("{error}；正在自动重连")
                }),
            });
        }
        if rejected {
            tokio::select! {biased;_=shutdown.changed()=>break,changed=target.changed()=>if changed.is_err(){break;}}
            reconnect = Reconnect::default();
            continue;
        }
        tokio::select! {biased;_=shutdown.changed()=>break,changed=target.changed()=>{if changed.is_err(){break;}},_=tokio::time::sleep(delay)=>{}}
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::core::client::{Client, Endpoint};
    use tokio_tungstenite::{accept_hdr_async, tungstenite::Message};
    #[tokio::test]
    async fn repeated_handshake_failures_keep_retrying_and_target_changes_skip_backoff() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("failed.sock");
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let current = Target {
            logs: None,
            http: Default::default(),
            client: Client::new(Endpoint::Unix(path), "secret".into()),
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
        let feed = Feed::start("/traffic", receiver, cancel);
        for _ in 0..4 {
            let (socket, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
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
        assert!(
            feed.events
                .borrow()
                .error
                .as_deref()
                .unwrap()
                .contains("正在自动重连")
        );
        let mut next = current;
        next.epoch += 1;
        targets.send_replace(Some(next));
        let (socket, _) = tokio::time::timeout(Duration::from_secs(2), listener.accept())
            .await
            .unwrap()
            .unwrap();
        drop(socket);
        shutdown.send(true).unwrap();
        feed.finish().await;
    }
    #[tokio::test]
    async fn authenticates_reconnects_responds_to_ping_and_closes_on_generation_change() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("api.sock");
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let server = tokio::spawn(async move {
            for index in 0..2 {
                let (stream, _) = listener.accept().await.unwrap();
                #[allow(
                    clippy::result_large_err,
                    reason = "tungstenite requires an owned HTTP rejection response for handshake callbacks"
                )]
                fn authenticate(
                    request: &tokio_tungstenite::tungstenite::handshake::server::Request,
                    response: tokio_tungstenite::tungstenite::handshake::server::Response,
                ) -> Result<
                    tokio_tungstenite::tungstenite::handshake::server::Response,
                    tokio_tungstenite::tungstenite::handshake::server::ErrorResponse,
                > {
                    assert_eq!(request.uri().path(), "/traffic");
                    assert_eq!(request.headers()["authorization"], "Bearer private-secret");
                    Ok(response)
                }
                let mut socket = accept_hdr_async(stream, authenticate).await.unwrap();
                socket
                    .send(Message::Ping(vec![1, 2, 3].into()))
                    .await
                    .unwrap();
                let pong = tokio::time::timeout(Duration::from_secs(1), socket.next())
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap();
                assert!(matches!(pong, Message::Pong(_)));
                socket
                    .send(Message::Text(
                        format!("{{\"up\":{index},\"down\":0}}").into(),
                    ))
                    .await
                    .unwrap();
                if index == 0 {
                    socket.close(None).await.unwrap();
                } else {
                    let closed = tokio::time::timeout(Duration::from_secs(2), socket.next())
                        .await
                        .unwrap();
                    assert!(matches!(closed, Some(Ok(Message::Close(_))) | None));
                }
            }
        });
        let target = Target {
            logs: None,
            http: crate::network::Http::default(),
            client: Client::new(Endpoint::Unix(path), "private-secret".into()),
            generation: 1,
            epoch: 1,
            order: vec![],
            proxy_port: 0,
            proxy_auth: None,
            dns_mode: None,
            proxy_error: None,
        };
        let (targets, receiver) = watch::channel(Some(target));
        let (shutdown, cancellation) = watch::channel(false);
        let mut feed = Feed::start("/traffic", receiver, cancellation);
        tokio::time::timeout(Duration::from_secs(4), async {
            loop {
                feed.events.changed().await.unwrap();
                let value = feed.events.borrow_and_update().clone();
                if let Some(packet) = value.packet
                    && packet.bytes == br#"{"up":1,"down":0}"#[..]
                {
                    assert_eq!(packet.generation, 1);
                    break;
                }
            }
        })
        .await
        .unwrap();
        targets.send_replace(None);
        server.await.unwrap();
        shutdown.send_replace(true);
        feed.finish().await;
    }
}
