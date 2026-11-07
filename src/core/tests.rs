//! Child-process tests use private temporary directories and no system networking.
use super::{Actor, Command, Process, Running};
use crate::core::{Phase, Service, launch::Launch};
use crate::storage::{AppPaths, DirectoryKind, Repository};
use std::{path::Path, time::Duration};

fn fixture(script: Option<&str>) -> (tempfile::TempDir, AppPaths) {
    use std::os::unix::fs::PermissionsExt as _;
    let root = tempfile::Builder::new()
        .prefix("suki-core-")
        .tempdir_in("/tmp")
        .unwrap();
    let paths = AppPaths::isolated(root.path());
    Repository::open(paths.clone()).unwrap();
    core_settings(&paths, 0);
    std::fs::write(
        paths
            .directory(DirectoryKind::Config)
            .join("runtime-source.yaml"),
        "mixed-port: 0\nport: 0\nsocks-port: 0\nlog-level: silent\nrules: ['MATCH,DIRECT']\n",
    )
    .unwrap();
    if let Some(script) = script {
        let binary = paths.directory(DirectoryKind::Core).join("mihomo");
        std::fs::write(&binary, script).unwrap();
        std::fs::set_permissions(binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    (root, paths)
}

fn actor(paths: AppPaths) -> Actor {
    let (sender, receiver) = async_channel::bounded(1);
    Actor::new(paths, sender, receiver)
}

const SLEEPING_CORE: &str = "#!/bin/sh\nfor arg do\nif [ \"$arg\" = -t ]; then exit 0; fi\ndone\n: > child-started\nexec /bin/sleep 30\n";

#[tokio::test]
async fn late_provider_selection_settles_without_overwriting_new_choices_and_expires() {
    use crate::proxies::runtime::{Action, Selections};
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let (_root, paths) = fixture(Some(SLEEPING_CORE));
    let mut launch = Launch::prepare(&paths).await.unwrap();
    let socket = paths.data().join("selection.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    launch.client = super::super::client::Client::new(
        super::super::client::Endpoint::Unix(socket),
        String::new(),
    );
    let loaded = Arc::new(AtomicBool::new(false));
    let requests = Arc::new(AtomicUsize::new(0));
    let selected = Arc::new(Mutex::new(
        std::collections::BTreeMap::<String, String>::new(),
    ));
    let server = tokio::spawn({
        let loaded = loaded.clone();
        let requests = requests.clone();
        let selected = selected.clone();
        async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                let header_end = loop {
                    let mut part = [0; 4096];
                    let n = stream.read(&mut part).await.unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&part[..n]);
                    if let Some(end) = bytes.windows(4).position(|p| p == b"\r\n\r\n") {
                        break end + 4;
                    }
                };
                let header = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
                let length = header
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                while bytes.len() < header_end + length {
                    let mut part = [0; 4096];
                    let n = stream.read(&mut part).await.unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&part[..n]);
                }
                requests.fetch_add(1, Ordering::SeqCst);
                let route = header
                    .lines()
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap();
                let body = if header.starts_with("PUT /proxies/") {
                    let body: serde_json::Value =
                        serde_json::from_slice(&bytes[header_end..]).unwrap();
                    selected.lock().unwrap().insert(
                        route.trim_start_matches("/proxies/").to_owned(),
                        body["name"].as_str().unwrap().into(),
                    );
                    serde_json::json!({})
                } else if route == "/proxies" {
                    if loaded.load(Ordering::SeqCst) {
                        let selected = selected.lock().unwrap();
                        serde_json::json!({"proxies":{
                            "g":{"name":"g","type":"Selector","all":["late","chosen"],"now":selected.get("g").map(String::as_str).unwrap_or("chosen")},
                            "other":{"name":"other","type":"Selector","all":["later"],"now":selected.get("other").map(String::as_str).unwrap_or("DIRECT")},
                            "late":{"name":"late","type":"Socks5"},"later":{"name":"later","type":"Socks5"},"chosen":{"name":"chosen","type":"Socks5"}
                        }})
                    } else {
                        serde_json::json!({"proxies":{}})
                    }
                } else {
                    serde_json::json!({"providers":{}})
                };
                let body = body.to_string();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        }
    });
    let mut saved = Selections::load(&paths).unwrap();
    saved.update(None, "g", Some("late"));
    saved.update(None, "other", Some("later"));
    saved.save(&paths).unwrap();
    let mut actor = actor(paths);
    actor.running = Some(Running {
        child: Process::Local(
            tokio::process::Command::new("/bin/sleep")
                .arg("30")
                .spawn()
                .unwrap(),
        ),
        pipe_suppressed: Default::default(),
        launch,
        output: None,
        log_settings: None,
        _lifetime: None,
        _execution: None,
    });
    actor.publish(Phase::Ready, None);
    actor.restore_proxy_selections().await.unwrap();
    assert!(
        actor.selection_recovery.is_some(),
        "missing cold groups must remain pending"
    );
    loaded.store(true, Ordering::SeqCst);
    let epoch = actor.snapshot.monitor_epoch;
    let mut monitor = actor.monitor.subscribe();
    monitor.borrow_and_update();
    actor
        .proxy_action(Action::Select {
            group: "g".into(),
            node: "chosen".into(),
            generation: actor.snapshot.generation,
            epoch: actor.snapshot.monitor_epoch,
        })
        .await
        .unwrap();
    assert_eq!(actor.snapshot.monitor_epoch, epoch);
    assert!(
        !monitor.has_changed().unwrap(),
        "node selection must retain active streams"
    );
    assert!(monitor.borrow().is_some());
    actor.settle_proxy_selections().await.unwrap();
    assert_eq!(
        selected.lock().unwrap().get("g").map(String::as_str),
        Some("chosen")
    );
    assert_eq!(
        selected.lock().unwrap().get("other").map(String::as_str),
        Some("later")
    );
    assert!(actor.selection_recovery.is_none());
    loaded.store(false, Ordering::SeqCst);
    actor.restore_proxy_selections().await.unwrap();
    actor.selection_recovery.as_mut().unwrap().expires = std::time::Instant::now();
    assert!(
        actor
            .settle_proxy_selections()
            .await
            .unwrap_err()
            .contains("30秒")
    );
    let count = requests.load(Ordering::SeqCst);
    actor.settle_proxy_selections().await.unwrap();
    assert_eq!(requests.load(Ordering::SeqCst), count);
    actor.restore_proxy_selections().await.unwrap();
    actor.snapshot.generation += 1;
    let count = requests.load(Ordering::SeqCst);
    actor.settle_proxy_selections().await.unwrap();
    assert_eq!(requests.load(Ordering::SeqCst), count);
    actor.stop().await.unwrap();
    server.abort();
}

#[tokio::test]
async fn provider_sync_is_queued_by_runtime_revision_and_stops_after_four_pending_attempts() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let (_root, paths) = fixture(Some(SLEEPING_CORE));
    let mut launch = Launch::prepare(&paths).await.unwrap();
    launch.composed =
        "proxy-providers: {p: {type: http, url: 'https://example.test/p', path: p.yaml}}\n".into();
    let socket = paths.data().join("provider-sync.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    launch.client = super::super::client::Client::new(
        super::super::client::Endpoint::Unix(socket),
        String::new(),
    );
    let requests = Arc::new(AtomicUsize::new(0));
    let server = tokio::spawn({
        let requests = requests.clone();
        async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut bytes = [0; 4096];
                let n = stream.read(&mut bytes).await.unwrap();
                assert!(n > 0);
                requests.fetch_add(1, Ordering::SeqCst);
                let body = if bytes[..n].starts_with(b"GET /providers/proxies ") {
                    r#"{"providers":{"p":{"updatedAt":null}}}"#
                } else {
                    r#"{"providers":{}}"#
                };
                stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            }
        }
    });
    let mut actor = actor(paths);
    actor.running = Some(Running {
        child: Process::Local(
            tokio::process::Command::new("/bin/sleep")
                .arg("30")
                .spawn()
                .unwrap(),
        ),
        pipe_suppressed: Default::default(),
        launch,
        output: None,
        log_settings: None,
        _lifetime: None,
        _execution: None,
    });
    actor.publish(Phase::Ready, None);
    assert_eq!(actor.provider_sync.as_ref().unwrap().remaining, 4);
    for _ in 0..3 {
        actor.sync_provider_cache().await.unwrap();
        assert!(actor.provider_sync.is_some());
    }
    assert!(
        actor
            .sync_provider_cache()
            .await
            .unwrap_err()
            .contains("同步已停止")
    );
    let count = requests.load(Ordering::SeqCst);
    actor.publish(Phase::Ready, None);
    actor.sync_provider_cache().await.unwrap();
    assert_eq!(requests.load(Ordering::SeqCst), count);
    actor.invalidate_monitor();
    actor.publish(Phase::Ready, None);
    assert_eq!(actor.provider_sync.as_ref().unwrap().remaining, 4);
    actor.stop().await.unwrap();
    server.abort();
}

#[tokio::test]
async fn mode_and_system_proxy_disable_apply_auto_close_to_snapshot_connections() {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let (_root, paths) = fixture(Some(SLEEPING_CORE));
    let repository = Repository::open(paths.clone()).unwrap();
    let settings = crate::preferences::defaults(crate::preferences::SettingsKind::General)
        .into_iter()
        .chain([("AutoCloseConnection".into(), "true".into())])
        .collect();
    repository
        .save(
            &crate::storage::Namespace::parse("settings").unwrap(),
            &settings,
        )
        .unwrap();
    let previous = crate::preferences::defaults(crate::preferences::SettingsKind::SysProxy)
        .into_iter()
        .chain([("IsSystemProxyEnabled".into(), "true".into())])
        .collect();
    repository
        .save(
            &crate::storage::Namespace::parse("sysproxy").unwrap(),
            &previous,
        )
        .unwrap();
    let mut launch = Launch::prepare(&paths).await.unwrap();
    let socket = paths.data().join("auto-close.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    launch.client = super::super::client::Client::new(
        super::super::client::Endpoint::Unix(socket),
        String::new(),
    );
    let server = tokio::spawn(async move {
        let old = r#"{"uploadTotal":0,"downloadTotal":0,"connections":[{"id":"old","upload":0,"download":0,"metadata":{}}]}"#;
        let new = r#"{"uploadTotal":0,"downloadTotal":0,"connections":[{"id":"new","upload":0,"download":0,"metadata":{}}]}"#;
        for (prefix, body) in [
            ("GET /configs ", r#"{"mode":"rule"}"#),
            ("PATCH /configs ", "{}"),
            ("GET /configs ", r#"{"mode":"global"}"#),
            ("GET /connections ", old),
            ("DELETE /connections/old ", "{}"),
            ("GET /connections ", new),
            ("GET /connections ", old),
            ("DELETE /connections/old ", "{}"),
            ("GET /connections ", new),
        ] {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut chunk = [0; 4096];
                let n = stream.read(&mut chunk).await.unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&chunk[..n]);
                if let Some(end) = bytes.windows(4).position(|p| p == b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&bytes[..end + 4]);
                    let len = header
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if bytes.len() >= end + 4 + len {
                        break;
                    }
                }
            }
            assert!(String::from_utf8_lossy(&bytes).starts_with(prefix));
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        }
    });
    let mut actor = actor(paths);
    actor.running = Some(Running {
        child: Process::Local(
            tokio::process::Command::new("/bin/sleep")
                .arg("30")
                .spawn()
                .unwrap(),
        ),
        pipe_suppressed: Default::default(),
        launch,
        output: None,
        log_settings: None,
        _lifetime: None,
        _execution: None,
    });
    actor.publish(Phase::Ready, None);
    actor
        .proxy_action(crate::proxies::runtime::Action::Mode(
            crate::session::ProxyMode::Global,
        ))
        .await
        .unwrap();
    actor
        .save_system_proxy(
            &repository,
            &previous,
            crate::storage::Values::from([("IsSystemProxyEnabled".into(), "false".into())]),
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .unwrap()
        .unwrap();
    assert!(actor.snapshot.operation_committed());
    assert_eq!(
        actor.snapshot.preferences().unwrap().native()["mode"],
        "global"
    );
    actor.stop().await.unwrap();
}

#[tokio::test]
async fn partial_credentials_preserve_the_other_secret_and_committed_delete_remains_undoable() {
    use crate::profiles::{
        CredentialEdits, CredentialStore,
        model::{Document, DocumentKind},
        repository,
    };
    let (_root, paths) = fixture(None);
    let mut actor = actor(paths.clone());
    let vault = CredentialStore::new(&paths);
    vault.write(1, "saved-token", "saved-age").await.unwrap();
    let mut document = Document::new(1, DocumentKind::Profile);
    document.credentials = CredentialEdits::from_inputs("new-token", "");
    actor
        .profiles(super::super::ProfileCommand::Save {
            expected: repository::read(&paths).unwrap().0,
            documents: vec![document],
        })
        .await
        .unwrap();
    assert_eq!(
        vault.read(1).await.unwrap(),
        ("new-token".into(), "saved-age".into())
    );
    let (expected, mut documents) = repository::read(&paths).unwrap();
    documents[0].credentials.age = crate::profiles::SecretEdit::Clear;
    actor
        .profiles(super::super::ProfileCommand::Save {
            expected,
            documents,
        })
        .await
        .unwrap();
    assert_eq!(
        vault.read(1).await.unwrap(),
        ("new-token".into(), String::new())
    );
    vault.fail_next_delete(1);
    let error = actor
        .profiles(super::super::ProfileCommand::Save {
            expected: repository::read(&paths).unwrap().0,
            documents: vec![],
        })
        .await
        .unwrap_err();
    assert!(error.contains("凭据清理失败"));
    assert!(actor.snapshot.operation_committed());
    assert!(repository::read(&paths).unwrap().1.is_empty());
    assert!(actor.deleted_profiles.contains_key(&1));
    actor
        .profiles(super::super::ProfileCommand::Restore { ids: vec![1] })
        .await
        .unwrap();
    assert_eq!(repository::read(&paths).unwrap().1.len(), 1);
    assert_eq!(
        vault.read(1).await.unwrap(),
        ("new-token".into(), String::new())
    );
}

#[tokio::test]
async fn first_subscription_becomes_current_but_later_imports_do_not_replace_it() {
    use crate::profiles::{
        model::{Document, DocumentKind},
        repository,
    };
    let (_root, paths) = fixture(None);
    let mut actor = actor(paths.clone());
    for id in [1, 2] {
        let (expected, mut documents) = repository::read(&paths).unwrap();
        documents.push(Document::new(id, DocumentKind::Profile));
        actor
            .profiles(super::super::ProfileCommand::Save {
                expected,
                documents,
            })
            .await
            .unwrap();
        assert_eq!(
            crate::configuration::selected_profile(&paths).unwrap(),
            Some(1)
        );
    }
}

#[tokio::test]
async fn override_credentials_and_partial_delete_use_the_same_commit_contract() {
    use crate::profiles::{
        CredentialEdits, CredentialStore,
        model::{Document, DocumentKind},
        override_repository,
    };
    let (_root, paths) = fixture(None);
    let mut actor = actor(paths.clone());
    let vault = CredentialStore::overrides(&paths);
    vault.write(7, "saved-token", "saved-age").await.unwrap();
    let mut document = Document::new(7, DocumentKind::Yaml);
    document.credentials = CredentialEdits::from_inputs("", "new-age");
    actor
        .overrides(super::super::OverrideChange::Save {
            expected: override_repository::read(&paths).unwrap().0,
            documents: vec![document],
        })
        .await
        .unwrap();
    assert_eq!(
        vault.read(7).await.unwrap(),
        ("saved-token".into(), "new-age".into())
    );
    vault.fail_next_delete(7);
    let error = actor
        .overrides(super::super::OverrideChange::Save {
            expected: override_repository::read(&paths).unwrap().0,
            documents: vec![],
        })
        .await
        .unwrap_err();
    assert!(error.contains("凭据清理失败"));
    assert!(actor.snapshot.operation_committed());
    assert!(override_repository::read(&paths).unwrap().1.is_empty());
    actor
        .overrides(super::super::OverrideChange::Restore { ids: vec![7] })
        .await
        .unwrap();
    assert_eq!(override_repository::read(&paths).unwrap().1.len(), 1);
    assert_eq!(
        vault.read(7).await.unwrap(),
        ("saved-token".into(), "new-age".into())
    );
}

