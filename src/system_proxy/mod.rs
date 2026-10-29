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
use types::{ManualProxy, OsSnapshot, PacProxy};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Observation {
    enabled: Option<bool>,
}
impl Observation {
    pub(crate) fn enabled(&self) -> Option<bool> {
        self.enabled
    }
    fn read(state: &OsSnapshot, target: Option<&Target>) -> Self {
        Self {
            enabled: Some(
                !state.is_all_disabled() && target.is_some_and(|target| target.matches(state)),
            ),
        }
    }
}
pub(super) fn authority(host: &str, port: u16) -> String {
    if host.contains(':') {
        format!("[{}]:{port}", host.trim_matches(['[', ']']))
    } else {
        format!("{host}:{port}")
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Target {
    system: ManualProxy,
    auto: PacProxy,
}
impl Target {
    fn from_values(values: &Values, port: u16, pac_url: &str) -> Self {
        let enabled = values
            .get("IsSystemProxyEnabled")
            .is_some_and(|v| v == "true");
        let auto = enabled
            && values
                .get("SystemProxyModeIndex")
                .is_some_and(|v| v == "pac");
        let bypass = values
            .get("BypassList")
            .map(String::as_str)
            .unwrap_or_default();
        Self {
            system: ManualProxy {
                enable: enabled && !auto,
                host: values
                    .get("SystemProxyHost")
                    .cloned()
                    .unwrap_or_default()
                    .trim_matches(['[', ']'])
                    .into(),
                port,
                bypass: crate::preferences::schema::split_entries(bypass)
                    .collect::<Vec<_>>()
                    .join(if cfg!(windows) { ";" } else { "," }),
            },
            auto: PacProxy {
                enable: auto,
                url: if auto { pac_url.into() } else { String::new() },
            },
        }
    }
    fn matches(&self, state: &OsSnapshot) -> bool {
        if self.auto.enable {
            state.matches_pac(&self.auto)
        } else if self.system.enable {
            let endpoint = |e: &types::Endpoint| {
                e.switched_on && e.host == self.system.host && e.port == self.system.port
            };
            endpoint(&state.http)
                && endpoint(&state.https)
                && endpoint(&state.socks)
                && !state.auto_switched_on
                && !state.auto_detect
                && entries(&state.bypass) == entries(&self.system.bypass)
        } else {
            state.is_all_disabled()
        }
    }
    // Cleanup must never disable a proxy another application has installed.
    fn owns(&self, state: &OsSnapshot) -> bool {
        let mut found = false;
        for e in [&state.http, &state.https, &state.socks] {
            if e.switched_on {
                if e.host != self.system.host || e.port != self.system.port {
                    return false;
                }
                found = true;
            }
        }
        if state.auto_switched_on {
            if !self.auto.enable || state.auto.url != self.auto.url {
                return false;
            }
            found = true;
        }
        found
    }
}
fn entries(value: &str) -> std::collections::BTreeSet<&str> {
    value
        .split([',', ';'])
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .collect()
}
trait Backend: Send + 'static {
    type Checkpoint;
    fn capture(&mut self) -> Result<(OsSnapshot, Self::Checkpoint), String>;
    fn read(&self) -> Result<OsSnapshot, String>;
    fn write(&mut self, target: &Target) -> Result<(), String>;
    fn restore(&mut self, checkpoint: &Self::Checkpoint) -> Result<(), String>;
    fn clear(&mut self, target: &Target) -> Result<OsSnapshot, String> {
        clear_current(self, target)
    }
}
struct Owner<B: Backend> {
    backend: B,
    target: Option<Target>,
    rollback: Option<(OsSnapshot, B::Checkpoint, Option<Target>)>,
}
impl<B: Backend> Owner<B> {
    fn apply(&mut self, target: Target) -> Result<Observation, String> {
        let (before, checkpoint) = self.backend.capture()?;
        if target.matches(&before) {
            let observed = Observation::read(&before, Some(&target));
            let previous = self.target.replace(target);
            self.rollback = Some((before, checkpoint, previous));
            return Ok(observed);
        }
        let previous = self.target.replace(target.clone());
        let result = self.backend.write(&target).and_then(|()| {
            let state = self.backend.read()?;
            if target.matches(&state) {
                Ok(Observation::read(&state, Some(&target)))
            } else {
                Err("系统代理写入后回读不一致".into())
            }
        });
        match result {
            Ok(observed) => {
                self.rollback = Some((before, checkpoint, previous));
                Ok(observed)
            }
            Err(cause) => {
                let unchanged = self
                    .backend
                    .read()
                    .is_ok_and(|state| equivalent(&before, &state));
                let recovery = (if unchanged {
                    Ok(())
                } else {
                    self.backend.restore(&checkpoint)
                })
                .and_then(|()| {
                    if equivalent(&before, &self.backend.read()?) {
                        Ok(())
                    } else {
                        Err("恢复后的系统代理回读不一致".into())
                    }
                });
                match recovery {
                    Ok(()) => {
                        self.target = previous;
                        Err(format!(
                            "{cause}；{}",
                            if self.target.is_some() {
                                "已恢复上次系统代理"
                            } else {
                                "已恢复操作前的系统代理"
                            }
                        ))
                    }
                    Err(error) => Err(format!(
                        "{cause}；系统代理恢复失败：{error}；实际状态待确认"
                    )),
                }
            }
        }
    }
    fn observe(&self) -> Result<Observation, String> {
        Ok(Observation::read(
            &self.backend.read()?,
            self.target.as_ref(),
        ))
    }
    fn rollback(&mut self) -> Result<Observation, String> {
        let Some((before, checkpoint, target)) = self.rollback.take() else {
            return Err("没有待恢复的系统代理操作".into());
        };
        self.backend.restore(&checkpoint)?;
        let observed = self.backend.read()?;
        if !equivalent(&before, &observed) {
            return Err("恢复后的系统代理回读不一致".into());
        }
        self.target = target;
        Ok(Observation::read(&observed, self.target.as_ref()))
    }
    fn clear(&mut self) -> Result<Observation, String> {
        self.rollback = None;
        let Some(target) = &self.target else {
            return Ok(Observation::default());
        };
        let observed = self.backend.clear(target)?;
        self.target = None;
        Ok(Observation::read(&observed, None))
    }
}
fn clear_current<B: Backend + ?Sized>(
    backend: &mut B,
    target: &Target,
) -> Result<OsSnapshot, String> {
    let state = backend.read()?;
    if target.owns(&state) {
        let mut off = target.clone();
        off.system.enable = false;
        off.system.bypass = state.bypass;
        off.auto.enable = false;
        backend.write(&off)?;
        if !backend.read()?.is_all_disabled() {
            return Err("系统代理清理后回读不一致；实际状态待确认".into());
        }
    }
    backend.read()
}
fn equivalent(a: &OsSnapshot, b: &OsSnapshot) -> bool {
    let endpoint = |a: &types::Endpoint, b: &types::Endpoint| {
        a.switched_on == b.switched_on && (!a.switched_on || (a.host == b.host && a.port == b.port))
    };
    endpoint(&a.http, &b.http)
        && endpoint(&a.https, &b.https)
        && endpoint(&a.socks, &b.socks)
        && a.auto_detect == b.auto_detect
        && a.auto_switched_on == b.auto_switched_on
        && (!a.auto_switched_on || a.auto.url == b.auto.url)
        && entries(&a.bypass) == entries(&b.bypass)
}
enum Request {
    Apply(Target),
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
    fn new() -> Result<Self, String> {
        let (sender, receiver) = async_channel::bounded::<Work>(2);
        let thread = std::thread::Builder::new()
            .name("suki-system-proxy".into())
            .spawn(move || {
                #[cfg(not(test))]
                let backend = native::Native::default();
                #[cfg(test)]
                let backend = tests::Memory::default();
                let mut owner = Owner {
                    backend,
                    target: None,
                    rollback: None,
                };
                while let Ok(work) = receiver.recv_blocking() {
                    let result = match work.request {
                        Request::Apply(target) => owner.apply(target),
                        Request::Observe => owner.observe(),
                        Request::Clear => owner.clear(),
                        Request::Rollback => owner.rollback(),
                    };
                    let _ = work.reply.send(result);
                }
                if let Err(error) = owner.clear() {
                    eprintln!("系统代理退出清理失败：{error}");
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
            .map_err(|_| "系统代理执行端已停止")?;
        receiver.await.map_err(|_| "系统代理执行端异常退出")?
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
    worker: Option<Worker>,
    pac: Option<pac::Server>,
    guard_at: Option<Instant>,
    target: Option<Target>,
    rollback: Option<(Option<Target>, Option<Instant>, Option<String>)>,
}
impl Controller {
    pub(crate) fn is_current(&self, values: &Values, port: u16) -> bool {
        let url = self.pac.as_ref().map(|s| s.url()).unwrap_or_default();
        self.target.as_ref() == Some(&Target::from_values(values, port, &url))
    }
    async fn request(&mut self, request: Request) -> Result<Observation, String> {
        if self.worker.is_none() {
            self.worker = Some(Worker::new()?);
        }
        self.worker
            .as_ref()
            .ok_or("系统代理执行端不存在")?
            .request(request)
            .await
    }
    pub(crate) async fn observe(&mut self) -> Result<Observation, String> {
        self.request(Request::Observe).await
    }
    pub(crate) async fn apply(
        &mut self,
        values: &Values,
        port: u16,
    ) -> Result<Observation, String> {
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
        let target = Target::from_values(values, port, &url);
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
        let observed = match self.request(Request::Apply(target.clone())).await {
            Ok(observed) => observed,
            Err(error) => {
                self.guard_at = previous_guard;
                if let (Some(server), Some(script)) = (&self.pac, previous_script) {
                    server.set(script);
                }
                return Err(error);
            }
        };
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
        if self.guard_at.is_some_and(|at| Instant::now() >= at) {
            self.guard_at = Some(Instant::now() + guard_duration(values));
            if let Some(target) = self.target.clone() {
                return self.request(Request::Apply(target)).await;
            }
        }
        self.observe().await
    }
    pub(crate) async fn clear(&mut self) -> Result<Observation, String> {
        self.rollback = None;
        self.guard_at = None;
        self.target = None;
        if self.worker.is_none() {
            return Ok(Observation::default());
        }
        self.request(Request::Clear).await
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
