//! CV-style supervised Mihomo streams: bounded latest value, stale detection and scoped reconnect.
use super::{client::Socket, monitor::Target};
use futures_util::{SinkExt, StreamExt};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{sync::watch, task::JoinHandle};
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
    let mut backoff = Duration::from_millis(250);
    let mut failures = 0u8;
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
            failures = 0;
        }
        let connection = tokio::select! {biased;_=shutdown.changed()=>break,changed=target.changed()=>{if changed.is_err(){break;}backoff=Duration::from_millis(250);continue;},result=current.client.websocket(path)=>result};
        let result: Result<(), String> = match connection {
            Err(error) => Err(error),
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
                        changed=target.changed()=>{close(&mut socket).await;if changed.is_err(){break 'supervisor;}backoff=Duration::from_millis(250);continue 'supervisor;},
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
                        Ok(Some(Err(
                            error @ tokio_tungstenite::tungstenite::Error::Capacity(_),
                        ))) => {
                            break Err(crate::logging::rejected(
                                "内核 WebSocket 消息超过大小上限".into(),
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
                        backoff = Duration::from_millis(250);
                        failures = 0;
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
        failures += 1;
        if let Err(error) = result {
            log::debug!("内核流 {path} 连接失败（{failures}/3）：{error}");
            sender.send_replace(State {
                generation: current.generation,
                epoch: current.epoch,
                packet: None,
                error: (failures >= 3).then(|| format!("{error}；已停止自动重连")),
            });
        }
        if failures >= 3 {
            tokio::select! {biased;_=shutdown.changed()=>break,changed=target.changed()=>if changed.is_err(){break;}}
            failures = 0;
            backoff = Duration::from_millis(250);
            continue;
        }
        tokio::select! {biased;_=shutdown.changed()=>break,changed=target.changed()=>{if changed.is_err(){break;}backoff=Duration::from_millis(250);failures=0;},_=tokio::time::sleep(backoff)=>{backoff=(backoff*2).min(Duration::from_secs(5));}}
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::core::client::{Client, Endpoint};
    use tokio_tungstenite::{accept_hdr_async, tungstenite::Message};
    #[tokio::test]
    async fn repeated_handshake_failures_stop_after_three_attempts_until_target_changes() {
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
        assert!(
            feed.events
                .borrow()
                .error
                .as_deref()
                .unwrap()
                .contains("已停止自动重连")
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