#[tokio::test]
async fn restored_core_is_ready_before_proxy_recovery_and_reports_its_independent_failure() {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let (_root, paths) = fixture(Some(SLEEPING_CORE));
    let repository = Repository::open(paths.clone()).unwrap();
    let mut proxy = crate::preferences::defaults(crate::preferences::SettingsKind::SysProxy);
    proxy.insert("IsSystemProxyEnabled".into(), "true".into());
    repository
        .save(
            &crate::storage::Namespace::parse("sysproxy").unwrap(),
            &proxy,
        )
        .unwrap();
    let launch = Launch::prepare(&paths).await.unwrap();
    let yaml: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&launch.yaml().await.unwrap()).unwrap();
    let socket = yaml["external-controller-unix"].as_str().unwrap();
    let listener = tokio::net::UnixListener::bind(socket).unwrap();
    let config = serde_json::to_string(&yaml).unwrap();
    let server = tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            let count = stream.read(&mut request).await.unwrap();
            let body = if request[..count].starts_with(b"GET /version ") {
                r#"{"version":"restored"}"#
            } else {
                &config
            };
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(reply.as_bytes()).await.unwrap();
        }
    });
    let mut actor = actor(paths);
    let error = actor.restore_running(launch).await.unwrap_err();
    assert_eq!(actor.snapshot.phase(), Phase::Ready);
    assert_eq!(actor.snapshot.version(), Some("restored"));
    assert!(error.contains("内核已就绪；系统代理未应用"), "{error}");
    assert!(error.contains("未启用 HTTP 或混合代理端口"), "{error}");
    actor.stop().await.unwrap();
    server.abort();
}

#[tokio::test]
async fn preparing_exit_keeps_the_executor_available_until_final_shutdown() {
    let (_root, paths) = fixture(None);
    let mut service = Service::start(paths).unwrap();
    let events = service.events();
    let reply = service.prepare_exit().unwrap();
    tokio::time::timeout(Duration::from_secs(5), reply.recv())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    // The acknowledgement is sent before the request reservation is released.
    let id = loop {
        match service.submit(Command::Start) {
            Ok(id) => break id,
            Err(super::ServiceError::Busy) => tokio::task::yield_now().await,
            Err(error) => panic!("executor destroyed before final exit: {error}"),
        }
    };
    let snapshot = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let snapshot = events.recv().await.unwrap();
            if snapshot.request_id == Some(id) && !snapshot.operation_active {
                break snapshot;
            }
        }
    })
    .await
    .unwrap();
    assert!(snapshot.error().unwrap().contains("未安装"));
    assert!(
        snapshot.error_logged,
        "the UI must not log the same command failure again"
    );
    let worker = service.shutdown().unwrap();
    tokio::task::spawn_blocking(move || worker.join().unwrap().unwrap())
        .await
        .unwrap();
}

#[tokio::test]
async fn exit_preparation_retains_failed_recovery_for_a_second_attempt() {
    let (_root, paths) = fixture(Some(SLEEPING_CORE));
    let mut actor = actor(paths.clone());
    actor.recovery = Some(super::super::install::Recovery::begin(&paths).unwrap());
    let backup = paths
        .directory(DirectoryKind::Core)
        .join(".core-update-recovery/binary");
    let bytes = std::fs::read(&backup).unwrap();
    std::fs::write(&backup, b"damaged").unwrap();
    assert!(actor.prepare_exit().await.is_err());
    assert!(actor.recovery.is_some());
    assert!(!actor.exit_prepared);
    std::fs::write(&backup, bytes).unwrap();
    actor.prepare_exit().await.unwrap();
    assert!(actor.recovery.is_none());
    assert!(actor.exit_prepared);
}

#[tokio::test]
async fn owned_health_recovery_is_observed_and_stops_after_its_budget() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let (_root, paths) = fixture(Some(SLEEPING_CORE));
    let socket = paths.data().join("health-test.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let healthy = Arc::new(AtomicBool::new(false));
    let requests = Arc::new(AtomicUsize::new(0));
    let server = tokio::spawn({
        let healthy = healthy.clone();
        let requests = requests.clone();
        async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = [0; 4096];
                let count = stream.read(&mut request).await.unwrap();
                requests.fetch_add(1, Ordering::SeqCst);
                let (status, body) = if !healthy.load(Ordering::SeqCst) {
                    (503, "{}")
                } else if request[..count].starts_with(b"GET /version ") {
                    (200, r#"{"version":"recovered"}"#)
                } else {
                    (200, r#"{"tun":{"enable":false}}"#)
                };
                let reply = format!(
                    "HTTP/1.1 {status} test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(reply.as_bytes()).await.unwrap();
            }
        }
    });
    let mut launch = Launch::prepare(&paths).await.unwrap();
    launch.client = super::super::client::Client::new(
        super::super::client::Endpoint::Unix(socket),
        String::new(),
    );
    let child = tokio::process::Command::new("/bin/sleep")
        .arg("30")
        .spawn()
        .unwrap();
    let mut actor = actor(paths);
    actor.running = Some(Running {
        child: Process::Local(child),
        pipe_suppressed: Default::default(),
        launch,
        output: None,
        log_settings: None,
        _lifetime: None,
        _execution: None,
    });
    actor.publish(Phase::Ready, None);
    actor.observe_health().await;
    assert_eq!(actor.snapshot.phase(), Phase::Failed);
    assert_eq!(actor.health_recovery_remaining, 15);
    healthy.store(true, Ordering::SeqCst);
    actor.observe_health().await;
    assert_eq!(actor.snapshot.phase(), Phase::Ready);
    assert_eq!(actor.snapshot.version(), Some("recovered"));
    assert_eq!(actor.health_recovery_remaining, 0);
    healthy.store(false, Ordering::SeqCst);
    for _ in 0..16 {
        actor.observe_health().await;
    }
    assert_eq!(actor.health_recovery_remaining, 0);
    let count = requests.load(Ordering::SeqCst);
    actor.observe_health().await;
    assert_eq!(requests.load(Ordering::SeqCst), count);
    assert!(actor.snapshot.error().unwrap().contains("恢复观察已停止"));
    actor.stop().await.unwrap();
    server.abort();
}

#[tokio::test]
async fn malformed_json_does_not_close_executor_and_corrected_files_are_reread() {
    let (_root, paths) = fixture(None);
    let repository = Repository::open(paths.clone()).unwrap();
    crate::preferences::initialize(&repository).unwrap();
    let broken = paths.settings().join("mihomo.yaml");
    let original = std::fs::read(&broken).unwrap();
    std::fs::write(&broken, b"{broken").unwrap();
    let mut service = Service::start(paths.clone()).unwrap();
    let events = service.events();
    for repair in [false, true] {
        if repair {
            std::fs::write(&broken, &original).unwrap();
        }
        let id = service.submit(Command::Start).unwrap();
        let failure = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let snapshot = events.recv().await.unwrap();
                if snapshot.request_id == Some(id) && !snapshot.operation_active {
                    break snapshot;
                }
            }
        })
        .await
        .unwrap();
        let error = failure.error().unwrap();
        if repair {
            assert!(error.contains("未安装"), "{error}");
        } else {
            assert!(error.contains("mihomo.yaml"), "{error}");
            assert_eq!(std::fs::read(&broken).unwrap(), b"{broken");
        }
        // The completion snapshot can arrive before its reservation is released.
        tokio::task::yield_now().await;
    }
    let worker = service.shutdown().unwrap();
    tokio::task::spawn_blocking(move || worker.join().unwrap().unwrap())
        .await
        .unwrap();
}

#[tokio::test]
async fn autostart_status_queries_do_not_reserve_the_mutation_lane() {
    let (_root, paths) = fixture(None);
    let mut service = Service::start(paths).unwrap();
    let events = service.events();
    tokio::time::timeout(Duration::from_secs(5), async {
        while events.recv().await.unwrap().phase == Phase::Preparing {}
    })
    .await
    .unwrap();
    for _ in 0..3 {
        service.refresh_autostart();
    }
    assert!(!service.busy.load(std::sync::atomic::Ordering::Acquire));
    let id = service
        .submit(Command::Settings {
            kind: crate::preferences::SettingsKind::General,
            patch: crate::storage::Values::from([("SilentStart".into(), "true".into())]),
        })
        .unwrap();
    let snapshot = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let snapshot = events.recv().await.unwrap();
            if snapshot.request_id == Some(id) && !snapshot.operation_active {
                break snapshot;
            }
        }
    })
    .await
    .unwrap();
    assert!(snapshot.error().is_none(), "{:?}", snapshot.error());
    assert_eq!(
        snapshot.settings("settings").unwrap()["SilentStart"],
        "true"
    );
    let worker = service.shutdown().unwrap();
    tokio::task::spawn_blocking(move || worker.join().unwrap().unwrap())
        .await
        .unwrap();
}

#[tokio::test]
async fn commands_reject_conflicts_and_shutdown_closes_the_owner() {
    let (_root, paths) = fixture(None);
    let mut service = Service::start(paths).unwrap();
    let events = service.events();
    assert_eq!(service.submit(Command::Start).unwrap(), 1);
    assert!(matches!(
        service.submit(Command::Start),
        Err(crate::core::ServiceError::Busy)
    ));
    let failure = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let snapshot = events.recv().await.unwrap();
            if snapshot.phase() == Phase::Failed && snapshot.request_id == Some(1) {
                break snapshot;
            }
        }
    })
    .await
    .unwrap();
    assert!(failure.error().unwrap().contains("未安装"));
    assert!(!failure.has_process());
    let worker = service.shutdown().unwrap();
    tokio::task::spawn_blocking(move || worker.join().unwrap().unwrap())
        .await
        .unwrap();
    assert!(matches!(
        service.submit(Command::Start),
        Err(crate::core::ServiceError::Closed)
    ));
}

#[tokio::test]
async fn startup_configuration_can_settle_after_three_seconds_and_keeps_timeout_cause() {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    for settles in [true, false] {
        let (_root, paths) = fixture(Some(SLEEPING_CORE));
        let socket = paths.data().join("slow-startup.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        let started = tokio::time::Instant::now();
        let server = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = [0; 4096];
                let count = stream.read(&mut request).await.unwrap();
                let body = if request[..count].starts_with(b"GET /version ") {
                    r#"{"version":"slow-startup"}"#.to_owned()
                } else {
                    serde_json::json!({"tun":{"enable": settles && started.elapsed() > Duration::from_millis(3300)}}).to_string()
                };
                stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            }
        });
        let mut launch = Launch::prepare(&paths).await.unwrap();
        launch.composed = "tun: {enable: true}".into();
        launch.client = super::super::client::Client::new(
            super::super::client::Endpoint::Unix(socket),
            String::new(),
        );
        let child = tokio::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let mut owner = actor(paths);
        owner.running = Some(Running {
            child: Process::Local(child),
            launch,
            pipe_suppressed: Default::default(),
            output: None,
            log_settings: None,
            _lifetime: None,
            _execution: None,
        });
        let result = owner
            .wait_ready(Duration::from_secs(if settles { 5 } else { 1 }))
            .await;
        if settles {
            assert_eq!(result.unwrap(), "slow-startup");
            assert_eq!(owner.snapshot.tun(), Some(true));
        } else {
            let error = result.unwrap_err();
            assert!(error.contains("1 秒内就绪"), "{error}");
            assert!(error.contains("期望 true，实际 false"), "{error}");
        }
        owner.stop().await.unwrap();
        server.abort();
    }
}

#[tokio::test]
async fn automatic_refresh_does_not_erase_startup_failure() {
    let (_root, paths) = fixture(None);
    let mut service = Service::start(paths).unwrap();
    let events = service.events();
    let mut latest = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let snapshot = events.recv().await.unwrap();
            if snapshot.phase() == Phase::Failed {
                break snapshot;
            }
        }
    })
    .await
    .unwrap();
    // The schedule interval ticks immediately after startup; let that refresh complete.
    tokio::time::sleep(Duration::from_millis(300)).await;
    while let Ok(snapshot) = events.try_recv() {
        latest = snapshot;
    }
    assert_eq!(latest.phase(), Phase::Failed);
    assert!(latest.error().unwrap().contains("未安装"));
    let worker = service.shutdown().unwrap();
    tokio::task::spawn_blocking(move || worker.join().unwrap().unwrap())
        .await
        .unwrap();
}

#[tokio::test]
async fn a_live_process_is_not_ready_and_cancelled_start_remains_owned_until_reaped() {
    let (_root, paths) = fixture(Some(SLEEPING_CORE));
    use futures_util::{SinkExt as _, StreamExt as _};
    use tokio_tungstenite::tungstenite::Message;
    let socket = paths.data().join("starting-logs.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let server = tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            // Version probes are intentionally unavailable while the log endpoint is connectable.
            let Ok(mut ws) = tokio_tungstenite::accept_async(stream).await else {
                continue;
            };
            ws.send(Message::Text(
                r#"{"type":"error","payload":"startup adapter failure"}"#.into(),
            ))
            .await
            .unwrap();
            while let Some(message) = ws.next().await {
                if matches!(message, Ok(Message::Close(_)) | Err(_)) {
                    break;
                }
            }
            break;
        }
    });
    let mut launch = Launch::prepare(&paths).await.unwrap();
    launch.client = super::super::client::Client::new(
        super::super::client::Endpoint::Unix(socket),
        String::new(),
    );
    let mut actor = actor(paths.clone());
    let mut log_target = actor.log_target.subscribe();
    let (shutdown, cancellation) = tokio::sync::watch::channel(false);
    let logging = tokio::spawn(super::super::log_stream::run(
        paths.clone(),
        actor.logs.clone(),
        log_target.clone(),
        cancellation,
    ));
    assert!(
        tokio::time::timeout(Duration::from_millis(120), actor.start_prepared(launch))
            .await
            .is_err()
    );
    assert_eq!(actor.snapshot.phase(), Phase::Starting);
    assert!(actor.running.is_some());
    assert!(
        actor.monitor.borrow().is_none(),
        "runtime streams must wait for readiness"
    );
    assert!(
        log_target.borrow_and_update().is_some(),
        "startup logs must already be collected"
    );
    tokio::time::timeout(Duration::from_secs(2), async {
        while !actor.logs.since(0).iter().any(|record| {
            record.level == crate::logging::Level::Error
                && record.message == "startup adapter failure"
        }) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    actor.publish(Phase::Ready, None);
    assert!(actor.monitor.borrow().is_some());
    assert!(
        !log_target.has_changed().unwrap(),
        "readiness must retain the same log connection"
    );
    actor.stop().await.unwrap();
    assert_eq!(actor.snapshot.phase(), Phase::Stopped);
    assert!(actor.running.is_none());
    assert!(actor.monitor.borrow().is_none());
    assert!(
        actor.log_target.borrow().is_none(),
        "stopped generations must drop the log target"
    );
    shutdown.send_replace(true);
    logging.await.unwrap();
    server.await.unwrap();
    assert!(
        std::fs::read_dir(paths.directory(DirectoryKind::Core))
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("run-"))
    );
    let saved = std::fs::read_dir(paths.directory(DirectoryKind::Logs))
        .unwrap()
        .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
        .collect::<String>();
    assert!(saved.contains("startup adapter failure"));
}

