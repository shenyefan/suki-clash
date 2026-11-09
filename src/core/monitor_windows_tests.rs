use super::*;
use crate::core::client::{Client, Endpoint, random_token};
use futures_util::{SinkExt as _, StreamExt as _};
use http_body_util::Full;
use hyper::{Request, Response, body::Bytes, service::service_fn};
use hyper_util::rt::TokioIo;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use tokio_tungstenite::tungstenite::{Message, handshake::server::create_response, protocol::Role};

async fn serve(stream: NamedPipeServer, controls: Arc<AtomicUsize>) {
    let (upgrades, pending) = async_channel::bounded(1);
    let service = service_fn(move |mut request: Request<hyper::body::Incoming>| {
        let path = request.uri().path().to_owned();
        let response = if request.headers().contains_key("sec-websocket-key") {
            let upgrade = hyper::upgrade::on(&mut request);
            let response = create_response(&request.map(|_| ())).unwrap();
            upgrades.try_send((path, upgrade)).unwrap();
            response.map(|_| Full::new(Bytes::new()))
        } else {
            let body = match path.as_str() {
                "/configs" => {
                    controls.fetch_add(1, Ordering::SeqCst);
                    r#"{"mode":"rule","mixed-port":0}"#
                }
                "/proxies" => r#"{"proxies":{}}"#,
                "/providers/proxies" => r#"{"providers":{}}"#,
                _ => panic!("unexpected request {path}"),
            };
            Response::new(Full::new(Bytes::from_static(body.as_bytes())))
        };
        std::future::ready(Ok::<_, std::convert::Infallible>(response))
    });
    let _ = hyper::server::conn::http1::Builder::new()
        .serve_connection(TokioIo::new(stream), service)
        .with_upgrades()
        .await;
    let Ok((path, upgrade)) = pending.try_recv() else {
        return;
    };
    let mut socket = tokio_tungstenite::WebSocketStream::from_raw_socket(
        TokioIo::new(upgrade.await.unwrap()),
        Role::Server,
        None,
    )
    .await;
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut total = 0;
    loop {
        tokio::select! {
            _ = tick.tick() => {
                total += 100;
                let body = match path.as_str() {
                    "/connections" => format!(r#"{{"uploadTotal":{total},"downloadTotal":{total},"connections":[]}}"#),
                    "/traffic" => r#"{"up":100,"down":200}"#.into(),
                    "/memory" => r#"{"inuse":128}"#.into(),
                    _ => panic!("unexpected stream {path}"),
                };
                if socket.send(Message::Text(body.into())).await.is_err() { break; }
            }
            next = socket.next() => match next {
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                _ => {}
            }
        }
    }
}

#[tokio::test]
async fn proxy_poll_is_fifteen_seconds_visible_only_and_hidden_sampling_continues() {
    let root = tempfile::tempdir().unwrap();
    let paths = AppPaths::isolated(root.path());
    paths.create().unwrap();
    let pipe = format!(r"\\.\pipe\suki-monitor-test-{}", random_token().unwrap());
    let first = ServerOptions::new()
        .first_pipe_instance(true)
        .create(&pipe)
        .unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let server = tokio::spawn({
        let count = count.clone();
        let pipe = pipe.clone();
        async move {
            let mut listener = first;
            let mut connections = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    connected = listener.connect() => {
                        connected.unwrap();
                        let stream = listener;
                        listener = ServerOptions::new().create(&pipe).unwrap();
                        connections.spawn(serve(stream, count.clone()));
                    }
                    result = connections.join_next(), if !connections.is_empty() => { result.unwrap().unwrap(); }
                }
            }
        }
    });
    let (_targets, targets) = watch::channel(Some(Target {
        client: Client::new(Endpoint::Pipe(pipe), "test".into()),
        logs: None,
        http: Default::default(),
        generation: 1,
        epoch: 1,
        order: vec![],
        proxy_port: 0,
        proxy_auth: None,
        dns_mode: None,
        proxy_error: None,
    }));
    let (controls, control_events) = watch::channel(ProbeControl {
        visible: true,
        ..Default::default()
    });
    let (shutdown, cancellation) = watch::channel(false);
    let (updates, events) = async_channel::bounded(1);
    let task = tokio::spawn(run(
        paths,
        targets,
        updates,
        events.clone(),
        cancellation,
        None,
        Some(control_events),
    ));
    tokio::time::timeout(Duration::from_secs(3), async {
        while !events.recv().await.unwrap().traffic_live {}
    })
    .await
    .unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
    tokio::time::sleep(Duration::from_secs(7)).await;
    assert_eq!(
        count.load(Ordering::SeqCst),
        1,
        "the old five-second poll must be gone"
    );
    tokio::time::timeout(Duration::from_secs(11), async {
        while count.load(Ordering::SeqCst) < 2 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let baseline = events.recv().await.unwrap().connections.upload_total;
    controls.send_modify(|control| control.visible = false);
    tokio::time::sleep(Duration::from_secs(17)).await;
    assert_eq!(
        count.load(Ordering::SeqCst),
        2,
        "hidden windows must not poll proxy state"
    );
    let background = events.recv().await.unwrap();
    assert!(background.traffic_live && background.connections_live);
    assert!(
        background.connections.upload_total > baseline,
        "background sampling must continue"
    );
    controls.send_modify(|control| control.visible = true);
    tokio::time::timeout(Duration::from_secs(2), async {
        while count.load(Ordering::SeqCst) < 3 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        count.load(Ordering::SeqCst),
        3,
        "becoming visible refreshes once"
    );
    shutdown.send_replace(true);
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap();
    server.abort();
}
