//! OS writes have one thread owner. Dropping a request never cancels an in-flight
//! native write: cleanup is queued after it, including during application quit.
// Ordinary tests compile the real adapter and its pure tests, while routing OS writes to Memory.
#[cfg_attr(test, allow(dead_code))]
mod native;
mod pac;
#[cfg(test)]
mod tests;
use crate::storage::Values;
use std::time::{Duration, Instant};
mod types;
#[cfg(test)]
use types::OsSnapshot;
use types::{ManualProxy, PacProxy};

#[cfg(target_os = "linux")]
use suki_service::proxy_owner::clear_current;
#[cfg(any(test, target_os = "linux"))]
use suki_service::proxy_owner::entries;
use suki_service::proxy_owner::{Backend, Owner};
pub(crate) use suki_service::proxy_owner::{Observation, Target};
pub(super) fn authority(host: &str, port: u16) -> String {
    if host.contains(':') {
        format!("[{}]:{port}", host.trim_matches(['[', ']']))
    } else {
        format!("{host}:{port}")
    }
}
fn target_from_values(values: &Values, port: u16, pac_url: &str) -> Target {
    let enabled = values
        .get("IsSystemProxyEnabled")
        .is_some_and(|v| v == "true");
    let auto = enabled
        && values
            .get("SystemProxyModeIndex")
            .is_some_and(|v| v == "pac");
    let custom_bypass = values
        .get("BypassList")
        .map(String::as_str)
        .unwrap_or_default();
    let bypass = if custom_bypass.trim().is_empty() {
        crate::preferences::schema::default_bypass().to_owned()
    } else if values
        .get("UseDefaultBypass")
        .is_none_or(|value| value != "false")
    {
        format!(
            "{}\n{custom_bypass}",
            crate::preferences::schema::default_bypass()
        )
    } else {
        custom_bypass.to_owned()
    };
    Target {
        system: ManualProxy {
            enable: enabled && !auto,
            host: values
                .get("SystemProxyHost")
                .cloned()
                .unwrap_or_default()
                .trim_matches(['[', ']'])
                .into(),
            port,
            bypass: crate::preferences::schema::split_entries(&bypass)
                .collect::<Vec<_>>()
                .join(if cfg!(windows) { ";" } else { "," }),
        },
        auto: PacProxy {
            enable: auto,
            url: if auto { pac_url.into() } else { String::new() },
        },
    }
}
enum Request {
    Apply(Target, bool),
    Observe,
    Clear,
    Rollback,
}
struct Work {
    request: Request,
    reply: tokio::sync::oneshot::Sender<Result<Observation, String>>,
}
struct Worker {
    sender: async_channel::Sender<Work>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Worker {
    fn new(paths: Option<crate::storage::AppPaths>) -> Result<Self, String> {
        let (sender, receiver) = async_channel::bounded::<Work>(2);
        let thread = std::thread::Builder::new()
            .name("suki-system-proxy".into())
            .spawn(move || {
                #[cfg(not(test))]
                let initialize = |interactive| native::Native::new(paths.as_ref(), interactive);
                #[cfg(test)]
                let initialize = |_: bool| {
                    let _ = &paths;
                    Ok::<_, String>(tests::Memory::default())
                };
                let initialize = |interactive| initialize(interactive).map(Owner::new);
                let mut owner = None;
                while let Ok(work) = receiver.recv_blocking() {
                    let interactive = matches!(&work.request, Request::Apply(_, true));
                    let initialized = owner.is_none()
                        || (owner.as_ref().is_some_and(Result::is_err)
                            && matches!(&work.request, Request::Apply(..) | Request::Observe));
                    if initialized {
                        owner = Some(initialize(interactive));
                    }
                    let result = match owner.as_mut() {
                        Some(Ok(owner)) => match work.request {
                            Request::Apply(target, interactive) => {
                                if !initialized {
                                    owner.backend_mut().allow_interaction(interactive);
                                }
                                let result = owner.apply(target);
                                owner.backend_mut().allow_interaction(false);
                                result
                            }
                            Request::Observe => owner.observe(),
                            Request::Clear => owner.clear(),
                            Request::Rollback => owner.rollback(),
                        },
                        Some(Err(error)) => Err(error.clone()),
                        None => Err("系统代理执行端未初始化".into()),
                    };
                    let _ = work.reply.send(result);
                }
                if let Some(Ok(owner)) = &mut owner
                    && let Err(error) = owner.clear()
                {
                    log::error!("退出时清理系统代理失败：{error}");
                }
            })
            .map_err(|error| format!("无法启动系统代理执行端：{error}"))?;
        Ok(Self {
            sender,
            thread: Some(thread),
        })
    }
    async fn request(&self, request: Request) -> Result<Observation, String> {
        let (reply, receiver) = tokio::sync::oneshot::channel();
        self.sender
            .send(Work { request, reply })
            .await
            .map_err(|error| crate::logging::diagnostic_error("系统代理执行端已停止", error))?;
        receiver
            .await
            .map_err(|error| crate::logging::diagnostic_error("系统代理执行端异常退出", error))?
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.sender.close();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
#[derive(Default)]
pub(crate) struct Controller {
    #[cfg(target_os = "macos")]
    network_service: Option<String>,
    #[cfg(all(test, target_os = "macos"))]
    pub(crate) network_probe: Option<fn() -> Option<String>>,
    #[cfg(target_os = "macos")]
    service: Option<(
        suki_service::OwnerCredentials,
        suki_service::OwnerSessionProof,
    )>,
    worker: Option<Worker>,
    paths: Option<crate::storage::AppPaths>,
    pac: Option<pac::Server>,
    guard_at: Option<Instant>,
    target: Option<Target>,
    rollback: Option<(Option<Target>, Option<Instant>, Option<String>)>,
    automatic_suspended: bool,
}
#[cfg(target_os = "macos")]
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum NetworkStatus {
    Unavailable,
    Changed,
    Unchanged,
}
impl Controller {
    pub(crate) fn new(paths: crate::storage::AppPaths) -> Self {
        Self {
            paths: Some(paths),
            ..Default::default()
        }
    }
    #[cfg(target_os = "macos")]
    pub(crate) fn refresh_network(&mut self) -> Result<NetworkStatus, String> {
        #[cfg(not(test))]
        let current = suki_service::macos_proxy::active_service_id()?;
        #[cfg(test)]
        let current = self
            .network_probe
            .unwrap_or(|| Some("test-network".to_owned()))();
        Ok(self.update_network(current))
    }
    #[cfg(target_os = "macos")]
    fn update_network(&mut self, current: Option<String>) -> NetworkStatus {
        let changed = self.network_service != current;
        if changed {
            // The cached target and guard refer to the previous network service.
            // Native ownership checkpoints remain in the worker for cleanup.
            self.network_service = current;
            self.target = None;
            self.guard_at = None;
        }
        if self.network_service.is_none() {
            NetworkStatus::Unavailable
        } else if changed {
            NetworkStatus::Changed
        } else {
            NetworkStatus::Unchanged
        }
    }
    /// Switch execution only after the previous proxy owner has completed cleanup.
    #[cfg(target_os = "macos")]
    pub(crate) async fn service_session(
        &mut self,
        session: Option<(
            suki_service::OwnerCredentials,
            suki_service::OwnerSessionProof,
        )>,
    ) -> Result<(), String> {
        if self.service == session {
            return Ok(());
        }
        self.clear().await?;
        self.worker = None;
        self.service = session;
        self.automatic_suspended = false;
        Ok(())
    }
    /// Called after this controller was cleared and its service core was confirmed stopped.
    #[cfg(target_os = "macos")]
    pub(crate) fn detach_service_session(&mut self) {
        self.service = None;
        self.target = None;
        self.rollback = None;
        self.guard_at = None;
        self.automatic_suspended = false;
    }
    pub(crate) fn is_current(&self, values: &Values, port: u16) -> bool {
        let url = self.pac.as_ref().map(|s| s.url()).unwrap_or_default();
        self.target.as_ref() == Some(&target_from_values(values, port, &url))
    }
    async fn request(&mut self, request: Request) -> Result<Observation, String> {
        #[cfg(all(target_os = "macos", not(test)))]
        if let Some((credentials, session)) = &self.service {
            use suki_service::proxy_owner::Operation;
            let operation = match request {
                Request::Apply(target, _) => Operation::Apply(target),
                Request::Observe => Operation::Observe,
                Request::Rollback => Operation::Rollback,
                Request::Clear => Operation::Clear,
            };
            let result = match suki_service::set_system_proxy(credentials, session, &operation)
                .await
            {
                Ok(reply) => suki_service::response_data(reply).map_err(|error| error.to_string()),
                Err(error) => Err(error.to_string()),
            };
            if result.is_err() {
                self.automatic_suspended = true;
                self.guard_at = None;
            }
            return result;
        }

        if self.worker.is_none() {
            self.worker = Some(Worker::new(self.paths.clone())?);
        }
        let result = self
            .worker
            .as_ref()
            .ok_or("系统代理执行端不存在")?
            .request(request)
            .await;
        if result.is_err() {
            self.automatic_suspended = true;
            self.guard_at = None;
        }
        result
    }
    pub(crate) fn automatic_suspended(&self) -> bool {
        self.automatic_suspended
    }
    pub(crate) async fn observe(&mut self) -> Result<Observation, String> {
        self.request(Request::Observe).await
    }
    pub(crate) async fn apply(
        &mut self,
        values: &Values,
        port: u16,
    ) -> Result<Observation, String> {
        self.apply_with(values, port, true).await
    }
    pub(crate) async fn apply_automatic(
        &mut self,
        values: &Values,
        port: u16,
    ) -> Result<Observation, String> {
        self.apply_with(values, port, false).await
    }
    async fn apply_with(
        &mut self,
        values: &Values,
        port: u16,
        interactive: bool,
    ) -> Result<Observation, String> {
        #[cfg(target_os = "macos")]
        self.refresh_network()?;
        self.automatic_suspended = false;
        let auto = values
            .get("SystemProxyModeIndex")
            .is_some_and(|v| v == "pac")
            && values
                .get("IsSystemProxyEnabled")
                .is_some_and(|v| v == "true");
        if auto && self.pac.is_none() {
            self.pac = Some(pac::Server::start().await?);
        }
        let url = self.pac.as_ref().map(|s| s.url()).unwrap_or_default();
        let target = target_from_values(values, port, &url);
        let previous_script = self.pac.as_ref().map(|s| s.contents());
        if auto {
            let host = authority(&target.system.host, port);
            let script = values
                .get("PacScript")
                .ok_or("PAC 脚本缺失")?
                .replace("%proxy_host%:%mixed-port%", &host)
                .replace("%proxy_host%", &target.system.host)
                .replace("%mixed-port%", &port.to_string());
            self.pac.as_ref().ok_or("PAC 服务未启动")?.set(script);
        }
        let previous_guard = self.guard_at.take();
        let observed = match self
            .request(Request::Apply(target.clone(), interactive))
            .await
        {
            Ok(observed) => observed,
            Err(error) => {
                if let (Some(server), Some(script)) = (&self.pac, previous_script) {
                    server.set(script);
                }
                return Err(error);
            }
        };
        log::info!(
            "系统代理已写入并回读确认，端口 {port}，模式 {}",
            if auto { "PAC" } else { "手动" }
        );
        self.rollback = Some((self.target.clone(), previous_guard, previous_script));
        self.target = Some(target);
        if values.get("EnableProxyGuard").is_some_and(|v| v == "true")
            && values
                .get("IsSystemProxyEnabled")
                .is_some_and(|v| v == "true")
        {
            self.guard_at = Some(Instant::now() + guard_duration(values));
        }
        Ok(observed)
    }
    pub(crate) async fn rollback(&mut self) -> Result<Observation, String> {
        let observed = self.request(Request::Rollback).await?;
        log::info!("系统代理已回滚到操作前的状态");
        if let Some((target, guard, script)) = self.rollback.take() {
            self.target = target;
            self.guard_at = guard;
            if let (Some(server), Some(script)) = (&self.pac, script) {
                server.set(script);
            }
        }
        Ok(observed)
    }
    pub(crate) async fn poll(&mut self, values: &Values) -> Result<Observation, String> {
        if self.automatic_suspended {
            return Ok(Observation::default());
        }
        if self.guard_at.is_some_and(|at| Instant::now() >= at) {
            self.guard_at = Some(Instant::now() + guard_duration(values));
            if let Some(target) = self.target.clone() {
                log::debug!("系统代理守卫到期，重新写入系统代理");
                return self.request(Request::Apply(target, false)).await;
            }
        }
        self.observe().await
    }
    pub(crate) async fn clear(&mut self) -> Result<Observation, String> {
        self.rollback = None;
        self.guard_at = None;
        self.target = None;
        #[cfg(target_os = "macos")]
        let remote = self.service.is_some();
        #[cfg(not(target_os = "macos"))]
        let remote = false;
        if self.worker.is_none() && !remote {
            return Ok(Observation::default());
        }
        let observed = self.request(Request::Clear).await?;
        log::info!("系统代理已清除");
        Ok(observed)
    }
}
fn guard_duration(values: &Values) -> Duration {
    Duration::from_secs(
        values
            .get("ProxyGuardDuration")
            .and_then(|v| v.parse().ok())
            .unwrap_or(10),
    )
}