#[tokio::test]
async fn failed_validation_preserves_the_old_process_and_source() {
    use std::os::unix::fs::PermissionsExt as _;
    let (_root, paths) = fixture(Some(SLEEPING_CORE));
    let launch = Launch::prepare(&paths).await.unwrap();
    let mut child = launch
        .command(false)
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let pid = child.id();
    tokio::time::timeout(Duration::from_secs(2), async {
        while !paths
            .directory(DirectoryKind::Core)
            .join("child-started")
            .exists()
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    if let Some(status) = child.try_wait().unwrap() {
        use tokio::io::AsyncReadExt as _;
        let mut output = String::new();
        child
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut output)
            .await
            .unwrap();
        panic!("fixture exited before restart: {status}; {output}");
    }
    let mut actor = actor(paths.clone());
    actor.running = Some(Running {
        child: Process::Local(child),
        launch,
        output: None,
        log_settings: None,
        pipe_suppressed: Default::default(),
        _lifetime: None,
        _execution: None,
    });
    actor.snapshot.phase = Phase::Ready;
    let binary = paths.directory(DirectoryKind::Core).join("mihomo");
    crate::storage::atomic_write(&binary, b"#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let source = paths
        .directory(DirectoryKind::Config)
        .join("runtime-source.yaml");
    let before = std::fs::read(&source).unwrap();
    assert!(actor.restart().await.unwrap_err().contains("拒绝"));
    assert_eq!(actor.running.as_ref().unwrap().child.id(), pid);
    let status = actor.running.as_mut().unwrap().child.try_wait().unwrap();
    assert!(status.is_none(), "old process exited: {status:?}");
    assert_eq!(std::fs::read(source).unwrap(), before);
    actor.stop().await.unwrap();
}

#[tokio::test]
async fn shutdown_cancels_and_reaps_an_in_flight_validator() {
    let (_root, paths) = fixture(Some("#!/bin/sh\nexec /bin/sleep 30\n"));
    let mut service = Service::start(paths.clone()).unwrap();
    service.submit(Command::Start).unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if std::fs::read_dir(paths.directory(DirectoryKind::Core))
                .unwrap()
                .any(|entry| {
                    entry
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .starts_with("run-")
                })
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let worker = service.shutdown().unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        tokio::task::spawn_blocking(move || worker.join().unwrap().unwrap()),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        std::fs::read_dir(paths.directory(DirectoryKind::Core))
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("run-"))
    );
}

#[tokio::test]
async fn unexpected_child_exit_invalidates_readiness_and_version() {
    let (_root, paths) = fixture(Some("#!/bin/sh\nexit 7\n"));
    let launch = Launch::prepare(&paths).await.unwrap();
    let child = launch.command(false).spawn().unwrap();
    let mut actor = actor(paths);
    actor.running = Some(Running {
        child: Process::Local(child),
        launch,
        output: None,
        log_settings: None,
        pipe_suppressed: Default::default(),
        _lifetime: None,
        _execution: None,
    });
    actor.snapshot.phase = Phase::Ready;
    actor.snapshot.version = Some("test-version".into());
    tokio::time::timeout(Duration::from_secs(2), async {
        while actor.running.is_some() {
            actor.observe_exit().await;
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(actor.snapshot.phase(), Phase::Failed);
    assert!(actor.snapshot.version().is_none());
    assert!(actor.snapshot.error().unwrap().contains("意外退出"));
}

#[tokio::test]
#[ignore = "requires an explicitly supplied official Mihomo binary; proxy ports stay disabled"]
async fn real_mihomo_validates_starts_restarts_and_stops_over_private_ipc() {
    use std::os::unix::fs::PermissionsExt as _;
    let binary = std::env::var_os("SUKI_TEST_MIHOMO").expect("set SUKI_TEST_MIHOMO");
    let (_root, paths) = fixture(None);
    let installed = paths.directory(DirectoryKind::Core).join("mihomo");
    std::fs::copy(Path::new(&binary), &installed).unwrap();
    std::fs::set_permissions(installed, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut actor = actor(paths.clone());
    actor.start().await.unwrap();
    assert_eq!(actor.snapshot.phase(), Phase::Ready);
    assert!(actor.snapshot.version().unwrap().starts_with('v'));
    save_override(&paths, "YAML", "mode: global\nlog-level: silent");
    let generation = actor.snapshot.generation();
    actor.restart().await.unwrap();
    assert_eq!(actor.snapshot.phase(), Phase::Ready);
    assert!(actor.snapshot.generation() > generation);
    let candidate: serde_yaml_ng::Value = serde_yaml_ng::from_slice(
        &std::fs::read(actor.running.as_ref().unwrap().launch.candidate_path()).unwrap(),
    )
    .unwrap();
    assert_eq!(candidate["mode"], "global");
    let pid = actor.running.as_ref().unwrap().child.id();
    save_override(&paths, "YAML", "proxies: [{type: definitely-invalid}]");
    assert!(actor.restart().await.unwrap_err().contains("拒绝"));
    assert_eq!(actor.snapshot.phase(), Phase::Ready);
    assert_eq!(actor.running.as_ref().unwrap().child.id(), pid);
    save_override(&paths, "YAML", "mode: global\nlog-level: silent");
    // A validated replacement can still fail at process launch/readiness. Make
    // only its first run fail, then exercise real recovery of the old config.
    let installed = paths.directory(DirectoryKind::Core).join("mihomo");
    crate::storage::atomic_write(
        &installed,
        br#"#!/bin/sh
for arg do
if [ "$arg" = -t ]; then exec "$SUKI_TEST_MIHOMO" "$@"; fi
done
if [ ! -f replacement-failed ]; then
: > replacement-failed
exit 7
fi
exec "$SUKI_TEST_MIHOMO" "$@"
"#,
    )
    .unwrap();
    std::fs::set_permissions(installed, std::fs::Permissions::from_mode(0o700)).unwrap();
    let generation = actor.snapshot.generation();
    let error = actor.restart().await.unwrap_err();
    assert!(error.contains("恢复就绪"), "{error}");
    assert_eq!(actor.snapshot.phase(), Phase::Ready);
    assert_eq!(actor.snapshot.generation(), generation + 2);
    let pid = actor.running.as_ref().unwrap().child.id();
    std::fs::write(
        paths
            .directory(DirectoryKind::Config)
            .join("runtime-source.yaml"),
        "proxies: [{type: definitely-invalid}]\n",
    )
    .unwrap();
    assert!(actor.restart().await.is_err());
    assert_eq!(actor.snapshot.phase(), Phase::Ready);
    assert_eq!(actor.running.as_ref().unwrap().child.id(), pid);
    actor.stop().await.unwrap();
    assert!(actor.running.is_none());
    assert_eq!(actor.snapshot.phase(), Phase::Stopped);
}

fn save_override(paths: &AppPaths, kind: &str, content: &str) {
    let directory = paths.data().join("overrides");
    std::fs::create_dir_all(&directory).unwrap();
    let extension = if kind == "JavaScript" { "js" } else { "yaml" };
    std::fs::write(directory.join(format!("42.{extension}")), content).unwrap();
    let store = Repository::open(paths.clone()).unwrap();
    store
        .save(
            &crate::storage::Namespace::parse("override-index").unwrap(),
            &crate::storage::Values::from([
                ("format".into(), "1".into()),
                ("ids".into(), "42".into()),
                ("42/kind".into(), kind.into()),
                ("42/name".into(), "Test override".into()),
                ("42/enabled".into(), "true".into()),
                ("42/scope-profile".into(), "".into()),
            ]),
        )
        .unwrap();
}

#[tokio::test]
async fn persisted_overrides_feed_private_candidate_without_mutating_source() {
    let (_root, paths) = fixture(Some(SLEEPING_CORE));
    let source = paths
        .directory(DirectoryKind::Config)
        .join("runtime-source.yaml");
    let before = std::fs::read(&source).unwrap();
    save_override(
        &paths,
        "YAML",
        "mode: global\ncustom: {MyName: retained}\nexternal-controller: 0.0.0.0:9090\nexternal-controller-unix: /tmp/untrusted.sock\nsecret: untrusted-secret\nallow-lan: true\nbind-address: '*'",
    );
    let launch = Launch::prepare(&paths).await.unwrap();
    let candidate = std::fs::read_to_string(launch.candidate_path()).unwrap();
    let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(&candidate).unwrap();
    assert_eq!(value["mode"], "global");
    assert_eq!(value["custom"]["MyName"], "retained");
    assert!(value.get("external-controller").is_none());
    assert!(!candidate.contains("untrusted"));
    assert_eq!(value["allow-lan"], false);
    assert_eq!(value["bind-address"], "*");
    assert_eq!(std::fs::read(source).unwrap(), before);
}

#[tokio::test]
async fn composition_failures_preserve_running_child_and_its_candidate() {
    let (_root, paths) = fixture(Some(SLEEPING_CORE));
    let launch = Launch::prepare(&paths).await.unwrap();
    let old_candidate = launch.candidate_path().to_owned();
    let before = std::fs::read(&old_candidate).unwrap();
    let child = launch.command(false).spawn().unwrap();
    let pid = child.id();
    let mut actor = actor(paths.clone());
    actor.running = Some(Running {
        child: Process::Local(child),
        launch,
        output: None,
        log_settings: None,
        pipe_suppressed: Default::default(),
        _lifetime: None,
        _execution: None,
    });
    actor.snapshot.phase = Phase::Ready;
    for (kind, content, message) in [
        ("YAML", "mode: [invalid", "YAML"),
        ("JavaScript", "function main(c) { throw 1; }", "JavaScript"),
    ] {
        save_override(&paths, kind, content);
        assert!(actor.restart().await.unwrap_err().contains(message));
        assert_eq!(actor.running.as_ref().unwrap().child.id(), pid);
        assert!(
            actor
                .running
                .as_mut()
                .unwrap()
                .child
                .try_wait()
                .unwrap()
                .is_none()
        );
        assert_eq!(actor.snapshot.phase(), Phase::Ready);
        assert_eq!(std::fs::read(&old_candidate).unwrap(), before);
    }
    actor.stop().await.unwrap();
}

fn save_profile(paths: &AppPaths, id: u64, content: &str) {
    std::fs::create_dir_all(paths.data().join("profiles")).unwrap();
    std::fs::write(crate::profiles::repository::body_path(paths, id), content).unwrap();
    let store = Repository::open(paths.clone()).unwrap();
    let namespace = crate::storage::Namespace::parse("profile-index").unwrap();
    let mut values = store.read(&namespace).unwrap();
    let ids = values.entry("ids".into()).or_default();
    if !ids.split(',').any(|value| value == id.to_string()) {
        if !ids.is_empty() {
            ids.push(',');
        }
        ids.push_str(&id.to_string());
    }
    for (key, value) in [
        ("format".into(), "1".into()),
        (format!("{id}/kind"), "配置".into()),
        (format!("{id}/name"), format!("Local {id}")),
    ] {
        values.insert(key, value);
    }
    store.save(&namespace, &values).unwrap();
}

#[tokio::test]
#[ignore = "requires explicit SUKI_TEST_MIHOMO; no proxy ports or system network changes"]
async fn real_profile_activation_apply_persistence_and_failed_commit_recovery() {
    use std::os::unix::fs::PermissionsExt as _;
    let binary = std::env::var_os("SUKI_TEST_MIHOMO").expect("set SUKI_TEST_MIHOMO");
    let (_root, paths) = fixture(None);
    let installed = paths.directory(DirectoryKind::Core).join("mihomo");
    std::fs::copy(binary, &installed).unwrap();
    std::fs::set_permissions(installed, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::remove_file(
        paths
            .directory(DirectoryKind::Config)
            .join("runtime-source.yaml"),
    )
    .unwrap();
    let source = "mixed-port: 0\nport: 0\nsocks-port: 0\nlog-level: silent\nmode: rule\nrules: ['MATCH,DIRECT']";
    save_profile(&paths, 7, source);
    let mut owner = actor(paths.clone());
    owner.activate(7).await.unwrap();
    assert_eq!(owner.snapshot.profile().unwrap().id(), 7);
    let ready_pid = owner.running.as_ref().unwrap().child.id();
    owner.start().await.unwrap();
    assert_eq!(owner.running.as_ref().unwrap().child.id(), ready_pid);
    let applied = paths.settings().join("settings.json");
    let first_commit = std::fs::read(&applied).unwrap();
    save_override(&paths, "YAML", "mode: direct");
    owner.apply().await.unwrap();
    assert_eq!(std::fs::read(&applied).unwrap(), first_commit);
    assert_eq!(owner.snapshot.profile().unwrap().id(), 7);
    let candidate: serde_yaml_ng::Value = serde_yaml_ng::from_slice(
        &std::fs::read(owner.running.as_ref().unwrap().launch.candidate_path()).unwrap(),
    )
    .unwrap();
    assert_eq!(candidate["mode"], "direct");
    let before = std::fs::read(&applied).unwrap();
    let pid = owner.running.as_ref().unwrap().child.id();
    save_profile(&paths, 8, "proxies: [{type: invalid-proxy-type}]");
    assert!(owner.activate(8).await.unwrap_err().contains("拒绝"));
    assert_eq!(owner.running.as_ref().unwrap().child.id(), pid);
    assert_eq!(owner.snapshot.profile().unwrap().id(), 7);
    assert_eq!(std::fs::read(&applied).unwrap(), before);
    // Deterministic persistence failure after the new process is ready. The
    // external collision is removed by the test; the app must recover the old
    // runtime itself and must not publish profile 8 as activated.
    save_profile(&paths, 8, source);
    std::fs::remove_file(&applied).unwrap();
    std::fs::create_dir(&applied).unwrap();
    assert!(owner.activate(8).await.unwrap_err().contains("恢复就绪"));
    assert_eq!(owner.snapshot.phase(), Phase::Ready);
    assert_eq!(owner.snapshot.profile().unwrap().id(), 7);
    std::fs::remove_dir(&applied).unwrap();
    std::fs::write(&applied, before).unwrap();
    owner.stop().await.unwrap();
    assert!(owner.snapshot.profile().is_none());
    let mut reopened = actor(paths.clone());
    reopened.start().await.unwrap();
    assert_eq!(reopened.snapshot.profile().unwrap().id(), 7);
    reopened.stop().await.unwrap();
}

#[tokio::test]
#[ignore = "requires explicit SUKI_TEST_BUNDLE with verified real runtime; no proxy ports"]
async fn real_offline_bundle_prepares_first_open_and_loads_geographic_rules() {
    let bundle = std::env::var_os("SUKI_TEST_BUNDLE").expect("set SUKI_TEST_BUNDLE");
    let (_root, paths) = fixture(None);
    crate::bootstrap::prepare(&paths, Path::new(&bundle), || false).unwrap();
    // Test both dat-based country/domain data and the mmdb country loader.
    for mode in [true, false] {
        let source = format!(
            "mixed-port: 0\nport: 0\nsocks-port: 0\nlog-level: silent\ngeodata-mode: {mode}\nrules: ['GEOSITE,cn,DIRECT', 'GEOIP,CN,DIRECT', 'MATCH,DIRECT']\n"
        );
        save_profile(&paths, 1, &source);
        let mut owner = actor(paths.clone());
        owner.activate(1).await.unwrap();
        assert_eq!(owner.snapshot.phase(), Phase::Ready);
        assert_eq!(owner.snapshot.profile().unwrap().id(), 1);
        owner.stop().await.unwrap();
    }
    // Subsequent open is offline and does not need its original install bundle.
    crate::bootstrap::prepare(&paths, Path::new("/nonexistent/install-bundle"), || false).unwrap();
}

fn core_settings(paths: &AppPaths, port: u16) {
    crate::configuration::Preferences::from_values(
        &crate::storage::Values::from([
            ("MixedPort".into(), port.to_string()),
            ("LogLevelIndex".into(), "静默（silent）".into()),
        ]),
        None,
    )
    .unwrap()
    .save(paths)
    .unwrap();
}

#[tokio::test]
#[ignore = "requires explicit SUKI_TEST_MIHOMO; only isolated loopback listeners"]
async fn real_startup_keeps_requested_port_and_invalid_configuration() {
    use std::os::unix::fs::PermissionsExt as _;
    let (_root, paths) = fixture(None);
    let binary = paths.directory(DirectoryKind::Core).join("mihomo");
    std::fs::copy(std::env::var_os("SUKI_TEST_MIHOMO").unwrap(), &binary).unwrap();
    std::fs::set_permissions(binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    core_settings(&paths, occupied.local_addr().unwrap().port());
    let [native, app] = crate::configuration::Preferences::paths(&paths);
    let before = [
        std::fs::read(&native).unwrap(),
        std::fs::read(&app).unwrap(),
    ];
    let mut owner = actor(paths.clone());
    assert!(owner.startup().await.is_err());
    assert!(owner.running.is_none());
    assert_eq!(std::fs::read(&native).unwrap(), before[0]);
    assert_eq!(std::fs::read(&app).unwrap(), before[1]);
    core_settings(&paths, 0);
    save_profile(&paths, 7, "rules: ['MATCH,DIRECT']");
    owner.activate(7).await.unwrap();
    owner.stop().await.unwrap();
    let applied = paths.settings().join("settings.json");
    let before = std::fs::read(&applied).unwrap();
    save_override(&paths, "YAML", "rules: [invalid");
    assert!(owner.startup().await.is_err());
    assert!(owner.running.is_none());
    assert_eq!(std::fs::read(&applied).unwrap(), before);
    save_override(&paths, "YAML", "{}");
    owner.startup().await.unwrap();
    assert_eq!(owner.snapshot.profile().unwrap().id(), 7);
    owner.stop().await.unwrap();
}

#[tokio::test]
#[ignore = "requires explicit SUKI_TEST_BUNDLE; no proxy ports or system network changes"]
async fn real_first_open_service_automatically_starts_base_runtime() {
    let bundle = std::env::var_os("SUKI_TEST_BUNDLE").expect("set SUKI_TEST_BUNDLE");
    let (_root, paths) = fixture(None);
    std::fs::remove_file(
        paths
            .directory(DirectoryKind::Config)
            .join("runtime-source.yaml"),
    )
    .unwrap();
    crate::bootstrap::prepare(&paths, Path::new(&bundle), || false).unwrap();
    core_settings(&paths, 0);
    let mut service = Service::start(paths).unwrap();
    let events = service.events();
    let ready = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let snapshot = events.recv().await.unwrap();
            if snapshot.phase() == Phase::Ready {
                break snapshot;
            }
            assert_ne!(snapshot.phase(), Phase::Failed, "{:?}", snapshot.error());
        }
    })
    .await
    .unwrap();
    assert!(ready.has_process());
    assert!(ready.profile().is_none());
    let pid = ready.pid.unwrap();
    let worker = service.shutdown().unwrap();
    tokio::task::spawn_blocking(move || worker.join().unwrap().unwrap())
        .await
        .unwrap();
    assert!(
        !std::process::Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success()
    );
}

#[tokio::test]
#[ignore = "requires explicit SUKI_TEST_MIHOMO and vendor network; isolated core and loopback only"]
async fn real_core_page_settings_smart_switch_and_owned_shutdown() {
    use std::os::unix::fs::PermissionsExt as _;
    let binary = std::env::var_os("SUKI_TEST_MIHOMO").expect("set SUKI_TEST_MIHOMO");
    let (_root, paths) = fixture(None);
    let managed = paths.directory(DirectoryKind::Core).join("mihomo");
    std::fs::copy(binary, &managed).unwrap();
    std::fs::set_permissions(&managed, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut owner = actor(paths.clone());
    owner.start().await.unwrap();
    owner.validate_current().await.unwrap();
    owner.flush_fake_ip().await.unwrap();
    let controller_port = {
        let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        socket.local_addr().unwrap().port()
    };
    let mut values = crate::core::Preferences::load(&paths).unwrap().values();
    values.insert("MixedPort".into(), "0".into());
    values.insert("HttpPort".into(), "0".into());
    values.insert("SocksPort".into(), "0".into());
    values.insert("EnableExternalController".into(), "true".into());
    values.insert(
        "ExternalController".into(),
        format!("127.0.0.1:{controller_port}"),
    );
    values.insert("Secret".into(), "core-page-test-secret".into());
    values.insert("FindProcessIndex".into(), "严格（strict）".into());
    owner
        .configure(crate::core::Preferences::from_values(&values, None).unwrap())
        .await
        .unwrap();
    assert_eq!(owner.snapshot.phase(), Phase::Ready);
    assert!(
        tokio::net::TcpStream::connect(("127.0.0.1", controller_port))
            .await
            .is_ok()
    );
    assert_eq!(
        crate::core::Preferences::load(&paths).unwrap().values()["Secret"],
        "core-page-test-secret"
    );
    // Verify HTTP token enforcement while private IPC continues to work.
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", controller_port))
        .await
        .unwrap();
    stream
        .write_all(b"GET /version HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    assert!(String::from_utf8_lossy(&response).starts_with("HTTP/1.1 401"));
    owner
        .install(crate::core::ReleaseSelection::Smart)
        .await
        .unwrap();
    assert!(owner.snapshot.version().unwrap().contains("smart"));
    assert_eq!(
        crate::core::Preferences::load(&paths).unwrap().values()["CoreReleaseIndex"],
        "Smart"
    );
    owner.validate_current().await.unwrap();
    owner.flush_fake_ip().await.unwrap();
    let pid = owner.running.as_ref().unwrap().child.id().unwrap();
    owner.stop().await.unwrap();
    assert!(
        !std::process::Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success()
    );
    assert!(
        tokio::net::TcpStream::connect(("127.0.0.1", controller_port))
            .await
            .is_err()
    );
    assert!(
        !paths
            .directory(DirectoryKind::Core)
            .join(".core-update-recovery")
            .exists()
    );
}

#[tokio::test]
async fn two_services_cannot_own_the_same_core_directory() {
    let (_root, paths) = fixture(None);
    let mut first = Service::start(paths.clone()).unwrap();
    let first_events = first.events();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let snapshot = first_events.recv().await.unwrap();
            if !snapshot.is_busy() {
                break;
            }
        }
    })
    .await
    .unwrap();
    let mut second = Service::start(paths).unwrap();
    let error = tokio::time::timeout(Duration::from_secs(3), second.events().recv())
        .await
        .unwrap()
        .unwrap();
    assert!(error.error().unwrap().contains("另一个"));
    assert!(!error.has_process());
    let one = first.shutdown().unwrap();
    let two = second.shutdown().unwrap();
    tokio::task::spawn_blocking(move || {
        one.join().unwrap().unwrap();
        two.join().unwrap().unwrap();
    })
    .await
    .unwrap();
}

#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
#[ignore = "requires explicit SUKI_TEST_MIHOMO; isolated loopback TCP/UDP only"]
async fn real_hot_settings_tunnels_cors_and_failure_recovery() {
    use std::os::unix::fs::PermissionsExt as _;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let (_root, paths) = fixture(None);
    let installed = paths.directory(DirectoryKind::Core).join("mihomo");
    std::fs::copy(std::env::var_os("SUKI_TEST_MIHOMO").unwrap(), &installed).unwrap();
    std::fs::set_permissions(installed, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut owner = actor(paths.clone());
    owner.start().await.unwrap();
    let pid = owner.running.as_ref().unwrap().child.id();
    owner
        .patch(crate::storage::Values::from([
            ("Ipv6".into(), "false".into()),
            ("LogLevelIndex".into(), "debug".into()),
            ("FindProcessIndex".into(), "strict".into()),
        ]))
        .await
        .unwrap();
    assert_eq!(owner.running.as_ref().unwrap().child.id(), pid);
    assert_eq!(
        crate::core::Preferences::load(&paths).unwrap().native()["ipv6"],
        false
    );
    // Real TCP and UDP echo servers prove that both transports reach the target.
    let tcp = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target_port = tcp.local_addr().unwrap().port();
    let udp = tokio::net::UdpSocket::bind(("127.0.0.1", target_port))
        .await
        .unwrap();
    let reserve = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let tunnel_port = reserve.local_addr().unwrap().port();
    drop(reserve);
    let tcp_task = tokio::spawn(async move {
        let (mut stream, _) = tcp.accept().await.unwrap();
        let mut bytes = [0; 4];
        stream.read_exact(&mut bytes).await.unwrap();
        stream.write_all(&bytes).await.unwrap();
    });
    let udp_task = tokio::spawn(async move {
        let mut bytes = [0; 4];
        let (count, address) = udp.recv_from(&mut bytes).await.unwrap();
        udp.send_to(&bytes[..count], address).await.unwrap();
    });
    let tunnels = format!(
        "- network: [tcp, udp]\n  address: 127.0.0.1:{tunnel_port}\n  target: 127.0.0.1:{target_port}\n  proxy: DIRECT\n"
    );
    let tunnel_entry = crate::configuration::tunnels::parse(&tunnels)
        .unwrap()
        .remove(0);
    owner
        .change_tunnel(crate::configuration::tunnels::Change::Add(
            tunnel_entry.clone(),
        ))
        .await
        .unwrap();
    assert_eq!(owner.running.as_ref().unwrap().child.id(), pid);
    let mut client = tokio::net::TcpStream::connect(("127.0.0.1", tunnel_port))
        .await
        .unwrap();
    client.write_all(b"ping").await.unwrap();
    let mut bytes = [0; 4];
    tokio::time::timeout(Duration::from_secs(3), client.read_exact(&mut bytes))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&bytes, b"ping");
    drop(client);
    let client = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    client
        .send_to(b"pong", ("127.0.0.1", tunnel_port))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), client.recv_from(&mut bytes))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&bytes, b"pong");
    tcp_task.await.unwrap();
    udp_task.await.unwrap();
    // A conflicting listener must not replace either the active configuration or its files.
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = occupied.local_addr().unwrap().port();
    let native_before = std::fs::read(crate::core::Preferences::paths(&paths)[0].clone()).unwrap();
    let bad = format!("- network: [tcp]\n  address: 127.0.0.1:{port}\n  target: 127.0.0.1:443\n");
    assert!(
        owner
            .patch(crate::storage::Values::from([("Tunnels".into(), bad)]))
            .await
            .is_err()
    );
    assert_eq!(
        std::fs::read(crate::core::Preferences::paths(&paths)[0].clone()).unwrap(),
        native_before
    );
    assert_eq!(owner.running.as_ref().unwrap().child.id(), pid);
    owner
        .patch(crate::storage::Values::from([(
            "Tunnels".into(),
            "[]".into(),
        )]))
        .await
        .unwrap();
    assert!(
        tokio::net::TcpStream::connect(("127.0.0.1", tunnel_port))
            .await
            .is_err()
    );
    // An unreadable shared settings file rejects the candidate before runtime reload.
    let app_path = crate::core::Preferences::paths(&paths)[1].clone();
    let old = std::fs::read(&app_path).unwrap();
    std::fs::remove_file(&app_path).unwrap();
    std::fs::create_dir(&app_path).unwrap();
    // Use the preloaded desired preferences, since the damaged app file prevents a normal load.
    let previous = owner.snapshot.preferences.clone().unwrap();
    let mut values = previous.values();
    values.insert("Ipv6".into(), "true".into());
    let desired = crate::core::Preferences::from_values(&values, Some(&previous)).unwrap();
    assert!(
        owner
            .configure_hot(previous, desired)
            .await
            .unwrap_err()
            .contains("程序设置")
    );
    std::fs::remove_dir(&app_path).unwrap();
    std::fs::write(&app_path, old).unwrap();
    assert_eq!(owner.running.as_ref().unwrap().child.id(), pid);
    let expected: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&owner.running.as_ref().unwrap().launch.composed).unwrap();
    owner
        .running
        .as_ref()
        .unwrap()
        .launch
        .client
        .verify_listeners(&expected)
        .await
        .unwrap();
    // External CORS follows CV's explicit restart boundary, independently of the private channel.
    let reserve = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = reserve.local_addr().unwrap().port();
    drop(reserve);
    owner
        .patch(crate::storage::Values::from([
            ("ExternalController".into(), format!("127.0.0.1:{port}")),
            ("Secret".into(), "cors-test-secret".into()),
            ("CorsOrigins".into(), "https://example.org".into()),
            ("EnableExternalController".into(), "true".into()),
        ]))
        .await
        .unwrap();
    let mut http = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    http.write_all(b"GET /version HTTP/1.1\r\nHost: localhost\r\nOrigin: https://example.org\r\nAuthorization: Bearer cors-test-secret\r\nConnection: close\r\n\r\n").await.unwrap();
    let mut response = Vec::new();
    http.read_to_end(&mut response).await.unwrap();
    let response = String::from_utf8(response).unwrap().to_lowercase();
    assert!(
        response.contains("access-control-allow-origin: https://example.org"),
        "{response}"
    );
    assert_ne!(owner.running.as_ref().unwrap().child.id(), pid);
    owner.stop().await.unwrap();
    let interfaces = super::super::interfaces::list().await.unwrap();
    assert!(!interfaces.is_empty());
}

