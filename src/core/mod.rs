//! Owned Mihomo child lifecycle and private local API; only `bridge` depends on GPUI.
mod bridge;
mod delays;
mod icons;
mod log_stream;
mod monitor;
mod streams;
pub(crate) use monitor::Runtime;
pub(crate) mod app_update;
mod chain;
mod client;
mod connections;
mod download;
pub(crate) mod gist;
mod install;
pub(crate) mod local_backup;
mod proxy_control;
mod system_proxy;
pub(crate) use install::recover_pending;
mod interfaces;
pub(crate) use interfaces::Interface;
mod geo_sync;
mod launch;
mod output;
mod ownership;
mod port;
mod privileged;
mod provider_sync;
mod release;
mod resources;
mod runtime_bundle;
mod service_identity;
pub(crate) use crate::configuration::Preferences;
pub(crate) use release::ReleaseSelection;
pub(crate) use resources::{ProviderKind, Resources};
mod overrides;
mod profiles;
pub(crate) use overrides::Change as OverrideChange;
pub(crate) use profiles::ProfileCommand;
mod service;

pub(crate) use bridge::{
    TunServiceRequired, apply_app_update, attach_window, controller, execute, init,
    init_unavailable, quit, refresh_autostart, refresh_overview, request, reset_program,
    restart_program, submit, try_controller,
};
pub(crate) use privileged::{Action as ServiceAction, State as ServiceState};
pub(crate) use service::{Command, Service, ServiceError};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Phase {
    #[default]
    Stopped,
    Preparing,
    Starting,
    Ready,
    Stopping,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Recovery {
    TunService,
}

#[cfg(test)]
mod snapshot_tests {
    use super::{Phase, Snapshot};
    #[test]
    fn delayed_results_cannot_overwrite_a_newer_request_or_generation() {
        let pending = Snapshot {
            revision: 10,
            generation: 2,
            request_id: Some(4),
            phase: Phase::Starting,
            ..Snapshot::default()
        };
        let mut incoming = Snapshot {
            revision: 11,
            generation: 2,
            request_id: Some(3),
            phase: Phase::Ready,
            ..Snapshot::default()
        };
        assert!(!incoming.follows(&pending));
        incoming.request_id = Some(4);
        assert!(incoming.follows(&pending));
        incoming.generation = 1;
        assert!(!incoming.follows(&pending));
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TransferProgress {
    label: String,
    received: u64,
    total: Option<u64>,
}
impl TransferProgress {
    fn pending(label: &str) -> Self {
        Self {
            label: label.into(),
            received: 0,
            total: None,
        }
    }
    pub(crate) fn label(&self) -> &str {
        &self.label
    }
    pub(crate) fn received(&self) -> u64 {
        self.received
    }
    pub(crate) fn total(&self) -> Option<u64> {
        self.total
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub(crate) autostart: Option<Result<bool, String>>,
    revision: u64,
    pub(crate) gist: gist::State,
    pub(crate) local_backup: local_backup::State,
    pub(crate) app_update: crate::updates::State,
    pub(crate) proxy_preferences: crate::proxies::preferences::Preferences,
    generation: u64,
    request_id: Option<u64>,
    phase: Phase,
    pid: Option<u32>,
    version: Option<String>,
    core_installed: Option<bool>,
    tun: Option<bool>,
    service_mode: bool,
    service_state: privileged::State,
    uwp_apps: Vec<suki_service::windows_network::UwpApp>,
    error: Option<String>,
    error_logged: bool,
    recovery: Option<Recovery>,
    notice: Option<String>,
    profile: Option<crate::configuration::ProfileIdentity>,
    preferences: Option<Preferences>,
    settings: std::collections::BTreeMap<String, crate::storage::Values>,
    versions: Vec<String>,
    information: Option<String>,
    operation_active: bool,
    operation_committed: bool,
    operation_module: Option<&'static str>,
    progress: Option<TransferProgress>,
    resources: Resources,
    rules: std::sync::Arc<crate::traffic::rules::Snapshot>,
    script_log: Option<(u64, String)>,
    runtime_yaml: Option<String>,
    interfaces: Vec<Interface>,
    proxy_names: Vec<String>,
    monitor_epoch: u64,
    chain: crate::proxies::chain_config::Config,
    pub(crate) chain_sources:
        std::collections::BTreeMap<String, crate::proxies::chain_sources::Source>,
    system_proxy: crate::system_proxy::Observation,
}
impl Snapshot {
    pub(crate) fn uwp_apps(&self) -> &[suki_service::windows_network::UwpApp] {
        &self.uwp_apps
    }

    pub(crate) fn service_state(&self) -> &privileged::State {
        &self.service_state
    }
    pub(crate) fn tun(&self) -> Option<bool> {
        self.tun
    }
    pub(crate) fn runtime_yaml(&self) -> Option<&str> {
        self.runtime_yaml.as_deref()
    }
    pub(crate) fn system_proxy(&self) -> &crate::system_proxy::Observation {
        &self.system_proxy
    }
    pub(crate) fn config_epoch(&self) -> u64 {
        self.monitor_epoch
    }
    pub(crate) fn script_log(&self, id: u64) -> Option<&str> {
        self.script_log
            .as_ref()
            .filter(|(key, _)| *key == id)
            .map(|(_, text)| text.as_str())
    }
    pub(crate) fn rules(&self) -> &std::sync::Arc<crate::traffic::rules::Snapshot> {
        &self.rules
    }
    pub(crate) fn chain(&self) -> &crate::proxies::chain_config::Config {
        &self.chain
    }
    #[cfg(test)]
    pub(crate) fn ready_with_providers(kind: ProviderKind, bytes: &[u8]) -> Result<Self, String> {
        let mut snapshot = Self {
            phase: Phase::Ready,
            ..Default::default()
        };
        snapshot
            .resources
            .replace_providers(kind, resources::parse(bytes, kind)?);
        Ok(snapshot)
    }

    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }
    pub(crate) fn settings(&self, page: &str) -> Option<&crate::storage::Values> {
        self.settings.get(page)
    }
    pub(crate) fn resources(&self) -> &Resources {
        &self.resources
    }
    pub(crate) fn interfaces(&self) -> &[Interface] {
        &self.interfaces
    }
    pub(crate) fn proxy_names(&self) -> &[String] {
        &self.proxy_names
    }
    pub(crate) fn versions(&self) -> &[String] {
        &self.versions
    }
    pub(crate) fn request_id(&self) -> Option<u64> {
        self.request_id
    }
    pub(crate) fn preferences(&self) -> Option<&Preferences> {
        self.preferences.as_ref()
    }
    pub(crate) fn profile(&self) -> Option<&crate::configuration::ProfileIdentity> {
        self.profile.as_ref()
    }
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }
    pub(crate) fn phase(&self) -> Phase {
        self.phase
    }
    pub(crate) fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }
    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
    pub(crate) fn operation_committed(&self) -> bool {
        self.operation_committed
    }
    pub(crate) fn is_busy(&self) -> bool {
        self.operation_active
            || matches!(
                self.phase,
                Phase::Preparing | Phase::Starting | Phase::Stopping
            )
    }
    pub(crate) fn has_process(&self) -> bool {
        self.pid.is_some()
    }
    fn follows(&self, previous: &Self) -> bool {
        self.revision > previous.revision
            && self.generation >= previous.generation
            && self.request_id >= previous.request_id
    }
    pub(crate) fn label(&self) -> &'static str {
        if self.core_installed == Some(false)
            && self.pid.is_none()
            && matches!(self.phase, Phase::Stopped | Phase::Failed)
        {
            return "不存在内核";
        }
        match self.phase {
            Phase::Stopped => "已停止",
            Phase::Preparing => "准备运行资源",
            Phase::Starting => "启动中",
            Phase::Ready => "运行中",
            Phase::Stopping => "停止中",
            Phase::Failed => "异常",
        }
    }
}

pub(crate) fn test_delay(
    test: crate::proxies::runtime::Test,
    window: &mut gpui_kit::Window,
    cx: &mut gpui_kit::App,
) {
    let result = controller(cx).update(cx, |owner, _| owner.service_test_delay(test));
    if let Err(error) = result {
        crate::ui::show_error(error, window, cx);
    }
}

#[cfg(all(windows, not(test)))]
pub(crate) use service_identity::current_owner_identity as service_identity_for_autostart;
