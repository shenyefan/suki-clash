//! Single-owner proxy transactions shared by the application and privileged service.
pub mod types;
use types::{ManualProxy, OsSnapshot, PacProxy};
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Observation {
    enabled: Option<bool>,
}
impl Observation {
    pub fn enabled(&self) -> Option<bool> {
        self.enabled
    }
    pub fn read(state: &OsSnapshot, target: Option<&Target>) -> Self {
        Self {
            enabled: Some(
                !state.is_all_disabled() && target.is_some_and(|target| target.matches(state)),
            ),
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Target {
    pub system: ManualProxy,
    pub auto: PacProxy,
}
impl Target {
    pub fn matches(&self, state: &OsSnapshot) -> bool {
        if self.auto.enable {
            state.matches_pac(&self.auto) && entries(&state.bypass) == entries(&self.system.bypass)
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
    pub fn owns(&self, state: &OsSnapshot) -> bool {
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
pub fn entries(value: &str) -> std::collections::BTreeSet<&str> {
    value
        .split([',', ';'])
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .collect()
}
// Native backends are created and used inside their serial worker. Requests
// cross threads; OS authorization handles stay with the backend's owner.
pub trait Backend: 'static {
    type Checkpoint;
    fn allow_interaction(&mut self, _allowed: bool) {}
    fn available(&self) -> Result<bool, String> {
        Ok(true)
    }
    fn matches(&self, target: &Target, state: &OsSnapshot) -> Result<bool, String> {
        Ok(target.matches(state))
    }
    fn checkpoint_matches(
        &self,
        checkpoint: &Self::Checkpoint,
        before: &OsSnapshot,
    ) -> Result<bool, String> {
        let _ = checkpoint;
        Ok(equivalent(before, &self.read()?))
    }
    fn capture(&mut self) -> Result<(OsSnapshot, Self::Checkpoint), String>;
    fn read(&self) -> Result<OsSnapshot, String>;
    fn write(&mut self, target: &Target) -> Result<(), String>;
    fn restore(&mut self, checkpoint: &Self::Checkpoint) -> Result<(), String>;
    fn clear(&mut self, target: &Target) -> Result<OsSnapshot, String> {
        clear_current(self, target)
    }
}
pub struct Owner<B: Backend> {
    backend: B,
    target: Option<Target>,
    rollback: Option<(OsSnapshot, B::Checkpoint, Option<Target>)>,
}
impl<B: Backend> Owner<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            target: None,
            rollback: None,
        }
    }
    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }
    pub fn target(&self) -> Option<&Target> {
        self.target.as_ref()
    }

    pub fn apply(&mut self, target: Target) -> Result<Observation, String> {
        if !self.backend.available()? {
            if target.system.enable || target.auto.enable {
                return Err("没有活动网络服务，无法开启系统代理".into());
            }
            self.backend
                .clear(self.target.as_ref().unwrap_or(&target))?;
            self.target = None;
            self.rollback = None;
            return Ok(Observation::default());
        }
        let (before, checkpoint) = self.backend.capture()?;
        if self.backend.matches(&target, &before)? {
            let observed = Observation::read(&before, Some(&target));
            let previous = self.target.replace(target);
            self.rollback = Some((before, checkpoint, previous));
            return Ok(observed);
        }
        let previous = self.target.replace(target.clone());
        let result = self.backend.write(&target).and_then(|()| {
            let state = self.backend.read()?;
            if self.backend.matches(&target, &state)? {
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
                    .checkpoint_matches(&checkpoint, &before)
                    .unwrap_or(false);
                let recovery = (if unchanged {
                    Ok(())
                } else {
                    self.backend.restore(&checkpoint)
                })
                .and_then(|()| {
                    if self.backend.checkpoint_matches(&checkpoint, &before)? {
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
    pub fn observe(&self) -> Result<Observation, String> {
        if !self.backend.available()? {
            return Ok(Observation::default());
        }
        let state = self.backend.read()?;
        let matches = match &self.target {
            Some(target) => self.backend.matches(target, &state)?,
            None => false,
        };
        Ok(Observation {
            enabled: Some(!state.is_all_disabled() && matches),
        })
    }
    pub fn rollback(&mut self) -> Result<Observation, String> {
        let Some((before, checkpoint, target)) = self.rollback.take() else {
            return Err("没有待恢复的系统代理操作".into());
        };
        self.backend.restore(&checkpoint)?;
        let observed = self.backend.read()?;
        if !self.backend.checkpoint_matches(&checkpoint, &before)? {
            return Err("恢复后的系统代理回读不一致".into());
        }
        self.target = target;
        Ok(Observation::read(&observed, self.target.as_ref()))
    }
    pub fn clear(&mut self) -> Result<Observation, String> {
        self.rollback = None;
        let Some(target) = &self.target else {
            return Ok(Observation::default());
        };
        let observed = self.backend.clear(target)?;
        self.target = None;
        if self.backend.available()? {
            Ok(Observation::read(&observed, None))
        } else {
            Ok(Observation::default())
        }
    }
}
pub fn clear_current<B: Backend + ?Sized>(
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
pub fn equivalent(a: &OsSnapshot, b: &OsSnapshot) -> bool {
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

/// The service accepts only proxy transactions, never arbitrary system commands.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "operation", content = "target", rename_all = "snake_case")]
pub enum Operation {
    Apply(Target),
    Observe,
    Rollback,
    Clear,
}

#[cfg(test)]
mod availability_tests {
    use super::*;
    #[derive(Default)]
    struct Offline {
        cleanups: usize,
    }
    impl Backend for Offline {
        type Checkpoint = ();
        fn available(&self) -> Result<bool, String> {
            Ok(false)
        }
        fn capture(&mut self) -> Result<(OsSnapshot, ()), String> {
            panic!("offline close must not capture an active service")
        }
        fn read(&self) -> Result<OsSnapshot, String> {
            panic!("offline observation must remain unknown")
        }
        fn write(&mut self, _: &Target) -> Result<(), String> {
            panic!("offline close must not write a guessed service")
        }
        fn restore(&mut self, _: &()) -> Result<(), String> {
            Ok(())
        }
        fn clear(&mut self, _: &Target) -> Result<OsSnapshot, String> {
            self.cleanups += 1;
            Ok(OsSnapshot::default())
        }
    }
    #[test]
    fn offline_disable_cleans_known_services_but_does_not_claim_an_observed_off_state() {
        let mut owner = Owner::new(Offline::default());
        assert_eq!(owner.observe().unwrap().enabled(), None);
        assert_eq!(owner.apply(Target::default()).unwrap().enabled(), None);
        assert_eq!(owner.backend_mut().cleanups, 1);
        let enabled = Target {
            system: ManualProxy {
                enable: true,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(
            owner
                .apply(enabled)
                .unwrap_err()
                .contains("没有活动网络服务")
        );
        assert_eq!(owner.backend_mut().cleanups, 1);
    }
}