#[tokio::test]
async fn webui_replacement_removes_deleted_panel_keys_without_resetting_other_preferences() {
    let (_root, paths) = fixture(None);
    let mut owner = actor(paths.clone());
    let values = crate::storage::Values::from([
        ("WebUiConfigured".into(), "true".into()),
        ("WebUi.1.name".into(), "我的面板".into()),
        ("WebUi.1.url".into(), "https://example.com".into()),
        ("CoreReleaseIndex".into(), "preview".into()),
    ]);
    owner.patch(values).await.unwrap();
    owner
        .patch(crate::storage::Values::from([(
            "WebUiConfigured".into(),
            "true".into(),
        )]))
        .await
        .unwrap();
    let preferences = crate::core::Preferences::load(&paths).unwrap();
    let values = preferences.values();
    assert!(!values.keys().any(|key| key.starts_with("WebUi.")));
    assert_eq!(values["CoreReleaseIndex"], "预览版");
    let app = std::fs::read_to_string(crate::core::Preferences::paths(&paths)[1].clone()).unwrap();
    assert!(app.contains("preview"));
    assert!(!app.contains("预览版"));
}

#[tokio::test]
async fn first_start_without_core_reports_download_guidance_without_fallback() {
    let (_root, paths) = fixture(None);
    let mut owner = actor(paths);
    let error = owner.startup().await.unwrap_err();
    assert!(error.contains("「内核」"));
    assert!(!error.contains("基础配置"));
    assert!(owner.running.is_none());
    assert!(owner.validation.is_none());
}

#[tokio::test]
async fn missing_geodata_allows_validation_download_time_and_reports_network_recovery() {
    let (_root, paths) = fixture(Some("#!/bin/sh\nsleep 6\nexit 0\n"));
    let mut owner = actor(paths.clone());
    let launch = Launch::prepare(&paths).await.unwrap();
    owner.validate(launch).await.unwrap();
    assert!(owner.validation.is_none());
    std::fs::write(
        paths.directory(DirectoryKind::Core).join("mihomo"),
        "#!/bin/sh\nexit 1\n",
    )
    .unwrap();
    let launch = Launch::prepare(&paths).await.unwrap();
    let error = owner.validate(launch).await.err().unwrap();
    assert!(error.contains("检查网络"));
    assert!(error.contains("保留原运行配置"));
    assert!(owner.validation.is_none());
}

#[tokio::test]
async fn first_install_validates_staged_core_without_requiring_managed_core() {
    let (root, paths) = fixture(None);
    let executable = root.path().join("staged-core");
    std::fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    let source = crate::configuration::Source::load(&paths).await.unwrap();
    let launch = Launch::for_install(&paths, source, executable)
        .await
        .unwrap();
    let mut owner = actor(paths.clone());
    owner.validate(launch).await.unwrap();
    assert!(crate::bootstrap::require_core(&paths).is_err());
}

