use super::*;
#[derive(Default)]
pub(super) struct Memory {
    state: OsSnapshot,
    fail_next: bool,
    ignore_next: bool,
    writes: usize,
}
impl Backend for Memory {
    type Checkpoint = OsSnapshot;
    fn capture(&mut self) -> Result<(OsSnapshot, Self::Checkpoint), String> {
        Ok((self.state.clone(), self.state.clone()))
    }
    fn read(&self) -> Result<OsSnapshot, String> {
        Ok(self.state.clone())
    }
    fn write(&mut self, target: &Target) -> Result<(), String> {
        self.writes += 1;
        if std::mem::take(&mut self.fail_next) {
            self.state.http.switched_on = true;
            self.state.http.port = 1;
            return Err("permission denied".into());
        }
        if std::mem::take(&mut self.ignore_next) {
            return Ok(());
        }
        let endpoint = || types::Endpoint {
            host: target.system.host.clone(),
            port: target.system.port,
            enable: target.system.enable,
            switched_on: target.system.enable,
        };
        self.state = OsSnapshot {
            http: endpoint(),
            https: endpoint(),
            socks: endpoint(),
            auto: target.auto.clone(),
            auto_switched_on: target.auto.enable,
            bypass: target.system.bypass.clone(),
            auto_detect: false,
        };
        Ok(())
    }
    fn restore(&mut self, snapshot: &OsSnapshot) -> Result<(), String> {
        self.state = snapshot.clone();
        Ok(())
    }
}
fn target() -> Target {
    let mut values = crate::preferences::defaults(crate::preferences::SettingsKind::SysProxy);
    values.insert("IsSystemProxyEnabled".into(), "true".into());
    Target::from_values(&values, 7897, "")
}
#[test]
fn native_failure_restores_last_confirmed_target() {
    let mut owner = Owner {
        backend: Memory::default(),
        target: None,
        rollback: None,
    };
    let previous = target();
    owner.apply(previous.clone()).unwrap();
    owner.backend.fail_next = true;
    let mut next = previous.clone();
    next.system.port = 7899;
    assert!(owner.apply(next).unwrap_err().contains("permission denied"));
    assert!(previous.matches(&owner.backend.state));
    assert_eq!(owner.observe().unwrap().enabled(), Some(true));
}
#[test]
fn failed_disable_restores_the_enabled_observation() {
    let mut owner = Owner {
        backend: Memory::default(),
        target: None,
        rollback: None,
    };
    owner.apply(target()).unwrap();
    owner.backend.fail_next = true;
    assert!(
        owner
            .apply(Target::default())
            .unwrap_err()
            .contains("已恢复上次系统代理")
    );
    assert_eq!(owner.observe().unwrap().enabled(), Some(true));
}
#[test]
fn silent_setter_failure_is_detected_and_never_published_as_success() {
    let mut owner = Owner {
        backend: Memory {
            ignore_next: true,
            ..Default::default()
        },
        target: None,
        rollback: None,
    };
    assert!(owner.apply(target()).unwrap_err().contains("回读不一致"));
    assert!(owner.backend.state.is_all_disabled());
    assert_eq!(owner.observe().unwrap().enabled(), Some(false));
}
#[test]
fn cleanup_preserves_another_app_proxy_and_clears_our_partial_write() {
    let mut owner = Owner {
        backend: Memory::default(),
        target: None,
        rollback: None,
    };
    let own = target();
    owner.apply(own.clone()).unwrap();
    let mut other = own.clone();
    other.system.port = 8000;
    owner.backend.write(&other).unwrap();
    owner.clear().unwrap();
    assert!(other.matches(&owner.backend.state));
    owner.apply(own).unwrap();
    owner.backend.state.https.switched_on = false;
    owner.clear().unwrap();
    assert!(owner.backend.state.is_all_disabled());
}
#[test]
fn manual_pac_and_disabled_modes_require_exact_readback() {
    let mut owner = Owner {
        backend: Memory::default(),
        target: None,
        rollback: None,
    };
    let mut pac = target();
    pac.system.enable = false;
    pac.auto = PacProxy {
        enable: true,
        url: "http://127.0.0.1:1234/proxy.pac".into(),
    };
    owner.apply(pac.clone()).unwrap();
    assert!(pac.matches(&owner.backend.state));
    owner.backend.state.http.switched_on = true;
    assert!(!pac.matches(&owner.backend.state));
    owner.apply(Target::default()).unwrap();
    assert_eq!(owner.observe().unwrap().enabled(), Some(false));
    assert_eq!(authority("::1", 7897), "[::1]:7897");
}
#[tokio::test]
async fn pac_is_served_over_loopback_and_updates_without_changing_url() {
    let server = pac::Server::start().await.unwrap();
    server.set("function FindProxyForURL() { return 'DIRECT'; }".into());
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let response = client.get(server.url()).send().await.unwrap();
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert!(response.text().await.unwrap().contains("DIRECT"));
    server.set("changed".into());
    assert_eq!(
        client
            .get(server.url())
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
        "changed"
    );
    assert_eq!(
        client
            .get(server.url().replace("proxy.pac", "other"))
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::NOT_FOUND
    );
}
#[tokio::test]
async fn guard_is_owned_and_stopping_cancels_future_reapply() {
    let mut controller = Controller::default();
    let mut values = crate::preferences::defaults(crate::preferences::SettingsKind::SysProxy);
    values.insert("IsSystemProxyEnabled".into(), "true".into());
    values.insert("EnableProxyGuard".into(), "true".into());
    controller.apply(&values, 7897).await.unwrap();
    controller.guard_at = Some(Instant::now());
    assert_eq!(
        controller.poll(&values).await.unwrap().enabled(),
        Some(true)
    );
    controller.clear().await.unwrap();
    assert!(controller.guard_at.is_none());
    assert_eq!(
        controller.poll(&values).await.unwrap().enabled(),
        Some(false)
    );
}

