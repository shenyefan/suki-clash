//! Native process handles, Job Objects and private pipes; no installed core or OS proxy writes.
use super::*;
use crate::core::{
    client::{Client, Endpoint, random_token},
    ownership::Lifetime,
};
use std::{
    os::windows::io::{AsRawHandle as _, BorrowedHandle, OwnedHandle},
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::net::windows::named_pipe::ServerOptions;
use windows_sys::Win32::{Foundation::WAIT_OBJECT_0, System::Threading::WaitForSingleObject};

#[test]
#[ignore = "subprocess fixture; invoked by the Windows lifecycle tests with --exact"]
fn child_fixture() {
    match std::env::var("SUKI_LIFECYCLE_TEST_CHILD").as_deref() {
        Ok("sleep") => std::thread::sleep(Duration::from_secs(30)),
        Ok("exit") => std::process::exit(7),
        _ => panic!("this fixture must be started by a lifecycle test"),
    }
}

fn child(mode: &str) -> tokio::process::Child {
    tokio::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "core::service::tests::windows::child_fixture",
            "--ignored",
            "--nocapture",
        ])
        .env("SUKI_LIFECYCLE_TEST_CHILD", mode)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(0x08000000)
        .kill_on_drop(true)
        .spawn()
        .unwrap()
}

fn process_handle(child: &tokio::process::Child) -> OwnedHandle {
    // Duplicate the exact child handle so termination can be confirmed after Running is dropped.
    unsafe { BorrowedHandle::borrow_raw(child.raw_handle().unwrap()) }
        .try_clone_to_owned()
        .unwrap()
}

fn running(launch: Launch, child: tokio::process::Child) -> Running {
    let lifetime = Lifetime::attach(&child).unwrap();
    Running {
        child: Process::Local(child),
        launch,
        output: None,
        log_settings: None,
        pipe_suppressed: Default::default(),
        _lifetime: Some(lifetime),
        _execution: None,
    }
}