#[tokio::test]
#[ignore = "requires explicit SUKI_TEST_BUNDLE; isolated file providers and local IPC only"]
async fn real_resources_hot_save_file_provider_updates_and_failure_retention() {
    use crate::core::ProviderKind;
    let bundle = std::env::var_os("SUKI_TEST_BUNDLE").unwrap();
    let (_root, paths) = fixture(None);
    crate::bootstrap::prepare(&paths, Path::new(&bundle), || false).unwrap();
    let core_dir = paths.directory(DirectoryKind::Core);
    let proxy_file = core_dir.join("local-proxies.yaml");
    let rule_file = core_dir.join("local-rules.yaml");
    std::fs::write(
        &proxy_file,
        "proxies:\n  - {name: local-one, type: socks5, server: 127.0.0.1, port: 9}\n",
    )
    .unwrap();
    std::fs::write(&rule_file, "payload:\n  - DOMAIN,one.example.test\n").unwrap();
    save_profile(
        &paths,
        42,
        "mixed-port: 0\nport: 0\nsocks-port: 0\nlog-level: silent\nproxy-providers:\n  local/name?: {type: file, path: local-proxies.yaml}\nrule-providers:\n  local-rules: {type: file, path: local-rules.yaml, behavior: classical}\nrules: ['RULE-SET,local-rules,DIRECT', 'MATCH,DIRECT']\n",
    );
    let mut owner = actor(paths.clone());
    owner.activate(42).await.unwrap();
    let pid = owner.running.as_ref().unwrap().child.id();
    let generation = owner.snapshot.generation();
    owner.resources_requested = true;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            owner.refresh_resources().await.unwrap();
            if owner
                .snapshot
                .resources()
                .providers(ProviderKind::Rules)
                .first()
                .is_some_and(|provider| provider.count() == 1)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        owner.snapshot.resources().providers(ProviderKind::Proxies)[0].count(),
        1
    );
    assert_eq!(
        owner.snapshot.resources().providers(ProviderKind::Rules)[0].count(),
        1
    );
    owner
        .patch(crate::storage::Values::from([
            ("GeoAutoUpdate".into(), "false".into()),
            ("GeoMode".into(), "db".into()),
            ("GeoUpdateIntervalValue".into(), "6".into()),
            ("GeoIpUrl".into(), "http://127.0.0.1:9/geo".into()),
        ]))
        .await
        .unwrap();
    assert_eq!(owner.running.as_ref().unwrap().child.id(), pid);
    assert_eq!(owner.snapshot.generation(), generation);
    let preferences = crate::core::Preferences::load(&paths).unwrap();
    assert_eq!(preferences.native()["geo-update-interval"], 6);
    assert_eq!(
        preferences.native()["geox-url"]["geoip"],
        "http://127.0.0.1:9/geo"
    );
    let yaml = owner.running.as_ref().unwrap().launch.yaml().await.unwrap();
    assert!(yaml.contains("geo-update-interval: 6"));
    std::fs::write(&proxy_file, "proxies:\n  - {name: local-one, type: socks5, server: 127.0.0.1, port: 9}\n  - {name: local-two, type: socks5, server: 127.0.0.1, port: 10}\n").unwrap();
    owner
        .update_providers(
            ProviderKind::Proxies,
            Some("local/name?".into()),
            generation,
        )
        .await
        .unwrap();
    assert_eq!(
        owner.snapshot.resources().providers(ProviderKind::Proxies)[0].count(),
        2
    );
    std::fs::write(
        &rule_file,
        "payload:\n  - DOMAIN,one.example.test\n  - DOMAIN,two.example.test\n",
    )
    .unwrap();
    owner
        .update_providers(ProviderKind::Rules, None, generation)
        .await
        .unwrap();
    assert_eq!(
        owner.snapshot.resources().providers(ProviderKind::Rules)[0].count(),
        2
    );
    std::fs::write(&proxy_file, "proxies: invalid\n").unwrap();
    let error = owner
        .update_providers(ProviderKind::Proxies, None, generation)
        .await
        .unwrap_err();
    assert!(error.contains("失败 1 项"));
    assert_eq!(
        owner.snapshot.resources().providers(ProviderKind::Proxies)[0].count(),
        2
    );
    assert!(
        owner
            .update_providers(ProviderKind::Rules, None, generation + 1)
            .await
            .unwrap_err()
            .contains("内核已变更")
    );
    assert!(
        owner
            .patch(crate::storage::Values::from([(
                "GeoUpdateIntervalValue".into(),
                "0".into()
            )]))
            .await
            .is_err()
    );
    assert_eq!(
        crate::core::Preferences::load(&paths).unwrap().native()["geo-update-interval"],
        6
    );
    assert_eq!(owner.running.as_ref().unwrap().child.id(), pid);
    owner.stop().await.unwrap();
}

#[tokio::test]
async fn stopped_resource_settings_save_without_starting_core() {
    let (_root, paths) = fixture(None);
    let mut owner = actor(paths.clone());
    owner
        .patch(crate::storage::Values::from([
            ("GeoAutoUpdate".into(), "true".into()),
            ("GeoUpdateIntervalValue".into(), "12".into()),
        ]))
        .await
        .unwrap();
    assert!(owner.running.is_none());
    assert_eq!(owner.snapshot.phase(), Phase::Stopped);
    assert_eq!(
        crate::core::Preferences::load(&paths).unwrap().values()["GeoUpdateIntervalValue"],
        "12"
    );
}

#[tokio::test]
async fn missing_core_does_not_block_generic_settings_saves() {
    use crate::preferences::SettingsKind;
    let (_root, paths) = fixture(None);
    let mut owner = actor(paths.clone());
    assert!(owner.startup().await.unwrap_err().contains("未安装"));
    owner.publish(Phase::Failed, None);
    assert_eq!(owner.snapshot.label(), "不存在内核");
    assert!(owner.running.is_none());
    owner
        .save_settings(
            SettingsKind::General,
            crate::storage::Values::from([
                ("ThemeIndex".into(), "浅色".into()),
                ("UserAgent".into(), "原文".into()),
            ]),
        )
        .await
        .unwrap();
    let repository = Repository::open(paths.clone()).unwrap();
    let values = crate::preferences::values(&repository, SettingsKind::General).unwrap();
    assert_eq!(values["ThemeIndex"], "light");
    assert_eq!(values["UserAgent"], "原文");
    assert!(
        owner
            .save_settings(
                SettingsKind::General,
                crate::storage::Values::from([("ThemeIndex".into(), "invalid-theme".into())])
            )
            .await
            .is_err()
    );
    assert_eq!(
        crate::preferences::values(&repository, SettingsKind::General).unwrap(),
        values
    );
}

#[tokio::test]
#[ignore = "requires explicit SUKI_TEST_BUNDLE; GeoData HTTP server is loopback-only"]
async fn real_single_database_update_validates_download_before_replacing_file() {
    use crate::configuration::resources::Database;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let bundle = std::env::var_os("SUKI_TEST_BUNDLE").unwrap();
    let (_root, paths) = fixture(None);
    crate::bootstrap::prepare(&paths, Path::new(&bundle), || false).unwrap();
    let bytes = std::fs::read(Path::new(&bundle).join("geosite.dat")).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/geosite", listener.local_addr().unwrap());
    let expected = bytes.clone();
    let server = tokio::spawn(async move {
        for body in [bytes, b"invalid geo database".to_vec()] {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            let received = stream.read(&mut request).await.unwrap();
            assert!(received > 0);
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            stream.write_all(&body).await.unwrap();
        }
    });
    let mut owner = actor(paths.clone());
    owner
        .patch(crate::storage::Values::from([("GeoSiteUrl".into(), url)]))
        .await
        .unwrap();
    let destination = paths.directory(DirectoryKind::Core).join("geosite.dat");
    std::fs::write(&destination, b"old-file").unwrap();
    owner.update_database(Database::GeoSite).await.unwrap();
    assert_eq!(std::fs::read(&destination).unwrap(), expected);
    assert!(
        owner
            .update_database(Database::GeoSite)
            .await
            .unwrap_err()
            .contains("格式无效")
    );
    assert_eq!(std::fs::read(destination).unwrap(), expected);
    assert!(owner.running.is_none());
    server.await.unwrap();
}

#[tokio::test]
#[ignore = "requires explicit SUKI_TEST_MIHOMO; isolated private control socket only"]
async fn real_core_logs_are_parsed_published_and_saved() {
    use std::os::unix::fs::PermissionsExt as _;
    let (_root, paths) = fixture(None);
    let binary = paths.directory(DirectoryKind::Core).join("mihomo");
    std::fs::copy(std::env::var_os("SUKI_TEST_MIHOMO").unwrap(), &binary).unwrap();
    std::fs::set_permissions(binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    crate::configuration::Preferences::from_values(
        &crate::storage::Values::from([
            ("MixedPort".into(), "0".into()),
            ("LogLevelIndex".into(), "信息（info）".into()),
        ]),
        None,
    )
    .unwrap()
    .save(&paths)
    .unwrap();
    let mut owner = actor(paths.clone());
    owner.start().await.unwrap();
    owner.stop().await.unwrap();
    let records = owner.logs.since(0);
    assert!(
        records
            .iter()
            .any(|record| record.source == crate::logging::Source::Core
                && record.level == crate::logging::Level::Info)
    );
    assert!(
        records
            .iter()
            .all(|record| !record.message.starts_with("time="))
    );
    let saved = std::fs::read_dir(paths.directory(DirectoryKind::Logs))
        .unwrap()
        .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
        .collect::<String>();
    assert!(saved.contains(&records[0].message));
    assert!(saved.contains("level=info"));
}

#[tokio::test]
async fn profile_crud_is_serialized_rejects_stale_edits_and_removes_bodies() {
    use crate::profiles::{
        model::{Document, DocumentKind},
        repository,
    };
    let (_root, paths) = fixture(None);
    let mut owner = actor(paths.clone());
    let first = Document::new(1, DocumentKind::Profile);
    let second = Document::new(2, DocumentKind::Profile);
    owner
        .profiles(super::super::ProfileCommand::Save {
            expected: Default::default(),
            documents: vec![first.clone(), second.clone()],
        })
        .await
        .unwrap();
    assert!(repository::body_path(&paths, 1).exists());
    let (expected, mut documents) = repository::read(&paths).unwrap();
    documents.reverse();
    owner
        .profiles(super::super::ProfileCommand::Save {
            expected: expected.clone(),
            documents: documents.clone(),
        })
        .await
        .unwrap();
    assert!(
        owner
            .profiles(super::super::ProfileCommand::Save {
                expected,
                documents: vec![first]
            })
            .await
            .unwrap_err()
            .contains("已变更")
    );
    let (expected, _) = repository::read(&paths).unwrap();
    owner
        .profiles(super::super::ProfileCommand::Save {
            expected,
            documents: vec![second],
        })
        .await
        .unwrap();
    assert!(!repository::body_path(&paths, 1).exists());
    assert!(repository::body_path(&paths, 2).exists());
    assert_eq!(repository::read(&paths).unwrap().1.len(), 1);
    owner
        .profiles(super::super::ProfileCommand::Restore { ids: vec![1] })
        .await
        .unwrap();
    let documents = repository::read(&paths).unwrap().1;
    assert_eq!(documents.len(), 2);
    assert_eq!(documents[0].id, 2);
    assert_eq!(documents[1].id, 1);
    assert!(repository::body_path(&paths, 1).exists());
}

async fn subscription_server(
    body: &str,
    extra_headers: &str,
) -> (String, tokio::task::JoinHandle<String>) {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/config", listener.local_addr().unwrap());
    let body = body.to_owned();
    let extra_headers = extra_headers.to_owned();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buffer = [0u8; 1024];
        while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
            let length = stream.read(&mut buffer).await.unwrap();
            if length == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..length]);
            assert!(request.len() < 65536);
        }
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n{}\r\n{}",
            body.len(),
            extra_headers,
            body
        );
        stream.write_all(response.as_bytes()).await.unwrap();
        String::from_utf8(request).unwrap()
    });
    (url, task)
}

#[tokio::test]
async fn remote_import_downloads_metadata_and_failed_update_keeps_previous_source() {
    use crate::profiles::{
        model::{Document, DocumentKind},
        repository,
    };
    let (_root, paths) = fixture(None);
    let mut owner = actor(paths.clone());
    let (url, server) = subscription_server(
        "proxies: []\nrules: ['MATCH,DIRECT']\n",
        "Subscription-Userinfo: upload=10; download=20; total=1000\r\n",
    )
    .await;
    let mut remote = Document::new(1, DocumentKind::Profile);
    remote.url = url;
    remote.content.clear();
    remote.user_agent = "Subscription-Test/1".into();
    owner
        .profiles(super::super::ProfileCommand::Save {
            expected: Default::default(),
            documents: vec![remote],
        })
        .await
        .unwrap();
    assert!(server.await.unwrap().contains("Subscription-Test/1"));
    let (expected, mut documents) = repository::read(&paths).unwrap();
    assert_eq!(documents[0].usage.as_ref().unwrap().used_bytes, 30);
    assert!(documents[0].updated_at.is_some());
    let old_content = documents[0].content.clone();
    let (url, server) = subscription_server("<html>invalid response</html>", "").await;
    documents[0].url = url;
    owner
        .profiles(super::super::ProfileCommand::Save {
            expected,
            documents,
        })
        .await
        .unwrap();
    let before = repository::read(&paths).unwrap().0;
    assert!(
        owner
            .profiles(super::super::ProfileCommand::Update {
                ids: vec![1],
                automatic: false
            })
            .await
            .is_err()
    );
    server.await.unwrap();
    let (after, documents) = repository::read(&paths).unwrap();
    assert_eq!(before, after);
    assert_eq!(documents[0].content, old_content);
}

#[tokio::test]
async fn default_subscription_agent_identifies_clash_verge_before_suki() {
    use crate::profiles::model::{Document, DocumentKind};
    let (_root, paths) = fixture(None);
    let mut owner = actor(paths);
    let (url, server) = subscription_server("proxies: []\nrules: ['MATCH,DIRECT']\n", "").await;
    let mut document = Document::new(1, DocumentKind::Profile);
    document.url = url;
    document.content.clear();
    owner
        .profiles(super::super::ProfileCommand::Save {
            expected: Default::default(),
            documents: vec![document],
        })
        .await
        .unwrap();
    let request = server.await.unwrap();
    let agent = request
        .lines()
        .find_map(|line| {
            line.split_once(':')
                .filter(|(key, _)| key.eq_ignore_ascii_case("user-agent"))
                .map(|(_, value)| value.trim())
        })
        .unwrap();
    assert_eq!(agent, crate::network::DEFAULT_USER_AGENT);
    // CV's identity follows docs/cv-upstream-sync.md, independently of Suki releases.
    assert!(agent.starts_with("clash-verge/v2.6.0 "));
    assert!(agent.contains(" Suki-Clash/"));
}

#[tokio::test]
async fn subscription_authorization_uses_typed_headers_and_logs_never_include_it() {
    use crate::profiles::model::{Document, DocumentKind};
    let (_root, paths) = fixture(None);
    let (url, server) = subscription_server("rules: ['MATCH,DIRECT']\n", "").await;
    let mut doc = Document::new(1, DocumentKind::Profile);
    doc.url = url;
    doc.token = "Bearer private-subscription-secret".into();
    doc.credentials = crate::profiles::CredentialEdits::from_inputs(&doc.token, "");
    doc.user_agent = "Custom-Agent".into();
    let mut transfer = crate::core::download::Transfer::subscription(
        &crate::network::Http::default(),
        &paths,
        &doc,
        "Default-Agent",
        &crate::core::download::SubscriptionTransport::Direct,
    )
    .unwrap();
    let bytes = transfer.finish(|_, _| {}).await.unwrap();
    assert!(bytes.starts_with(b"rules:"));
    let request = server.await.unwrap();
    assert!(
        request
            .to_lowercase()
            .contains("authorization: bearer private-subscription-secret")
    );
    assert!(request.contains("Custom-Agent"));
    assert!(
        !crate::profiles::store::values(&[doc], None)
            .values()
            .any(|value| value.contains("private-subscription-secret"))
    );
}

#[tokio::test]
async fn automatic_subscription_update_uses_updated_deadline_and_disabled_flag() {
    use crate::profiles::{
        model::{Document, DocumentKind},
        repository,
    };
    let (_root, paths) = fixture(None);
    let mut owner = actor(paths.clone());
    let (url, server) =
        subscription_server("proxies: []\nmode: direct\nrules: ['MATCH,DIRECT']\n", "").await;
    let mut doc = Document::new(1, DocumentKind::Profile);
    doc.url = url;
    doc.interval = "1".into();
    doc.updated_at = Some(chrono::Utc::now().timestamp() - 120);
    repository::commit(&paths, &[], &[doc], None).unwrap();
    owner.automatic_profiles().await.unwrap();
    server.await.unwrap();
    let (expected, mut documents) = repository::read(&paths).unwrap();
    assert!(documents[0].content.contains("mode: direct"));
    let before = documents[0].updated_at;
    owner.automatic_profiles().await.unwrap();
    assert_eq!(repository::read(&paths).unwrap().1[0].updated_at, before);
    documents[0].interval = "0".into();
    documents[0].updated_at = Some(1);
    owner
        .profiles(super::super::ProfileCommand::Save {
            expected,
            documents,
        })
        .await
        .unwrap();
    owner.automatic_profiles().await.unwrap();
    assert_eq!(repository::read(&paths).unwrap().1[0].updated_at, Some(1));
}

