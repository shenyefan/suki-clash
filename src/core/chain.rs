use super::{Phase, launch::Launch, service::Actor};
use crate::proxies::{
    chain_config::{Change, Config},
    runtime::Selections,
};
impl Actor {
    pub(super) async fn change_chain(&mut self, change: Change) -> Result<(), String> {
        self.snapshot.operation_module = Some("proxies");
        if (change.generation, change.epoch)
            != (self.snapshot.generation, self.snapshot.monitor_epoch)
        {
            return Err("内核配置已变更，请重新打开链式代理".into());
        }
        let previous = Config::load(&self.paths)?;
        if previous != change.expected {
            return Err("链式代理设置已变更，请重新打开".into());
        }
        change.desired.validate()?;
        let paths = self.paths.clone();
        let desired = change.desired;
        if self.snapshot.phase != Phase::Ready {
            desired.save(&paths)?;
            self.snapshot.chain = desired;
            self.snapshot.information = Some("链式代理设置已保存".into());
            return Ok(());
        }
        let running = self.running.as_ref().ok_or("内核未运行")?;
        let client = running.launch.client.clone();
        let source = running.launch.source.clone();
        let profile = source.profile().map(|profile| profile.id());
        let before = client.proxies(&[]).await?;
        let target = if desired.enabled {
            desired.group.clone()
        } else {
            previous.group.clone()
        };
        let selection = before.selected(&target).map(str::to_owned);
        let candidate = Launch::with_chain(&paths, source.clone(), desired.clone()).await?;
        let commit = desired.clone();
        if let Err(error) = self
            .apply_profile_candidate(candidate, move || commit.save(&paths))
            .await
        {
            return Err(match previous.save(&self.paths) {
                Ok(()) => error,
                Err(recovery) => format!("{error}；{recovery}"),
            });
        }
        if !desired.enabled && !previous.enabled {
            self.snapshot.chain = desired;
            self.snapshot.information = Some("链式代理设置已保存".into());
            return Ok(());
        }
        let client = self
            .running
            .as_ref()
            .ok_or("链式配置已应用；内核尚未运行")?
            .launch
            .client
            .clone();
        let result = async {
            let node = if desired.enabled {
                desired
                    .effective_nodes()?
                    .last()
                    .cloned()
                    .ok_or("链式节点为空")?
            } else {
                let nodes = previous.effective_nodes()?;
                let entries = client.proxies(&[]).await?;
                let members = entries
                    .entries
                    .get(&target)
                    .and_then(|group| group.all.as_ref())
                    .ok_or("原目标代理组不存在")?;
                if members.iter().any(|name| name == "DIRECT") {
                    "DIRECT".to_owned()
                } else {
                    nodes
                        .first()
                        .filter(|node| members.contains(node))
                        .cloned()
                        .ok_or("原目标组没有可恢复的节点")?
                }
            };
            client.select_proxy(&target, Some(&node)).await?;
            if !client
                .proxies(&[])
                .await?
                .confirms_selection(&target, &node)
            {
                return Err("内核未确认链式出口选择".to_owned());
            }
            if super::Preferences::load(&self.paths)?
                .values()
                .get("StoreSelected")
                .is_none_or(|value| value != "false")
            {
                let mut saved = Selections::load(&self.paths)?;
                saved.update(profile, &target, Some(&node));
                saved.save(&self.paths)?;
            }
            Ok::<(), String>(())
        }
        .await;
        if let Err(error) = result {
            let rollback = async {
                let candidate = Launch::with_chain(&self.paths, source, previous.clone()).await?;
                let paths = self.paths.clone();
                let saved = previous.clone();
                self.apply_profile_candidate(candidate, move || saved.save(&paths))
                    .await?;
                if let Some(node) = selection {
                    let client = &self
                        .running
                        .as_ref()
                        .ok_or("原链式配置已恢复；内核尚未运行")?
                        .launch
                        .client;
                    client.select_proxy(&target, Some(&node)).await?;
                    if !client
                        .proxies(&[])
                        .await?
                        .confirms_selection(&target, &node)
                    {
                        return Err("原节点选择恢复未确认".into());
                    }
                }
                Ok::<(), String>(())
            }
            .await;
            return Err(match rollback {
                Ok(()) => format!("{error}；原链式配置已恢复"),
                Err(recovery) => format!("{error}；链式配置恢复失败：{recovery}"),
            });
        }
        // Configuration and selection are committed before connection cleanup.
        // A later cleanup failure must not publish the old connected state.
        self.snapshot.chain = desired.clone();
        self.snapshot.operation_committed = true;
        self.invalidate_monitor();
        self.publish(Phase::Ready, None);
        if !desired.enabled {
            super::connections::close_current_connections(&client)
                .await
                .map_err(|error| format!("链式代理已断开；连接清理未完成：{error}"))?;
        }
        self.snapshot.information = Some(if self.snapshot.chain.enabled {
            let observed = client
                .proxies(&[])
                .await
                .map_err(|error| format!("链式配置已应用；出口状态回读失败：{error}"))?;
            let exit = self.snapshot.chain.nodes.last().map(String::as_str);
            if observed.selected(&self.snapshot.chain.group) == exit {
                "链式代理已连接".into()
            } else {
                format!(
                    "链式配置已应用；出口已固定，当前节点：{}",
                    observed
                        .selected(&self.snapshot.chain.group)
                        .unwrap_or("待确认")
                )
            }
        } else {
            "链式代理已断开".into()
        });
        Ok(())
    }
}
