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
    target_from_values(&values, 7897, "")
}
#[cfg(target_os = "macos")]
#[tokio::test]
async fn network_change_invalidates_target_but_keeps_write_failures_suspended() {
    let mut values = crate::preferences::defaults(crate::preferences::SettingsKind::SysProxy);
    values.insert("IsSystemProxyEnabled".into(), "true".into());
    values.insert("EnableProxyGuard".into(), "true".into());
    let mut controller = Controller::default();
    controller.apply(&values, 7897).await.unwrap();
    assert_eq!(
        controller.refresh_network().unwrap(),
        NetworkStatus::Unchanged
    );
    assert!(controller.is_current(&values, 7897));
    assert!(controller.guard_at.is_some());

    controller.network_probe = Some(|| None);
    assert_eq!(
        controller.refresh_network().unwrap(),
        NetworkStatus::Unavailable
    );
    assert!(!controller.is_current(&values, 7897));
    assert!(controller.guard_at.is_none());
    controller.network_probe = Some(|| Some("ethernet".into()));
    assert_eq!(
        controller.refresh_network().unwrap(),
        NetworkStatus::Changed
    );
    controller.apply_automatic(&values, 7897).await.unwrap();
    assert!(controller.is_current(&values, 7897));

    controller.automatic_suspended = true;
    controller.network_probe = Some(|| Some("wifi".into()));
    assert_eq!(
        controller.refresh_network().unwrap(),
        NetworkStatus::Changed
    );
    assert!(controller.automatic_suspended());
    assert_eq!(controller.poll(&values).await.unwrap().enabled(), None);
    // The native owner still remembers all writes after cache invalidation.
    assert_eq!(controller.clear().await.unwrap().enabled(), Some(false));
}
#[test]
fn default_bypass_combination_and_pac_readback_follow_saved_policy() {
    let mut values = crate::preferences::defaults(crate::preferences::SettingsKind::SysProxy);
    values.insert("IsSystemProxyEnabled".into(), "true".into());
    values.insert("BypassList".into(), "custom.test".into());
    let combined = target_from_values(&values, 7897, "");
    assert!(entries(&combined.system.bypass).contains("custom.test"));
    assert!(entries(&combined.system.bypass).len() > 1);
    values.insert("UseDefaultBypass".into(), "false".into());
    assert_eq!(
        target_from_values(&values, 7897, "").system.bypass,
        "custom.test"
    );
    values.insert("BypassList".into(), String::new());
    assert!(
        !target_from_values(&values, 7897, "")
            .system
            .bypass
            .is_empty()
    );
    values.insert("SystemProxyModeIndex".into(), "pac".into());
    let pac = target_from_values(&values, 7897, "http://127.0.0.1:1234/proxy.pac");
    let mut backend = Memory::default();
    backend.write(&pac).unwrap();
    assert!(pac.matches(&backend.state));
    backend.state.bypass = "wrong.test".into();
    assert!(!pac.matches(&backend.state));
}
#[tokio::test]
async fn failed_guard_stops_automatic_requests_until_an_explicit_apply() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let attempts = Arc::new(AtomicUsize::new(0));
    let observed = attempts.clone();
    let (sender, receiver) = async_channel::bounded::<Work>(2);
    let thread = std::thread::spawn(move || {
        while let Ok(work) = receiver.recv_blocking() {
            let index = observed.fetch_add(1, Ordering::SeqCst);
            let result = if index == 0 {
                Err("Permission denied（1003）".into())
            } else {
                let target = target();
                let mut backend = Memory::default();
                backend.write(&target).unwrap();
                Ok(Observation::read(&backend.state, Some(&target)))
            };
            let _ = work.reply.send(result);
        }
    });
    let mut controller = Controller {
        worker: Some(Worker {
            sender,
            thread: Some(thread),
        }),
        target: Some(target()),
        guard_at: Some(Instant::now()),
        ..Default::default()
    };
    let mut values = crate::preferences::defaults(crate::preferences::SettingsKind::SysProxy);
    values.insert("IsSystemProxyEnabled".into(), "true".into());
    values.insert("EnableProxyGuard".into(), "true".into());
    assert!(
        controller
            .poll(&values)
            .await
            .unwrap_err()
            .contains("Permission denied")
    );
    for _ in 0..10 {
        assert_eq!(controller.poll(&values).await.unwrap().enabled(), None);
    }
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    assert!(controller.automatic_suspended());
    assert_eq!(
        controller.apply(&values, 7897).await.unwrap().enabled(),
        Some(true)
    );
    assert!(!controller.automatic_suspended());
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
}
#[test]
fn native_failure_restores_last_confirmed_target() {
    let mut owner = Owner::new(Memory::default());
    let previous = target();
    owner.apply(previous.clone()).unwrap();
    owner.backend_mut().fail_next = true;
    let mut next = previous.clone();
    next.system.port = 7899;
    assert!(owner.apply(next).unwrap_err().contains("permission denied"));
    assert!(previous.matches(&owner.backend_mut().state));
    assert_eq!(owner.observe().unwrap().enabled(), Some(true));
}
#[test]
fn failed_disable_restores_the_enabled_observation() {
    let mut owner = Owner::new(Memory::default());
    owner.apply(target()).unwrap();
    owner.backend_mut().fail_next = true;
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
    let mut owner = Owner::new(Memory {
        ignore_next: true,
        ..Default::default()
    });
    assert!(owner.apply(target()).unwrap_err().contains("回读不一致"));
    assert!(owner.backend_mut().state.is_all_disabled());
    assert_eq!(owner.observe().unwrap().enabled(), Some(false));
}
#[test]
fn cleanup_preserves_another_app_proxy_and_clears_our_partial_write() {
    let mut owner = Owner::new(Memory::default());
    let own = target();
    owner.apply(own.clone()).unwrap();
    let mut other = own.clone();
    other.system.port = 8000;
    owner.backend_mut().write(&other).unwrap();
    owner.clear().unwrap();
    assert!(other.matches(&owner.backend_mut().state));
    owner.apply(own).unwrap();
    owner.backend_mut().state.https.switched_on = false;
    owner.clear().unwrap();
    assert!(owner.backend_mut().state.is_all_disabled());
}
#[test]
fn manual_pac_and_disabled_modes_require_exact_readback() {
    let mut owner = Owner::new(Memory::default());
    let mut pac = target();
    pac.system.enable = false;
    pac.auto = PacProxy {
        enable: true,
        url: "http://127.0.0.1:1234/proxy.pac".into(),
    };
    owner.apply(pac.clone()).unwrap();
    assert!(pac.matches(&owner.backend_mut().state));
    owner.backend_mut().state.http.switched_on = true;
    assert!(!pac.matches(&owner.backend_mut().state));
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
    let mut owner = Owner::new(external);
    assert!(owner.apply(target()).is_err());
    assert_eq!(owner.backend_mut().state, before);
}
#[test]
fn unchanged_target_uses_readback_without_rewriting_os_settings() {
    let mut owner = Owner::new(Memory::default());
    owner.apply(target()).unwrap();
    let writes = owner.backend_mut().writes;
    owner.apply(target()).unwrap();
    assert_eq!(owner.backend_mut().writes, writes);
}
#[tokio::test]
async fn cleanup_runs_after_a_request_whose_waiter_was_dropped() {
    let worker = Worker::new(None).unwrap();
    let (reply, receiver) = tokio::sync::oneshot::channel();
    worker
        .sender
        .send(Work {
            request: Request::Apply(target(), false),
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
    let mut owner = Owner::new(backend);
    owner.apply(target()).unwrap();
    owner.rollback().unwrap();
    assert_eq!(owner.backend_mut().state, external);
    assert!(owner.target().is_none());
}