#[tokio::test]
#[ignore = "requires explicit SUKI_TEST_MIHOMO; isolated private control socket only"]
async fn real_subscription_edit_enhance_and_active_delete_apply_before_committing() {
    use crate::profiles::{
        model::{Document, DocumentKind},
        repository,
    };
    use std::os::unix::fs::PermissionsExt as _;
    let (_root, paths) = fixture(None);
    let binary = paths.directory(DirectoryKind::Core).join("mihomo");
    std::fs::copy(std::env::var_os("SUKI_TEST_MIHOMO").unwrap(), &binary).unwrap();
    std::fs::set_permissions(binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut owner = actor(paths.clone());
    let mut first = Document::new(1, DocumentKind::Profile);
    first.content = "rules: ['MATCH,DIRECT']\n".into();
    let mut second = Document::new(2, DocumentKind::Profile);
    second.content = first.content.clone();
    owner
        .profiles(super::super::ProfileCommand::Save {
            expected: Default::default(),
            documents: vec![first, second],
        })
        .await
        .unwrap();
    owner.activate(1).await.unwrap();
    let initial_pid = owner.running.as_ref().unwrap().child.id();
    let (expected, mut documents) = repository::read(&paths).unwrap();
    documents[0].content = "rules: ['DOMAIN,example.com,DIRECT', 'MATCH,DIRECT']\n".into();
    owner
        .profiles(super::super::ProfileCommand::Save {
            expected,
            documents,
        })
        .await
        .unwrap();
    assert!(
        owner
            .running
            .as_ref()
            .unwrap()
            .launch
            .composed
            .contains("example.com")
    );
    assert_eq!(owner.running.as_ref().unwrap().child.id(), initial_pid);
    let (before, mut documents) = repository::read(&paths).unwrap();
    let body = std::fs::read(repository::body_path(&paths, 1)).unwrap();
    let pid = owner.running.as_ref().unwrap().child.id();
    documents[0].content = "proxies: [{name: bad, type: invalid-proxy-type}]\n".into();
    assert!(
        owner
            .profiles(super::super::ProfileCommand::Save {
                expected: before.clone(),
                documents
            })
            .await
            .is_err()
    );
    assert_eq!(repository::read(&paths).unwrap().0, before);
    assert_eq!(
        std::fs::read(repository::body_path(&paths, 1)).unwrap(),
        body
    );
    assert_eq!(owner.running.as_ref().unwrap().child.id(), pid);
    let edits = crate::storage::Values::from([(
        "rules".into(),
        "prepend: ['DOMAIN,enhanced.example,DIRECT']\n".into(),
    )]);
    owner
        .profiles(super::super::ProfileCommand::Enhance {
            id: 1,
            expected: Default::default(),
            values: edits,
        })
        .await
        .unwrap();
    assert!(
        owner
            .running
            .as_ref()
            .unwrap()
            .launch
            .composed
            .contains("enhanced.example")
    );
    assert_eq!(
        std::fs::read(repository::body_path(&paths, 1)).unwrap(),
        body
    );
    let (expected, mut documents) = repository::read(&paths).unwrap();
    documents.retain(|doc| doc.id != 1);
    owner
        .profiles(super::super::ProfileCommand::Save {
            expected,
            documents,
        })
        .await
        .unwrap();
    assert_eq!(owner.snapshot.profile().unwrap().id(), 2);
    assert_eq!(
        crate::configuration::selected_profile(&paths).unwrap(),
        Some(2)
    );
    assert!(!repository::body_path(&paths, 1).exists());
    assert!(!paths.records().join("profile-enhance-1.json").exists());
    owner
        .profiles(super::super::ProfileCommand::Restore { ids: vec![1] })
        .await
        .unwrap();
    assert!(paths.records().join("profile-enhance-1.json").exists());
    assert_eq!(owner.snapshot.profile().unwrap().id(), 2);
    let (expected, _) = repository::read(&paths).unwrap();
    owner
        .profiles(super::super::ProfileCommand::Save {
            expected,
            documents: vec![],
        })
        .await
        .unwrap();
    assert!(owner.snapshot.profile().is_none());
    assert_eq!(owner.snapshot.phase(), Phase::Ready);
    assert_eq!(
        crate::configuration::selected_profile(&paths).unwrap(),
        None
    );
    owner.stop().await.unwrap();
}

#[tokio::test]
async fn force_subscription_proxy_does_not_silently_download_direct_when_core_is_stopped() {
    use crate::profiles::{
        model::{Document, DocumentKind},
        repository,
    };
    let (_root, paths) = fixture(None);
    let mut owner = actor(paths.clone());
    let store = Repository::open(paths.clone()).unwrap();
    let mut settings = crate::preferences::defaults(crate::preferences::SettingsKind::General);
    settings.insert("ProfileUseProxy".into(), "true".into());
    store
        .save(
            &crate::storage::Namespace::parse("settings").unwrap(),
            &settings,
        )
        .unwrap();
    let mut document = Document::new(1, DocumentKind::Profile);
    document.url = "http://127.0.0.1:1/config".into();
    document.content.clear();
    document.self_proxy = true;
    let error = owner
        .profiles(super::super::ProfileCommand::Save {
            expected: Default::default(),
            documents: vec![document],
        })
        .await
        .unwrap_err();
    assert!(error.contains("需要已就绪"));
    assert!(repository::read(&paths).unwrap().1.is_empty());
    assert!(owner.transfer.is_none());
}

#[tokio::test]
#[ignore = "requires SUKI_TEST_MIHOMO"]
async fn real_proxy_selection_modes_persistence_and_readback() {
    use crate::proxies::runtime::{Action, ProxyMode};
    use std::os::unix::fs::PermissionsExt as _;
    let (_root, paths) = fixture(None);
    let binary = paths.directory(DirectoryKind::Core).join("mihomo");
    std::fs::copy(std::env::var_os("SUKI_TEST_MIHOMO").unwrap(), &binary).unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(paths.directory(DirectoryKind::Config).join("runtime-source.yaml"),"mixed-port: 0\nport: 0\nsocks-port: 0\nproxy-groups:\n  - name: '选择 / test'\n    type: select\n    proxies: [DIRECT, REJECT]\n  - name: automatic\n    type: url-test\n    proxies: [DIRECT, REJECT]\n    url: http://127.0.0.1:9\n    interval: 3600\n    lazy: true\nrules: ['MATCH,选择 / test']\n").unwrap();
    let mut owner = actor(paths.clone());
    owner.start().await.unwrap();
    let pid = owner.running.as_ref().unwrap().child.id();
    let generation = owner.snapshot.generation;
    owner
        .proxy_action(Action::Select {
            group: "选择 / test".into(),
            node: "REJECT".into(),
            generation,
            epoch: owner.snapshot.monitor_epoch,
        })
        .await
        .unwrap();
    let client = owner.running.as_ref().unwrap().launch.client.clone();
    assert_eq!(
        client.proxies(&[]).await.unwrap().selected("选择 / test"),
        Some("REJECT")
    );
    assert!(
        owner
            .proxy_action(Action::Select {
                group: "选择 / test".into(),
                node: "missing".into(),
                generation,
                epoch: owner.snapshot.monitor_epoch,
            })
            .await
            .is_err()
    );
    owner
        .proxy_action(Action::Select {
            group: "automatic".into(),
            node: "REJECT".into(),
            generation,
            epoch: owner.snapshot.monitor_epoch,
        })
        .await
        .unwrap();
    assert_eq!(
        client.proxies(&[]).await.unwrap().entries["automatic"]
            .fixed
            .as_deref(),
        Some("REJECT")
    );
    owner
        .proxy_action(Action::Unfix {
            group: "automatic".into(),
            generation,
            epoch: owner.snapshot.monitor_epoch,
        })
        .await
        .unwrap();
    assert!(
        client.proxies(&[]).await.unwrap().entries["automatic"]
            .fixed
            .as_deref()
            .is_none_or(str::is_empty)
    );
    let target = super::super::monitor::Target {
        logs: None,
        client: client.clone(),
        http: crate::network::Http::default(),
        generation,
        epoch: owner.snapshot.monitor_epoch,
        order: vec!["选择 / test".into()],
        proxy_port: 0,
        proxy_auth: None,
        dns_mode: None,
        proxy_error: None,
    };
    let (connections, mode, _, proxies, _) = super::super::monitor::sample(&target).await.unwrap();
    assert_eq!(connections.connections.len(), 0);
    assert_eq!(mode, ProxyMode::Rule);
    assert_eq!(proxies.selected("选择 / test"), Some("REJECT"));
    let repository = Repository::open(paths.clone()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buffer = [0; 4096];
        assert!(socket.read(&mut buffer).await.unwrap() > 0);
        socket
            .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
    });
    let mut general = crate::preferences::defaults(crate::preferences::SettingsKind::General);
    general.insert("DelayTestUrl".into(), format!("http://{address}/test"));
    repository
        .save(
            &crate::storage::Namespace::parse("settings").unwrap(),
            &general,
        )
        .unwrap();
    client
        .node_delay("DIRECT", None, &format!("http://{address}/test"), 5000)
        .await
        .unwrap();
    server.await.unwrap();
    assert!(
        !client.proxies(&[]).await.unwrap().entries["DIRECT"]
            .history
            .is_empty()
    );
    for mode in [ProxyMode::Global, ProxyMode::Direct, ProxyMode::Rule] {
        owner.proxy_action(Action::Mode(mode)).await.unwrap();
        assert_eq!(client.json("/configs").await.unwrap()["mode"], mode.label());
        assert_eq!(
            crate::core::Preferences::load(&paths).unwrap().native()["mode"],
            mode.label()
        );
        assert_eq!(owner.running.as_ref().unwrap().child.id(), pid);
    }
    owner.restart().await.unwrap();
    assert_eq!(
        owner
            .running
            .as_ref()
            .unwrap()
            .launch
            .client
            .proxies(&[])
            .await
            .unwrap()
            .selected("选择 / test"),
        Some("REJECT")
    );
    assert!(
        owner
            .proxy_action(Action::Select {
                group: "选择 / test".into(),
                node: "DIRECT".into(),
                generation,
                epoch: owner.snapshot.monitor_epoch,
            })
            .await
            .is_err()
    );
    owner.stop().await.unwrap();
}

#[tokio::test]
#[ignore = "requires SUKI_TEST_MIHOMO"]
async fn real_websocket_traffic_connections_and_confirmed_close_use_private_ipc() {
    use futures_util::StreamExt;
    use std::os::unix::fs::PermissionsExt as _;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (_root, paths) = fixture(None);
    let installed = paths.directory(DirectoryKind::Core).join("mihomo");
    std::fs::copy(std::env::var_os("SUKI_TEST_MIHOMO").unwrap(), &installed).unwrap();
    std::fs::set_permissions(&installed, std::fs::Permissions::from_mode(0o700)).unwrap();
    let reservation = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = reservation.local_addr().unwrap().port();
    drop(reservation);
    core_settings(&paths, port);
    let mut owner = actor(paths.clone());
    owner.start().await.unwrap();
    let (runtime_updates, runtime_events) = async_channel::bounded(2);
    let (shutdown, cancellation) = tokio::sync::watch::channel(false);
    let monitor = tokio::spawn(super::super::monitor::run(
        paths.clone(),
        owner.monitor.subscribe(),
        runtime_updates,
        runtime_events.clone(),
        cancellation,
        None,
        None,
    ));
    let client = owner.running.as_ref().unwrap().launch.client.clone();
    let mut connections = client.websocket("/connections?interval=100").await.unwrap();
    let mut traffic = client.websocket("/traffic").await.unwrap();
    let destination = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = destination.local_addr().unwrap();
    let (release, wait) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        let (mut stream, _) = destination.accept().await.unwrap();
        let mut request = [0; 4096];
        assert!(stream.read(&mut request).await.unwrap() > 0);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1048576\r\n\r\n")
            .await
            .unwrap();
        stream.write_all(&vec![b'x'; 4096]).await.unwrap();
        let _ = wait.await;
    });
    let http = crate::network::Http::default();
    let route = crate::network::Transport::Proxy {
        url: format!("http://127.0.0.1:{port}"),
        authentication: None,
    };
    let mut response = http
        .client(&route, false)
        .unwrap()
        .get(format!("http://{address}/stream"))
        .send()
        .await
        .unwrap();
    assert!(!response.chunk().await.unwrap().unwrap().is_empty());
    let id = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let message = connections.next().await.unwrap().unwrap();
            if let tokio_tungstenite::tungstenite::Message::Text(text) = message {
                let frame: crate::overview::statistics::Connections =
                    serde_json::from_str(&text).unwrap();
                if let Some(connection) = frame.connections.iter().find(|connection| {
                    connection.metadata.destination_port == address.port().to_string()
                }) {
                    break connection.id.clone();
                }
            }
        }
    })
    .await
    .unwrap();
    let traffic_frame = tokio::time::timeout(Duration::from_secs(3), traffic.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let traffic_value: serde_json::Value =
        serde_json::from_slice(&traffic_frame.into_data()).unwrap();
    assert!(traffic_value["up"].as_u64().is_some());
    assert!(traffic_value["down"].as_u64().is_some());
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let runtime = runtime_events.recv().await.unwrap();
            if runtime.connections_live
                && runtime.traffic_live
                && runtime
                    .connections
                    .connections
                    .iter()
                    .any(|connection| connection.id == id)
            {
                break;
            }
        }
    })
    .await
    .unwrap();
    owner
        .connection_action(crate::traffic::ConnectionAction::Close(
            crate::traffic::ConnectionTarget {
                id: id.clone(),
                generation: owner.snapshot.generation,
                epoch: owner.snapshot.monitor_epoch,
            },
        ))
        .await
        .unwrap();
    let observed: crate::overview::statistics::Connections =
        serde_json::from_value(client.json("/connections").await.unwrap()).unwrap();
    assert!(
        !observed
            .connections
            .iter()
            .any(|connection| connection.id == id)
    );
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let runtime = runtime_events.recv().await.unwrap();
            if runtime.connections_live
                && !runtime
                    .connections
                    .connections
                    .iter()
                    .any(|connection| connection.id == id)
            {
                break;
            }
        }
    })
    .await
    .unwrap();
    let _ = release.send(());
    server.await.unwrap();
    drop(response);
    drop(connections);
    drop(traffic);
    owner.stop().await.unwrap();
    shutdown.send_replace(true);
    tokio::time::timeout(Duration::from_secs(2), monitor)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
#[ignore = "requires SUKI_TEST_MIHOMO"]
async fn real_group_tests_keep_group_urls_and_results_separate() {
    use std::os::unix::fs::PermissionsExt as _;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let (_root, paths) = fixture(None);
    let binary = paths.directory(DirectoryKind::Core).join("mihomo");
    std::fs::copy(std::env::var_os("SUKI_TEST_MIHOMO").unwrap(), &binary).unwrap();
    std::fs::set_permissions(binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(3));
    let mut servers = Vec::new();
    let mut urls = Vec::new();
    for name in ["a", "b"] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        urls.push(format!("http://{}/{name}", listener.local_addr().unwrap()));
        let barrier = barrier.clone();
        servers.push(tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = [0; 4096];
            let n = stream.read(&mut bytes).await.unwrap();
            assert!(String::from_utf8_lossy(&bytes[..n]).starts_with(&format!("HEAD /{name} ")));
            barrier.wait().await;
            tokio::time::sleep(Duration::from_millis(25)).await;
            stream
                .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
        }));
    }
    std::fs::write(paths.directory(DirectoryKind::Config).join("runtime-source.yaml"),format!("mixed-port: 0\nport: 0\nsocks-port: 0\nproxy-groups:\n  - name: hop\n    type: select\n    proxies: [DIRECT]\n  - name: a\n    type: select\n    proxies: [hop]\n    url: {}\n  - name: b\n    type: select\n    proxies: [hop]\n    url: {}\nrules: ['MATCH,DIRECT']\n",urls[0],urls[1])).unwrap();
    let mut owner = actor(paths);
    owner.start().await.unwrap();
    let (requests, receiver) = async_channel::bounded(32);
    let (updates, mut events) = tokio::sync::watch::channel(super::super::delays::State::default());
    let (shutdown, cancellation) = tokio::sync::watch::channel(false);
    let worker = tokio::spawn(super::super::delays::run(
        owner.paths.clone(),
        owner.monitor.subscribe(),
        receiver,
        updates,
        cancellation,
    ));
    let target = owner.monitor.borrow().clone().unwrap();
    for (index, group) in ["a", "b"].into_iter().enumerate() {
        requests
            .send(super::super::delays::Request {
                id: index as u64 + 1,
                test: crate::proxies::runtime::Test {
                    urls: Default::default(),
                    group: Some(group.into()),
                    node: Some("hop".into()),
                    generation: target.generation,
                    epoch: target.epoch,
                },
            })
            .await
            .unwrap();
    }
    // Both servers must receive a measurement before either can finish.
    tokio::time::timeout(Duration::from_secs(4), barrier.wait())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            events.changed().await.unwrap();
            let state = events.borrow_and_update().clone();
            if state.results.len() == 2 && state.testing.is_empty() {
                break;
            }
        }
    })
    .await
    .unwrap();
    let results = events.borrow().results.clone();
    shutdown.send_replace(true);
    tokio::time::timeout(Duration::from_secs(1), worker)
        .await
        .unwrap()
        .unwrap();
    for server in servers {
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap();
    }
    assert!(
        results[&("a".into(), "hop".into())]
            .as_ref()
            .is_ok_and(|delay| *delay >= 20)
    );
    assert!(
        results[&("b".into(), "hop".into())]
            .as_ref()
            .is_ok_and(|delay| *delay >= 20)
    );
    owner.stop().await.unwrap();
}

