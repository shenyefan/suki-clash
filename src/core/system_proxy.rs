//! Lifecycle owns configuration intent; the OS worker owns actual proxy writes.
use super::{Phase, service::Actor};
use crate::{
    preferences::{self, SettingsKind},
    storage::{Namespace, Repository, Values},
};
impl Actor {
    fn proxy_settings(&self) -> Result<Values, String> {
        let repository = Repository::open(self.paths.clone()).map_err(|e| e.to_string())?;
        preferences::values(&repository, SettingsKind::SysProxy).map_err(|e| e.to_string())
    }
    async fn proxy_port(&self, values: &Values) -> Result<u16, String> {
        let running = self
            .running
            .as_ref()
            .filter(|_| self.snapshot.phase == Phase::Ready)
            .ok_or("内核未就绪，无法开启系统代理")?;
        running.launch.client.version().await?;
        let yaml: serde_yaml_ng::Value = serde_yaml_ng::from_str(&running.launch.composed)
            .map_err(|_| "无法读取运行中的代理监听配置")?;
        let port = ["mixed-port", "port"]
            .into_iter()
            .find_map(|key| {
                yaml.get(key)
                    .and_then(|v| v.as_u64())
                    .and_then(|p| u16::try_from(p).ok())
                    .filter(|p| *p > 0)
            })
            .ok_or("运行配置未启用 HTTP 或混合代理端口")?;
        // The OS adapter sets HTTP/HTTPS/SOCKS to one endpoint. Require a mixed
        // listener rather than advertising HTTP-only ports as SOCKS.
        if yaml.get("mixed-port").and_then(|v| v.as_u64()) != Some(port as u64) {
            return Err("系统代理需要启用混合端口".into());
        }
        let host = values
            .get("SystemProxyHost")
            .ok_or("系统代理主机缺失")?
            .trim_matches(['[', ']']);
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            tokio::net::TcpStream::connect((host, port)),
        )
        .await
        .map_err(|_| "系统代理端口连接超时")?
        .map_err(|e| format!("系统代理端口不可用：{e}"))?;
        Ok(port)
    }
    pub(super) async fn toggle_system_proxy(&mut self) -> Result<(), String> {
        let repository = Repository::open(self.paths.clone()).map_err(|e| e.to_string())?;
        let previous = self.proxy_settings()?;
        // An unknown OS state must not be interpreted as an off switch.
        self.snapshot.system_proxy = self.system_proxy.observe().await?;
        let enabled = match self.snapshot.system_proxy.enabled() {
            Some(enabled) => !enabled,
            None if previous
                .get("IsSystemProxyEnabled")
                .is_some_and(|value| value == "true") =>
            {
                false
            }
            None => return Err("无法确认系统代理状态，不能开启系统代理".into()),
        };
        self.save_system_proxy(
            &repository,
            &previous,
            Values::from([("IsSystemProxyEnabled".into(), enabled.to_string())]),
        )
        .await
    }
    pub(super) async fn save_system_proxy(
        &mut self,
        repository: &Repository,
        previous: &Values,
        patch: Values,
    ) -> Result<(), String> {
        if patch
            .get("IsSystemProxyEnabled")
            .is_some_and(|v| v == "true")
            && previous
                .get("IsSystemProxyEnabled")
                .is_none_or(|v| v != "true")
        {
            crate::bootstrap::require_core(&self.paths)?;
            if self.snapshot.phase != Phase::Ready {
                return Err("内核未就绪，无法开启系统代理".into());
            }
        }
        let mut desired = previous.clone();
        desired.extend(preferences::choices::encode(patch.clone()));
        preferences::validate(SettingsKind::SysProxy, &desired)?;
        if desired
            .get("SystemProxyModeIndex")
            .is_some_and(|mode| mode == "pac")
        {
            crate::configuration::script::validate(
                desired
                    .get("PacScript")
                    .map(String::as_str)
                    .unwrap_or_default(),
            )?;
        }
        let enabled = desired
            .get("IsSystemProxyEnabled")
            .is_some_and(|v| v == "true");
        let apply = (self.running.is_some() && enabled)
            || patch
                .get("IsSystemProxyEnabled")
                .is_some_and(|v| v == "false");
        let port = if apply && enabled {
            self.proxy_port(&desired).await?
        } else {
            0
        };
        let namespace = Namespace::parse("sysproxy").map_err(|e| e.to_string())?;
        if apply {
            match self.system_proxy.apply(&desired, port).await {
                Ok(observed) => self.snapshot.system_proxy = observed,
                Err(error) => {
                    self.snapshot.system_proxy =
                        self.system_proxy.observe().await.unwrap_or_default();
                    return Err(error);
                }
            }
        }
        if let Err(cause) = repository.save(&namespace, &desired) {
            let recovery = if apply {
                self.system_proxy
                    .rollback()
                    .await
                    .map(|observed| self.snapshot.system_proxy = observed)
            } else {
                Ok(())
            };
            return Err(match recovery {
                Ok(()) => cause.to_string(),
                Err(error) => format!("{cause}；系统代理恢复失败：{error}"),
            });
        }
        self.snapshot.settings.insert("sysproxy".into(), desired);
        self.snapshot.operation_committed = true;
        if !enabled
            && previous
                .get("IsSystemProxyEnabled")
                .is_some_and(|value| value == "true")
            && self.running.is_some()
        {
            self.cleanup_all_after_commit("系统代理已关闭").await?;
        }
        Ok(())
    }
    pub(super) async fn sync_system_proxy(&mut self) -> Result<(), String> {
        if self.snapshot.phase != Phase::Ready || self.system_proxy.automatic_suspended() {
            return Ok(());
        }
        #[cfg(target_os = "macos")]
        if self.system_proxy.refresh_network()? == crate::system_proxy::NetworkStatus::Unavailable {
            // Offline startup is deferred; permission/write failures still suspend.
            self.snapshot.system_proxy = Default::default();
            return Ok(());
        }
        let values = self.proxy_settings()?;
        self.snapshot
            .settings
            .insert("sysproxy".into(), values.clone());
        if values
            .get("IsSystemProxyEnabled")
            .is_some_and(|v| v == "true")
        {
            let result = async {
                let port = self.proxy_port(&values).await?;
                if !self.system_proxy.is_current(&values, port) {
                    self.snapshot.system_proxy =
                        self.system_proxy.apply_automatic(&values, port).await?;
                }
                Ok::<(), String>(())
            }
            .await;
            if let Err(cause) = result {
                // The kernel may already have committed a different port. An old
                // proxy must not remain pointed at a listener that no longer exists.
                return Err(match self.system_proxy.clear().await {
                    Ok(observed) => {
                        self.snapshot.system_proxy = observed;
                        format!("内核已就绪；系统代理未应用：{cause}")
                    }
                    Err(error) => {
                        self.snapshot.system_proxy = Default::default();
                        format!("内核已就绪；系统代理未应用：{cause}；清理失败：{error}")
                    }
                });
            }
        }
        Ok(())
    }
    pub(super) async fn poll_system_proxy(&mut self) -> Result<(), String> {
        if self.system_proxy.automatic_suspended() {
            return Ok(());
        }
        let observed = if self.snapshot.phase == Phase::Ready {
            #[cfg(target_os = "macos")]
            match self.system_proxy.refresh_network()? {
                crate::system_proxy::NetworkStatus::Unavailable => {
                    if self.snapshot.system_proxy != Default::default() {
                        self.snapshot.system_proxy = Default::default();
                        self.publish(self.snapshot.phase, None);
                    }
                    return Ok(());
                }
                crate::system_proxy::NetworkStatus::Changed => {
                    self.sync_system_proxy().await?;
                    self.publish(self.snapshot.phase, None);
                }
                crate::system_proxy::NetworkStatus::Unchanged => {}
            }
            // Observe and service the guard. A real apply failure remains suspended
            // until an explicit request, including across network changes.
            let values = self.proxy_settings()?;
            self.system_proxy.poll(&values).await
        } else {
            self.system_proxy.clear().await?;
            self.system_proxy.observe().await
        };
        match observed {
            Ok(observed) => {
                if observed != self.snapshot.system_proxy {
                    self.snapshot.system_proxy = observed;
                    self.publish(self.snapshot.phase, None);
                }
                Ok(())
            }
            Err(error) => {
                self.snapshot.system_proxy = Default::default();
                Err(error)
            }
        }
    }
}
