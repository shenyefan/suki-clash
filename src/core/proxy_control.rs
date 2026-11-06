//! Proxy commands are serialized with lifecycle/configuration changes and verified by readback.
use super::{Phase, monitor::Target, service::Actor};
use crate::{
    proxies::runtime::{Action, Selections},
    session::ProxyMode,
};
pub(super) struct SelectionRecovery {
    generation: u64,
    epoch: u64,
    pending: std::collections::BTreeMap<String, String>,
    pub(super) expires: std::time::Instant,
}
impl Actor {
    pub(super) async fn restore_all_selections(&mut self) -> Result<(), String> {
        let mut failures = Vec::new();
        if let Err(error) = self.restore_proxy_selections().await {
            failures.push(("节点选择", error));
        }
        if let Err(error) = self.restore_chain_selection().await {
            failures.push(("代理链", error));
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(crate::failure::scoped(failures))
        }
    }

    pub(super) fn invalidate_monitor(&mut self) {
        self.selection_recovery = None;
        self.snapshot.monitor_epoch = self.snapshot.monitor_epoch.saturating_add(1);
        self.monitor.send_replace(None);
    }
    pub(super) fn sync_monitor(&mut self) {
        if self.snapshot.phase != Phase::Ready {
            self.monitor.send_if_modified(|current| {
                if current.is_some() {
                    *current = None;
                    true
                } else {
                    false
                }
            });
            return;
        }
        let Some(running) = &self.running else { return };
        let mut order = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&running.launch.composed)
            .ok()
            .and_then(|config| {
                config
                    .get("proxy-groups")
                    .and_then(serde_yaml_ng::Value::as_sequence)
                    .cloned()
            })
            .unwrap_or_default()
            .iter()
            .filter_map(|group| {
                group
                    .get("name")
                    .and_then(serde_yaml_ng::Value::as_str)
                    .map(str::to_owned)
            })
            .collect::<Vec<_>>();
        order.push("GLOBAL".into());
        let values = self
            .snapshot
            .preferences
            .as_ref()
            .map(|preferences| preferences.values())
            .unwrap_or_default();
        let port = ["MixedPort", "HttpPort"]
            .iter()
            .find_map(|key| {
                values
                    .get(*key)
                    .and_then(|port| port.parse::<u16>().ok())
                    .filter(|port| *port > 0)
            })
            .unwrap_or(0);
        let dns_mode = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&running.launch.composed)
            .ok()
            .and_then(|config| {
                config
                    .get("dns")
                    .and_then(|dns| dns.get("enhanced-mode"))
                    .and_then(serde_yaml_ng::Value::as_str)
                    .map(str::to_owned)
            });
        let target = Target {
            logs: running
                .log_settings
                .as_ref()
                .map(|settings| super::log_stream::Source {
                    service: running.log_session(),
                    settings: settings.subscribe(),
                    pipe_suppressed: running.pipe_suppressed.clone(),
                }),
            proxy_error: self
                .snapshot
                .preferences
                .as_ref()
                .and_then(super::Preferences::local_proxy_error),
            http: self.http.clone(),
            dns_mode,
            client: running.launch.client.clone(),
            generation: self.snapshot.generation,
            epoch: self.snapshot.monitor_epoch,
            order,
            proxy_port: port,
            proxy_auth: values
                .get("Authentication")
                .and_then(|values| values.lines().next())
                .map(str::to_owned),
        };
        self.monitor.send_if_modified(|current| {
            if current.as_ref().is_some_and(|current| {
                current.generation == target.generation && current.epoch == target.epoch
            }) {
                false
            } else {
                *current = Some(target);
                true
            }
        });
    }
    pub(super) async fn restore_chain_selection(&self) -> Result<(), String> {
        let chain = crate::proxies::chain_config::Config::load(&self.paths)?;
        if !chain.enabled {
            return Ok(());
        }
        let client = &self.running.as_ref().ok_or("内核尚未运行")?.launch.client;
        let nodes = chain.effective_nodes()?;
        let node = nodes.last().ok_or("链式节点为空")?;
        client.select_proxy(&chain.group, Some(node)).await?;
        if !client
            .proxies(&[])
            .await?
            .confirms_selection(&chain.group, node)
        {
            return Err("内核未确认链式出口选择".into());
        }
        Ok(())
    }
    pub(super) async fn restore_proxy_selections(&mut self) -> Result<(), String> {
        self.selection_recovery = None;
        let running = self.running.as_ref().ok_or("内核尚未运行")?;
        if super::Preferences::load(&self.paths)?
            .values()
            .get("StoreSelected")
            .is_some_and(|value| value == "false")
        {
            return Ok(());
        }
        let pending = Selections::load(&self.paths)?
            .for_profile(running.launch.source.profile().map(|profile| profile.id()));
        if pending.is_empty() {
            return Ok(());
        }
        self.selection_recovery = Some(SelectionRecovery {
            generation: self.snapshot.generation,
            epoch: self.snapshot.monitor_epoch,
            pending,
            expires: std::time::Instant::now() + std::time::Duration::from_secs(30),
        });
        self.settle_proxy_selections().await
    }
    pub(super) async fn settle_proxy_selections(&mut self) -> Result<(), String> {
        let Some(mut recovery) = self.selection_recovery.take() else {
            return Ok(());
        };
        if (recovery.generation, recovery.epoch)
            != (self.snapshot.generation, self.snapshot.monitor_epoch)
        {
            return Ok(());
        }
        let client = self
            .running
            .as_ref()
            .ok_or("内核尚未运行")?
            .launch
            .client
            .clone();
        let remaining = recovery
            .expires
            .saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(format!(
                "节点选择恢复已停止：{} 个组或节点在30秒内未就绪",
                recovery.pending.len()
            ));
        }
        let restored = tokio::time::timeout(remaining, async {
            let proxies = client.proxies(&[]).await?;
            let mut restored = Vec::new();
            for (group, node) in &recovery.pending {
                // A cold provider may not have published the group or member yet.
                if !proxies.selectable(group, node) {
                    continue;
                }
                if !proxies.confirms_selection(group, node) {
                    client.select_proxy(group, Some(node)).await?;
                    if !client.proxies(&[]).await?.confirms_selection(group, node) {
                        return Err("内核未确认保存的节点选择，已停止本次恢复".into());
                    }
                }
                restored.push(group.clone());
            }
            Ok::<_, String>(restored)
        })
        .await
        .map_err(|error| {
            crate::logging::diagnostic_error("节点选择恢复已达到30秒时限，已停止".to_owned(), error)
        })??;
        for group in restored {
            recovery.pending.remove(&group);
        }
        if recovery.pending.is_empty() {
            return Ok(());
        }
        if std::time::Instant::now() >= recovery.expires {
            return Err(format!(
                "节点选择恢复已停止：{} 个组或节点在30秒内未就绪",
                recovery.pending.len()
            ));
        }
        self.selection_recovery = Some(recovery);
        Ok(())
    }
    pub(super) async fn proxy_action(&mut self, action: Action) -> Result<(), String> {
        self.snapshot.operation_module = Some("proxies");
        let running = self
            .running
            .as_ref()
            .filter(|_| self.snapshot.phase == Phase::Ready)
            .ok_or("内核尚未就绪")?;
        let client = running.launch.client.clone();
        let profile = running.launch.source.profile().map(|profile| profile.id());
        let generation = self.snapshot.generation;
        let remember = super::Preferences::load(&self.paths)?
            .values()
            .get("StoreSelected")
            .is_none_or(|value| value != "false");
        match action {
            Action::Refresh => {}
            Action::Mode(mode) => return self.change_mode(mode).await,
            Action::Select {
                group,
                node,
                generation: expected,
                epoch,
            } => {
                if expected != generation || epoch != self.snapshot.monitor_epoch {
                    return Err("内核配置已变更，请重试节点选择".into());
                }
                let before = client.proxies(&[]).await?;
                let previous_node = before.selected(&group).map(str::to_owned);
                if !before.selectable(&group, &node) {
                    return Err("代理组不支持此节点选择".into());
                }
                if let Some(recovery) = &mut self.selection_recovery {
                    recovery.pending.remove(&group);
                }
                client.select_proxy(&group, Some(&node)).await?;
                let actual = client.proxies(&[]).await?;
                if !actual.confirms_selection(&group, &node) {
                    return Err("内核未确认节点切换".into());
                }
                if remember {
                    let mut selections = Selections::load(&self.paths)
                        .map_err(|error| format!("节点已切换；{error}"))?;
                    selections.update(profile, &group, Some(&node));
                    if let Err(error) = selections.save(&self.paths) {
                        return Err(format!("节点已切换；{error}"));
                    }
                }
                if previous_node.as_deref() != Some(node.as_str()) {
                    self.cleanup_previous_proxy(previous_node.as_deref())
                        .await?;
                }
                self.snapshot.information =
                    Some(if actual.selected(&group) == Some(node.as_str()) {
                        "节点已切换".into()
                    } else {
                        format!(
                            "节点已固定；当前使用 {}",
                            actual.selected(&group).unwrap_or("未知")
                        )
                    });
            }
            Action::Unfix {
                group,
                generation: expected,
                epoch,
            } => {
                if expected != generation || epoch != self.snapshot.monitor_epoch {
                    return Err("内核配置已变更，请重试".into());
                }
                let proxies = client.proxies(&[]).await?;
                let previous_node = proxies.selected(&group).map(str::to_owned);
                let proxy = proxies.entries.get(&group).ok_or("代理组不存在")?;
                if !matches!(proxy.kind.as_str(), "URLTest" | "Fallback") {
                    return Err("此代理组不支持自动选择".into());
                }
                if let Some(recovery) = &mut self.selection_recovery {
                    recovery.pending.remove(&group);
                }
                client.select_proxy(&group, None).await?;
                let actual = client.proxies(&[]).await?;
                if actual
                    .entries
                    .get(&group)
                    .and_then(|proxy| proxy.fixed.as_deref())
                    .is_some_and(|name| !name.is_empty())
                {
                    return Err("内核未清除固定节点".into());
                }
                if remember {
                    let mut saved = Selections::load(&self.paths)
                        .map_err(|error| format!("自动选择已恢复；{error}"))?;
                    saved.update(profile, &group, None);
                    saved
                        .save(&self.paths)
                        .map_err(|error| format!("自动选择已恢复；{error}"))?;
                }
                self.cleanup_previous_proxy(previous_node.as_deref())
                    .await?;
                self.snapshot.information = Some("已恢复自动选择".into());
            }
        }
        self.publish(Phase::Ready, None);
        Ok(())
    }
    async fn change_mode(&mut self, mode: ProxyMode) -> Result<(), String> {
        let client = self
            .running
            .as_ref()
            .ok_or("内核尚未运行")?
            .launch
            .client
            .clone();
        let previous = super::Preferences::load(&self.paths)?;
        let desired = previous.with_mode(mode);
        let before = client
            .json("/configs")
            .await?
            .get("mode")
            .and_then(serde_json::Value::as_str)
            .ok_or("内核模式回读无效")?
            .to_owned();
        let result = async {
            client.patch_mode(mode).await?;
            let actual = client.json("/configs").await?;
            if actual.get("mode").and_then(serde_json::Value::as_str) != Some(mode.label()) {
                return Err("内核模式未确认".into());
            }
            desired.save(&self.paths)?;
            self.running
                .as_mut()
                .ok_or("模式切换期间内核已停止")?
                .launch
                .record_mode(mode)?;
            Ok::<(), String>(())
        }
        .await;
        if let Err(error) = result {
            let old = serde_json::from_value::<ProxyMode>(serde_json::Value::String(before))
                .map_err(|error| crate::logging::rejected("旧模式无效", error))?;
            let api = async {
                client.patch_mode(old).await?;
                if client.json("/configs").await?["mode"].as_str() != Some(old.label()) {
                    return Err("模式恢复未确认".to_owned());
                }
                Ok(())
            }
            .await;
            let files = previous.save(&self.paths);
            return Err(format!(
                "{error}；模式恢复{}，设置恢复{}",
                if api.is_ok() { "完成" } else { "失败" },
                if files.is_ok() { "完成" } else { "失败" }
            ));
        }
        self.snapshot.preferences = Some(desired);
        self.snapshot.operation_committed = true;
        self.publish(Phase::Ready, None);
        self.cleanup_all_after_commit("出站模式已切换").await?;
        self.completed("出站模式已切换".into());
        Ok(())
    }
}