#[test]
fn measurement_requests_do_not_reserve_the_lifecycle_lane() {
    let (_root, paths) = fixture(None);
    let mut service = Service::start(paths).unwrap();
    service
        .busy
        .store(true, std::sync::atomic::Ordering::Release);
    assert!(
        service
            .test_delay(crate::proxies::runtime::Test {
                urls: Default::default(),
                group: None,
                node: None,
                generation: 1,
                epoch: 1
            })
            .is_ok()
    );
    assert!(matches!(
        service.submit(Command::Start),
        Err(super::ServiceError::Busy)
    ));
    service
        .busy
        .store(false, std::sync::atomic::Ordering::Release);
    service.shutdown().unwrap().join().unwrap().unwrap();
}

#[tokio::test]
#[ignore = "requires SUKI_TEST_MIHOMO"]
async fn real_chain_save_applies_links_selects_exit_restores_on_restart_and_disconnects() {
    for kind in ["select", "url-test"] {
        chain_roundtrip(kind).await;
    }
}
async fn chain_roundtrip(kind: &str) {
    use crate::proxies::chain_config::{Change, Config};
    use std::os::unix::fs::PermissionsExt as _;
    let (_root, paths) = fixture(None);
    let binary = paths.directory(DirectoryKind::Core).join("mihomo");
    std::fs::copy(std::env::var_os("SUKI_TEST_MIHOMO").unwrap(), &binary).unwrap();
    std::fs::set_permissions(binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(paths.directory(DirectoryKind::Config).join("runtime-source.yaml"),"mixed-port: 0\nport: 0\nsocks-port: 0\nproxies:\n- {name: entry, type: socks5, server: 127.0.0.1, port: 10081}\n- {name: exit, type: socks5, server: 127.0.0.1, port: 10082}\nproxy-groups:\n- {name: select, type: GROUP_KIND, proxies: [DIRECT, entry, exit], url: 'http://127.0.0.1:9/health', interval: 300}\nrules: ['MATCH,select']\n".replace("GROUP_KIND",kind)).unwrap();
    let mut owner = actor(paths.clone());
    owner.start().await.unwrap();
    let pid = owner.running.as_ref().unwrap().child.id();
    let previous = Config::load(&paths).unwrap();
    let desired = Config {
        enabled: true,
        group: "select".into(),
        nodes: vec!["entry".into(), "exit".into()],
        custom: String::new(),
        sources: Default::default(),
    };
    owner
        .change_chain(Change {
            expected: previous,
            desired: desired.clone(),
            generation: owner.snapshot.generation,
            epoch: owner.snapshot.monitor_epoch,
        })
        .await
        .unwrap();
    assert_eq!(owner.running.as_ref().unwrap().child.id(), pid);
    assert_eq!(Config::load(&paths).unwrap(), desired);
    let config: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&owner.running.as_ref().unwrap().launch.composed).unwrap();
    assert_eq!(config["proxies"][1]["dialer-proxy"].as_str(), Some("entry"));
    let client = owner.running.as_ref().unwrap().launch.client.clone();
    assert!(
        client
            .proxies(&[])
            .await
            .unwrap()
            .confirms_selection("select", "exit")
    );
    let invalid = Config {
        nodes: vec!["entry".into(), "missing".into()],
        ..desired.clone()
    };
    assert!(
        owner
            .change_chain(Change {
                expected: desired.clone(),
                desired: invalid,
                generation: owner.snapshot.generation,
                epoch: owner.snapshot.monitor_epoch
            })
            .await
            .is_err()
    );
    assert_eq!(Config::load(&paths).unwrap(), desired);
    owner.restart().await.unwrap();
    let client = owner.running.as_ref().unwrap().launch.client.clone();
    assert!(
        client
            .proxies(&[])
            .await
            .unwrap()
            .confirms_selection("select", "exit")
    );
    owner
        .change_chain(Change {
            expected: desired.clone(),
            desired: Config {
                enabled: false,
                ..desired
            },
            generation: owner.snapshot.generation,
            epoch: owner.snapshot.monitor_epoch,
        })
        .await
        .unwrap();
    let config: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&owner.running.as_ref().unwrap().launch.composed).unwrap();
    assert!(config["proxies"][1].get("dialer-proxy").is_none());
    assert!(
        client
            .proxies(&[])
            .await
            .unwrap()
            .confirms_selection("select", "DIRECT")
    );
    owner.stop().await.unwrap();
}

#[tokio::test]
async fn stopped_overrides_import_update_schedule_and_reject_stale_changes() {
    use crate::profiles::{
        model::{Document, DocumentKind},
        override_repository as overrides,
    };
    let (_root, paths) = fixture(None);
    let mut owner = actor(paths.clone());
    let (url, server) = subscription_server(
        "function main(c){c.rules=['MATCH,DIRECT'];return c;}",
        "Content-Disposition: attachment; filename=remote.js\r\n",
    )
    .await;
    let mut document = Document::new(71, DocumentKind::JavaScript);
    document.url = url;
    document.name.clear();
    document.content.clear();
    document.interval = "0".into();
    owner
        .overrides(crate::core::OverrideChange::Save {
            expected: Default::default(),
            documents: vec![document],
        })
        .await
        .unwrap();
    server.await.unwrap();
    let (expected, mut docs) = overrides::read(&paths).unwrap();
    assert_eq!(docs[0].name, "remote");
    assert!(docs[0].updated_at.is_some());
    assert!(owner.running.is_none());
    let (url, server) = subscription_server(
        "function main(c){c.rules=['DOMAIN,new.example,DIRECT','MATCH,DIRECT'];return c;}",
        "",
    )
    .await;
    docs[0].url = url;
    owner
        .overrides(crate::core::OverrideChange::Save {
            expected: expected.clone(),
            documents: docs,
        })
        .await
        .unwrap();
    assert!(
        owner
            .overrides(crate::core::OverrideChange::Save {
                expected,
                documents: vec![]
            })
            .await
            .unwrap_err()
            .contains("已变更")
    );
    owner
        .overrides(crate::core::OverrideChange::Update {
            ids: vec![71],
            automatic: false,
        })
        .await
        .unwrap();
    server.await.unwrap();
    let (expected, mut docs) = overrides::read(&paths).unwrap();
    assert!(docs[0].content.contains("new.example"));
    let original = docs[0].content.clone();
    let (url, server) = subscription_server("function invalid(", "").await;
    docs[0].url = url;
    owner
        .overrides(crate::core::OverrideChange::Save {
            expected,
            documents: docs,
        })
        .await
        .unwrap();
    assert!(
        owner
            .overrides(crate::core::OverrideChange::Update {
                ids: vec![71],
                automatic: false
            })
            .await
            .is_err()
    );
    server.await.unwrap();
    assert_eq!(overrides::read(&paths).unwrap().1[0].content, original);
    owner.automatic_overrides().await.unwrap(); // interval 0 never starts a second HTTP request.
    let expected = overrides::read(&paths).unwrap().0;
    owner
        .overrides(crate::core::OverrideChange::Save {
            expected,
            documents: vec![],
        })
        .await
        .unwrap();
    assert!(overrides::read(&paths).unwrap().1.is_empty());
    owner
        .overrides(crate::core::OverrideChange::Restore { ids: vec![71] })
        .await
        .unwrap();
    assert_eq!(overrides::read(&paths).unwrap().1[0].content, original);
}

#[tokio::test]
#[ignore = "requires SUKI_TEST_MIHOMO; isolated private control socket, DNS listener disabled"]
async fn real_rules_overrides_dns_sniffer_apply_atomically_without_restarting() {
    use crate::{
        preferences::SettingsKind,
        profiles::{
            model::{Document, DocumentKind},
            override_repository as overrides, repository,
        },
        storage::Values,
    };
    use std::os::unix::fs::PermissionsExt as _;
    let (_root, paths) = fixture(None);
    let binary = paths.directory(DirectoryKind::Core).join("mihomo");
    std::fs::copy(std::env::var_os("SUKI_TEST_MIHOMO").unwrap(), &binary).unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut owner = actor(paths.clone());
    let mut profile = Document::new(1, DocumentKind::Profile);
    profile.content="dns: {enable: false, nameserver-policy: {'example.com': ['1.1.1.1']}}\nrules: ['MATCH,DIRECT']\n".into();
    owner
        .profiles(crate::core::ProfileCommand::Save {
            expected: Default::default(),
            documents: vec![profile],
        })
        .await
        .unwrap();
    owner.activate(1).await.unwrap();
    let pid = owner.running.as_ref().unwrap().child.id();
    let mut yaml = Document::new(2, DocumentKind::Yaml);
    yaml.content = "rules: ['DOMAIN,one.example,DIRECT','MATCH,DIRECT']".into();
    let mut script = Document::new(3, DocumentKind::JavaScript);
    script.content="function main(c,n){console.info(n);c.rules.unshift('DOMAIN,two.example,DIRECT');return c;}".into();
    owner
        .overrides(crate::core::OverrideChange::Save {
            expected: Default::default(),
            documents: vec![yaml, script],
        })
        .await
        .unwrap();
    owner.refresh_rules().await.unwrap();
    let rules = &owner.snapshot.rules.entries;
    assert_eq!(rules[0].payload, "two.example");
    assert_eq!(rules[1].payload, "one.example");
    assert_eq!(owner.running.as_ref().unwrap().child.id(), pid);
    owner.script_logs(3).unwrap();
    assert!(owner.snapshot.script_log(3).unwrap().contains("执行完成"));
    let (expected, mut docs) = overrides::read(&paths).unwrap();
    let old = expected.clone();
    docs[1].content = "function main(c){throw new Error('private');}".into();
    assert!(
        owner
            .overrides(crate::core::OverrideChange::Save {
                expected,
                documents: docs
            })
            .await
            .is_err()
    );
    assert_eq!(overrides::read(&paths).unwrap().0, old);
    owner.refresh_rules().await.unwrap();
    assert_eq!(owner.snapshot.rules.entries[0].payload, "two.example");
    let dns=Values::from([("DnsOverrideEnabled".into(),"true".into()),("DnsYaml".into(),"dns: {enable: false, nameserver: ['9.9.9.9'], cache-algorithm: arc}\nhosts: {custom.example: 127.0.0.1}".into())]);
    owner.save_settings(SettingsKind::Dns, dns).await.unwrap();
    let (index, mut documents) = repository::read(&paths).unwrap();
    documents[0].content = documents[0].content.replace("1.1.1.1", "8.8.8.8");
    owner
        .profiles(crate::core::ProfileCommand::Save {
            expected: index,
            documents,
        })
        .await
        .unwrap();
    let saved = std::fs::read(paths.settings().join("dns.yaml")).unwrap();
    assert!(!paths.settings().join("dns-profiles.json").exists());
    owner.save_settings(SettingsKind::Sniffer,Values::from([("SnifferOverrideEnabled".into(),"true".into()),("SnifferYaml".into(),"sniffer: {enable: true, sniff: {HTTP: {ports: [80, '8080-8081'], override-destination: false}}, skip-domain: ['*.internal']}".into())])).await.unwrap();
    let config: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&owner.running.as_ref().unwrap().launch.composed).unwrap();
    assert_eq!(config["dns"]["cache-algorithm"], "arc");
    assert_eq!(
        config["sniffer"]["sniff"]["HTTP"]["override-destination"],
        false
    );
    assert_eq!(owner.running.as_ref().unwrap().child.id(), pid);
    let invalid = Values::from([(
        "DnsYaml".into(),
        "dns: {enable: true, listen: invalid-listener, nameserver: ['1.1.1.1']}".into(),
    )]);
    assert!(
        owner
            .save_settings(SettingsKind::Dns, invalid)
            .await
            .is_err()
    );
    assert_eq!(
        std::fs::read(paths.settings().join("dns.yaml")).unwrap(),
        saved
    );
    // The same global document applies across profiles; a scoped override wins only there.
    let (index, mut documents) = repository::read(&paths).unwrap();
    let mut second = Document::new(4, DocumentKind::Profile);
    second.content =
        "dns: {enable: false, nameserver: ['8.8.4.4']}\nrules: ['MATCH,DIRECT']".into();
    documents.push(second);
    owner
        .profiles(crate::core::ProfileCommand::Save {
            expected: index,
            documents,
        })
        .await
        .unwrap();
    let (expected, mut documents) = overrides::read(&paths).unwrap();
    let mut scoped = Document::new(5, DocumentKind::Yaml);
    scoped.scope_profile = Some(1);
    scoped.content = "dns: {nameserver: ['1.0.0.1']}".into();
    documents.push(scoped);
    owner
        .overrides(crate::core::OverrideChange::Save {
            expected,
            documents,
        })
        .await
        .unwrap();
    let config: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&owner.running.as_ref().unwrap().launch.composed).unwrap();
    assert_eq!(config["dns"]["nameserver"][0], "1.0.0.1");
    assert_eq!(config["dns"]["cache-algorithm"], "arc");
    owner.activate(4).await.unwrap();
    let config: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&owner.running.as_ref().unwrap().launch.composed).unwrap();
    assert_eq!(config["dns"]["nameserver"][0], "9.9.9.9");
    assert_eq!(config["hosts"]["custom.example"], "127.0.0.1");
    owner.stop().await.unwrap();
    owner.start().await.unwrap();
    let config: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&owner.running.as_ref().unwrap().launch.composed).unwrap();
    assert_eq!(config["dns"]["nameserver"][0], "9.9.9.9");
    owner
        .save_settings(
            SettingsKind::Dns,
            Values::from([("DnsOverrideEnabled".into(), "false".into())]),
        )
        .await
        .unwrap();
    let config: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&owner.running.as_ref().unwrap().launch.composed).unwrap();
    assert_eq!(config["dns"]["nameserver"][0], "8.8.4.4");
    assert_eq!(repository::read(&paths).unwrap().1.len(), 2);
    owner.stop().await.unwrap();
}