#[test]
fn first_failure_restores_an_external_proxy_with_different_protocol_endpoints() {
    let mut external = Memory::default();
    external.write(&target()).unwrap();
    external.state.http.port = 8001;
    external.state.https.port = 8002;
    external.state.socks.switched_on = false;
    external.fail_next = true;
    let before = external.state.clone();
    let mut owner = Owner {
        backend: external,
        target: None,
        rollback: None,
    };
    assert!(owner.apply(target()).is_err());
    assert_eq!(owner.backend.state, before);
}
#[test]
fn unchanged_target_uses_readback_without_rewriting_os_settings() {
    let mut owner = Owner {
        backend: Memory::default(),
        target: None,
        rollback: None,
    };
    owner.apply(target()).unwrap();
    let writes = owner.backend.writes;
    owner.apply(target()).unwrap();
    assert_eq!(owner.backend.writes, writes);
}
#[tokio::test]
async fn cleanup_runs_after_a_request_whose_waiter_was_dropped() {
    let worker = Worker::new().unwrap();
    let (reply, receiver) = tokio::sync::oneshot::channel();
    worker
        .sender
        .send(Work {
            request: Request::Apply(target()),
            reply,
        })
        .await
        .unwrap();
    drop(receiver);
    assert_eq!(
        worker.request(Request::Clear).await.unwrap().enabled(),
        Some(false)
    );
    assert_eq!(
        worker.request(Request::Observe).await.unwrap().enabled(),
        Some(false)
    );
}

#[test]
fn persistence_failure_can_restore_the_external_os_checkpoint_after_successful_apply() {
    let mut backend = Memory::default();
    backend.write(&target()).unwrap();
    backend.state.http.port = 8001;
    backend.state.https.port = 8002;
    backend.state.socks.switched_on = false;
    let external = backend.state.clone();
    let mut owner = Owner {
        backend,
        target: None,
        rollback: None,
    };
    owner.apply(target()).unwrap();
    owner.rollback().unwrap();
    assert_eq!(owner.backend.state, external);
    assert!(owner.target.is_none());
}