#[tokio::test]
async fn pipe_readiness_requires_config_readback_and_exit_reaps_the_owned_child() {
    use http_body_util::Full;
    use hyper::{Response, body::Bytes, service::service_fn};
    use hyper_util::rt::TokioIo;

    let (_root, paths) = fixture(Some(SLEEPING_CORE));
    let mut launch = Launch::prepare(&paths).await.unwrap();
    let candidate = launch.candidate_path().to_owned();
    let yaml: serde_yaml_ng::Value = serde_yaml_ng::from_str(&launch.composed).unwrap();
    let mut config = serde_json::to_value(yaml).unwrap();
    config["tun"] = serde_json::json!({"enable": false});
    let ready = Arc::new(AtomicBool::new(false));
    let pipe = format!(r"\\.\pipe\suki-lifecycle-test-{}", random_token().unwrap());
    let first = ServerOptions::new()
        .first_pipe_instance(true)
        .create(&pipe)
        .unwrap();
    launch.client = Client::new(Endpoint::Pipe(pipe.clone()), "test-secret".into());
    let server = tokio::spawn({
        let ready = ready.clone();
        async move {
            let mut listener = first;
            loop {
                listener.connect().await.unwrap();
                let stream = listener;
                listener = ServerOptions::new().create(&pipe).unwrap();
                let ready = ready.clone();
                let config = config.clone();
                let service = service_fn(move |request: hyper::Request<hyper::body::Incoming>| {
                    assert_eq!(request.headers()["authorization"], "Bearer test-secret");
                    let body = match request.uri().path() {
                        "/version" => serde_json::json!({"version": "windows-fixture"}),
                        "/configs" => {
                            let mut config = config.clone();
                            if !ready.load(Ordering::Acquire) {
                                config["mixed-port"] = 1.into();
                            }
                            config
                        }
                        path => panic!("unexpected private API request: {path}"),
                    };
                    std::future::ready(Ok::<_, std::convert::Infallible>(Response::new(Full::new(
                        Bytes::from(body.to_string()),
                    ))))
                });
                hyper::server::conn::http1::Builder::new()
                    .keep_alive(false)
                    .serve_connection(TokioIo::new(stream), service)
                    .await
                    .unwrap();
            }
        }
    });
    let child = child("sleep");
    let handle = process_handle(&child);
    let mut owner = actor(paths);
    owner.running = Some(running(launch, child));
    owner.publish(Phase::Starting, None);

    let error = owner
        .wait_ready(Duration::from_millis(250))
        .await
        .unwrap_err();
    assert!(error.contains("mixed-port"), "{error}");
    assert_eq!(owner.snapshot.phase(), Phase::Starting);
    assert!(owner.monitor.borrow().is_none());
    assert!(
        owner
            .running
            .as_mut()
            .unwrap()
            .child
            .try_wait()
            .unwrap()
            .is_none()
    );

    ready.store(true, Ordering::Release);
    let version = owner.wait_ready(Duration::from_secs(3)).await.unwrap();
    assert_eq!(version, "windows-fixture");
    assert_eq!(owner.snapshot.tun(), Some(false));
    owner.snapshot.version = Some(version);
    owner.publish(Phase::Ready, None);
    assert!(owner.monitor.borrow().is_some());

    owner.prepare_exit().await.unwrap();
    assert!(owner.exit_prepared);
    assert_eq!(owner.snapshot.phase(), Phase::Stopped);
    assert!(owner.running.is_none());
    assert!(owner.monitor.borrow().is_none());
    assert!(owner.log_target.borrow().is_none());
    assert!(!candidate.exists());
    assert_eq!(
        unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) },
        WAIT_OBJECT_0
    );
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn stopping_the_owned_process_keeps_an_unrelated_process_alive() {
    let (_root, paths) = fixture(Some(SLEEPING_CORE));
    let launch = Launch::prepare(&paths).await.unwrap();
    let owned = child("sleep");
    let handle = process_handle(&owned);
    let mut unrelated = child("sleep");
    let mut owner = actor(paths);
    owner.running = Some(running(launch, owned));
    owner.publish(Phase::Ready, None);

    owner.stop().await.unwrap();
    assert_eq!(
        unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) },
        WAIT_OBJECT_0
    );
    assert_eq!(owner.snapshot.phase(), Phase::Stopped);
    assert!(owner.running.is_none());
    assert!(unrelated.try_wait().unwrap().is_none());
    unrelated.kill().await.unwrap();
}

#[tokio::test]
async fn closing_the_job_reaps_its_child_and_preserves_an_unrelated_process() {
    let mut owned = child("sleep");
    let lifetime = Lifetime::attach(&owned).unwrap();
    let mut unrelated = child("sleep");

    assert!(owned.try_wait().unwrap().is_none());
    drop(lifetime);
    tokio::time::timeout(Duration::from_secs(3), owned.wait())
        .await
        .unwrap()
        .unwrap();
    assert!(unrelated.try_wait().unwrap().is_none());
    unrelated.kill().await.unwrap();
}

#[tokio::test]
async fn unexpected_native_child_exit_clears_readiness_and_runtime_targets() {
    let (_root, paths) = fixture(Some(SLEEPING_CORE));
    let launch = Launch::prepare(&paths).await.unwrap();
    let mut owner = actor(paths);
    owner.running = Some(running(launch, child("exit")));
    owner.snapshot.version = Some("previous-version".into());
    owner.publish(Phase::Ready, None);
    let status = tokio::time::timeout(
        Duration::from_secs(5),
        owner.running.as_mut().unwrap().child.wait(),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(status.code(), Some(7));

    owner.observe_exit().await;
    assert_eq!(owner.snapshot.phase(), Phase::Failed);
    assert!(owner.snapshot.error().unwrap().contains("意外退出"));
    assert!(!owner.snapshot.has_process());
    assert!(owner.snapshot.version().is_none());
    assert!(owner.running.is_none());
    assert!(owner.monitor.borrow().is_none());
    assert!(owner.log_target.borrow().is_none());
}