#[tokio::test]
async fn global_dns_saves_while_stopped_and_invalid_edits_preserve_committed_yaml() {
    use crate::{preferences::SettingsKind, storage::Values};
    let (_root, paths) = fixture(Some(SLEEPING_CORE));
    let mut owner = actor(paths.clone());
    owner
        .save_settings(
            SettingsKind::Dns,
            Values::from([
                ("DnsOverrideEnabled".into(), "true".into()),
                (
                    "DnsYaml".into(),
                    "dns: {enable: false, nameserver: ['9.9.9.9']}".into(),
                ),
            ]),
        )
        .await
        .unwrap();
    assert!(owner.running.is_none());
    let saved = std::fs::read(paths.settings().join("dns.yaml")).unwrap();
    assert!(!paths.settings().join("dns-profiles.json").exists());
    assert!(
        owner
            .save_settings(
                SettingsKind::Dns,
                Values::from([("DnsYaml".into(), "dns: {ipv6: wrong}".into()),])
            )
            .await
            .is_err()
    );
    assert!(owner.running.is_none());
    assert_eq!(
        std::fs::read(paths.settings().join("dns.yaml")).unwrap(),
        saved
    );
    for enabled in [true, false] {
        owner
            .save_settings(
                SettingsKind::Dns,
                Values::from([("DnsEnabled".into(), enabled.to_string())]),
            )
            .await
            .unwrap();
        let yaml: serde_yaml_ng::Value =
            serde_yaml_ng::from_slice(&std::fs::read(paths.settings().join("dns.yaml")).unwrap())
                .unwrap();
        assert_eq!(yaml["dns"]["enable"], enabled);
        assert!(yaml.get("override-enabled").is_none());
        let app: serde_json::Value =
            serde_json::from_slice(&std::fs::read(crate::preferences::app::path(&paths)).unwrap())
                .unwrap();
        assert_eq!(app["dns"]["dns_override_enabled"], true);
    }
    owner
        .save_settings(
            SettingsKind::Dns,
            Values::from([("DnsOverrideEnabled".into(), "false".into())]),
        )
        .await
        .unwrap();
    let repository = Repository::open(paths).unwrap();
    let values = crate::preferences::values(&repository, SettingsKind::Dns).unwrap();
    assert_eq!(values["DnsOverrideEnabled"], "false");
    assert_eq!(values["Nameserver"], "9.9.9.9");
}

#[tokio::test]
async fn stopped_native_settings_save_intent_but_system_proxy_requires_a_ready_core() {
    use crate::{preferences::SettingsKind, storage::Values};
    let (_root, paths) = fixture(None);
    let mut owner = actor(paths.clone());
    for (kind, key) in [
        (SettingsKind::Dns, "DnsOverrideEnabled"),
        (SettingsKind::Sniffer, "SnifferOverrideEnabled"),
        (SettingsKind::Tun, "IsTunEnabled"),
    ] {
        owner
            .save_settings(kind, Values::from([(key.into(), "true".into())]))
            .await
            .unwrap();
        let repository = Repository::open(paths.clone()).unwrap();
        assert_eq!(
            crate::preferences::values(&repository, kind).unwrap()[key],
            "true"
        );
        assert!(owner.running.is_none());
        assert_eq!(owner.snapshot.phase(), Phase::Stopped);
        assert_ne!(owner.snapshot.tun(), Some(true));
        owner
            .save_settings(kind, Values::from([(key.into(), "false".into())]))
            .await
            .unwrap();
        assert_eq!(
            crate::preferences::values(&repository, kind).unwrap()[key],
            "false"
        );
    }
    let error = owner
        .save_settings(
            SettingsKind::SysProxy,
            Values::from([("IsSystemProxyEnabled".into(), "true".into())]),
        )
        .await
        .unwrap_err();
    assert!(error.contains("「内核」"), "{error}");
    assert_eq!(
        crate::preferences::values(&Repository::open(paths).unwrap(), SettingsKind::SysProxy)
            .unwrap()["IsSystemProxyEnabled"],
        "false"
    );
}

#[tokio::test]
async fn stopped_system_proxy_edits_only_save_and_enabling_requires_readiness() {
    use crate::{preferences::SettingsKind, storage::Values};
    let (_root, paths) = fixture(Some(SLEEPING_CORE));
    let mut owner = actor(paths.clone());
    owner
        .save_settings(
            SettingsKind::SysProxy,
            Values::from([("SystemProxyHost".into(), "::1".into())]),
        )
        .await
        .unwrap();
    let repository = Repository::open(paths).unwrap();
    assert_eq!(
        crate::preferences::values(&repository, SettingsKind::SysProxy).unwrap()["SystemProxyHost"],
        "::1"
    );
    assert_eq!(owner.snapshot.system_proxy.enabled(), None);
    let error = owner
        .save_settings(
            SettingsKind::SysProxy,
            Values::from([("IsSystemProxyEnabled".into(), "true".into())]),
        )
        .await
        .unwrap_err();
    assert!(error.contains("未就绪"), "{error}");
    assert_eq!(
        crate::preferences::values(&repository, SettingsKind::SysProxy).unwrap()["IsSystemProxyEnabled"],
        "false"
    );
    assert!(owner.running.is_none());
}

#[tokio::test]
async fn proxy_observation_does_not_retry_failed_startup_intent() {
    let (_root, paths) = fixture(None);
    let repository = Repository::open(paths.clone()).unwrap();
    crate::preferences::initialize(&repository).unwrap();
    let namespace = crate::storage::Namespace::parse("sysproxy").unwrap();
    let mut values =
        crate::preferences::values(&repository, crate::preferences::SettingsKind::SysProxy)
            .unwrap();
    values.insert("IsSystemProxyEnabled".into(), "true".into());
    values.insert("SystemProxyModeIndex".into(), "pac".into());
    repository.save(&namespace, &values).unwrap();
    let mut owner = actor(paths);
    #[cfg(target_os = "macos")]
    owner.system_proxy.refresh_network().unwrap();
    // Startup can leave the core ready and OS application failed. Poll must only
    // observe; attempting to apply here would require a running child/API again.
    owner.snapshot.phase = Phase::Ready;
    for _ in 0..3 {
        owner.poll_system_proxy().await.unwrap();
    }
    assert_eq!(owner.snapshot.phase, Phase::Ready);
    assert_eq!(owner.snapshot.system_proxy.enabled(), Some(false));
    assert_eq!(
        crate::preferences::values(&repository, crate::preferences::SettingsKind::SysProxy)
            .unwrap()
            .get("IsSystemProxyEnabled")
            .map(String::as_str),
        Some("true")
    );
}

#[tokio::test]
async fn stopped_proxy_poll_only_observes_and_preserves_owner_transactions() {
    let (_root, paths) = fixture(None);
    let mut owner = actor(paths);
    let mut values = crate::preferences::defaults(crate::preferences::SettingsKind::SysProxy);
    values.insert("IsSystemProxyEnabled".into(), "true".into());
    owner.system_proxy.apply(&values, 7897).await.unwrap();
    owner.snapshot.phase = Phase::Stopped;
    for _ in 0..3 {
        owner.poll_system_proxy().await.unwrap();
    }
    assert_eq!(owner.snapshot.system_proxy.enabled(), Some(true));
    assert!(owner.system_proxy.is_current(&values, 7897));
    // Polling is not a lifecycle transition and must not consume this checkpoint.
    owner.system_proxy.rollback().await.unwrap();
    owner.mark_runtime_failed().await.unwrap();
    assert_eq!(owner.snapshot.phase, Phase::Failed);
    for _ in 0..3 {
        owner.poll_system_proxy().await.unwrap();
    }
    assert_eq!(owner.snapshot.system_proxy.enabled(), Some(false));
    assert!(!owner.system_proxy.is_current(&values, 7897));
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn offline_proxy_restore_is_deferred_and_reconnect_checks_kernel_readiness() {
    let (_root, paths) = fixture(None);
    let repository = Repository::open(paths.clone()).unwrap();
    crate::preferences::initialize(&repository).unwrap();
    let namespace = crate::storage::Namespace::parse("sysproxy").unwrap();
    let mut values =
        crate::preferences::values(&repository, crate::preferences::SettingsKind::SysProxy)
            .unwrap();
    values.insert("IsSystemProxyEnabled".into(), "true".into());
    repository.save(&namespace, &values).unwrap();
    let mut owner = actor(paths);
    owner.snapshot.phase = Phase::Ready;
    owner.system_proxy.network_probe = Some(|| None);
    owner.sync_system_proxy().await.unwrap();
    for _ in 0..3 {
        owner.poll_system_proxy().await.unwrap();
    }
    assert!(!owner.system_proxy.automatic_suspended());
    assert_eq!(owner.snapshot.system_proxy.enabled(), None);

    // Returning online must enter the readiness check rather than mark the
    // proxy enabled or write to an unconfirmed listener. No child exists here.
    owner.system_proxy.network_probe = Some(|| Some("wifi".into()));
    assert!(
        owner
            .poll_system_proxy()
            .await
            .unwrap_err()
            .contains("未就绪")
    );
    assert_eq!(
        crate::preferences::values(&repository, crate::preferences::SettingsKind::SysProxy,)
            .unwrap()["IsSystemProxyEnabled"],
        "true"
    );
}

#[tokio::test]
async fn gui_login_registration_reconciles_startup_and_explicit_same_value_save() {
    let (_root, paths) = fixture(None);
    let mut owner = actor(paths.clone());
    crate::desktop::autostart::apply(true).unwrap();
    owner
        .save_settings(
            crate::preferences::SettingsKind::General,
            crate::storage::Values::from([("AutoRun".into(), "false".into())]),
        )
        .await
        .unwrap();
    assert!(!crate::desktop::autostart::enabled().unwrap());
    assert_eq!(owner.snapshot.autostart, Some(Ok(false)));
    crate::desktop::autostart::apply(true).unwrap();
    // Missing core must not prevent coordinating the independent GUI login entry.
    assert!(owner.startup().await.is_err());
    assert!(!crate::desktop::autostart::enabled().unwrap());
    assert_eq!(owner.snapshot.autostart, Some(Ok(false)));
}

#[tokio::test]
async fn gist_credentials_are_isolated_keep_individual_secrets_and_reject_stale_config() {
    use crate::core::gist::{Command as Gist, Settings};
    use crate::profiles::{CredentialEdits, CredentialStore, SecretEdit};
    let (_root, paths) = fixture(None);
    let mut owner = actor(paths.clone());
    owner
        .gist(Gist::Configure {
            expected: Settings::default(),
            settings: Settings::default(),
            credentials: CredentialEdits {
                token: SecretEdit::Replace("test-gist-token".into()),
                age: SecretEdit::Keep,
            },
        })
        .await
        .unwrap();
    assert!(owner.snapshot.gist.token_saved);
    assert!(
        CredentialStore::new(&paths)
            .read(0)
            .await
            .unwrap()
            .0
            .is_empty()
    );
    let identity = age::x25519::Identity::generate();
    use age::secrecy::ExposeSecret as _;
    let settings = Settings {
        encrypt: true,
        recipient: identity.to_public().to_string(),
    };
    owner
        .gist(Gist::Configure {
            expected: Settings::default(),
            settings: settings.clone(),
            credentials: CredentialEdits {
                token: SecretEdit::Keep,
                age: SecretEdit::Replace(identity.to_string().expose_secret().to_owned()),
            },
        })
        .await
        .unwrap();
    let vault = CredentialStore::gist(&paths);
    assert_eq!(vault.read(0).await.unwrap().0, "test-gist-token");
    assert!(
        !std::fs::read_to_string(crate::preferences::app::path(&paths))
            .unwrap()
            .contains("test-gist-token")
    );
    assert!(
        owner
            .gist(Gist::Configure {
                expected: Settings::default(),
                settings: Settings::default(),
                credentials: CredentialEdits {
                    token: SecretEdit::Clear,
                    age: SecretEdit::Keep
                }
            })
            .await
            .unwrap_err()
            .contains("已变更")
    );
    assert_eq!(vault.read(0).await.unwrap().0, "test-gist-token");
    owner
        .gist(Gist::Configure {
            expected: settings.clone(),
            settings,
            credentials: CredentialEdits {
                token: SecretEdit::Clear,
                age: SecretEdit::Keep,
            },
        })
        .await
        .unwrap();
    assert!(vault.read(0).await.unwrap().0.is_empty());
    assert!(!vault.read(0).await.unwrap().1.is_empty());
}

#[tokio::test]
async fn gist_restore_rolls_back_files_and_credentials_on_cleanup_failure_and_stays_stopped() {
    use crate::profiles::{
        CredentialStore,
        model::{Document, DocumentKind},
        repository,
    };
    let (_root, paths) = fixture(None);
    let mut owner = actor(paths.clone());
    let mut doc = Document::new(77, DocumentKind::Profile);
    doc.name = "备份来源".into();
    doc.url = "https://backup.example/sub".into();
    doc.content = "proxies: []\nproxy-groups: []\nrules: [MATCH,DIRECT]\n".into();
    owner
        .profiles(crate::core::ProfileCommand::Save {
            expected: repository::read(&paths).unwrap().0,
            documents: vec![doc.clone()],
        })
        .await
        .unwrap();
    let archive = crate::backup::Archive::capture(&paths).unwrap();
    doc.url = "https://local.example/sub".into();
    doc.name = "本机来源".into();
    owner
        .profiles(crate::core::ProfileCommand::Save {
            expected: repository::read(&paths).unwrap().0,
            documents: vec![doc.clone()],
        })
        .await
        .unwrap();
    let vault = CredentialStore::new(&paths);
    vault.write(77, "local-token", "").await.unwrap();
    vault.fail_next_delete(77);
    assert!(owner.restore_archive(archive.clone()).await.is_err());
    assert_eq!(repository::read(&paths).unwrap().1[0].url, doc.url);
    assert_eq!(vault.read(77).await.unwrap().0, "local-token");
    owner.restore_archive(archive).await.unwrap();
    assert_eq!(
        repository::read(&paths).unwrap().1[0].url,
        "https://backup.example/sub"
    );
    assert!(vault.read(77).await.unwrap().0.is_empty());
    assert!(owner.running.is_none());
    assert_eq!(owner.snapshot.phase, Phase::Stopped);
    assert!(owner.snapshot.operation_committed);
}

#[tokio::test]
async fn local_backup_import_export_restore_use_the_complete_archive_without_starting_core() {
    use crate::core::local_backup::Command as Backup;
    let (root, paths) = fixture(None);
    let mut owner = actor(paths.clone());
    let original = crate::preferences::values(
        &crate::storage::Repository::open(paths.clone()).unwrap(),
        crate::preferences::SettingsKind::General,
    )
    .unwrap();
    owner.local_backup(Backup::Create).await.unwrap();
    let id = owner.snapshot.local_backup.entries[0].id.clone();
    let target = root.path().join("export.suki-backup");
    owner
        .local_backup(Backup::Export {
            id: id.clone(),
            target: target.clone(),
        })
        .await
        .unwrap();
    let mut values = crate::preferences::defaults(crate::preferences::SettingsKind::General);
    values.insert("ThemeIndex".into(), "浅色".into());
    crate::storage::Repository::open(paths.clone())
        .unwrap()
        .save(
            &crate::storage::Namespace::parse("settings").unwrap(),
            &values,
        )
        .unwrap();
    owner
        .local_backup(Backup::Restore(id.clone()))
        .await
        .unwrap();
    assert_eq!(
        crate::preferences::values(
            &crate::storage::Repository::open(paths.clone()).unwrap(),
            crate::preferences::SettingsKind::General
        )
        .unwrap(),
        original
    );
    assert!(owner.running.is_none());
    owner.local_backup(Backup::Delete(id)).await.unwrap();
    assert!(owner.snapshot.local_backup.entries.is_empty());
    owner.local_backup(Backup::Import(target)).await.unwrap();
    assert_eq!(owner.snapshot.local_backup.entries.len(), 1);
    let invalid = root.path().join("invalid.suki-backup");
    std::fs::write(&invalid, "bad backup").unwrap();
    assert!(owner.local_backup(Backup::Import(invalid)).await.is_err());
    assert_eq!(owner.snapshot.local_backup.entries.len(), 1);
    assert_eq!(
        crate::preferences::values(
            &crate::storage::Repository::open(paths.clone()).unwrap(),
            crate::preferences::SettingsKind::General
        )
        .unwrap(),
        original
    );
}

#[tokio::test]
async fn unmanaged_update_check_returns_manual_release_link_without_failing_core() {
    let (_root, paths) = fixture(None);
    let mut owner = actor(paths);
    let (_sender, shutdown) = tokio::sync::watch::channel(false);
    owner
        .app_update(super::super::app_update::Command::Check, shutdown)
        .await
        .unwrap();
    assert_eq!(owner.snapshot.phase, Phase::Stopped);
    assert!(owner.snapshot.error.is_none());
    assert_eq!(
        owner.snapshot.app_update.manual_url.as_deref(),
        Some(crate::updates::manual_releases_url().unwrap().as_str())
    );
    assert!(owner.running.is_none());
}
