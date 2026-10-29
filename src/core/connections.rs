//! Connection commands share the lifecycle command owner and confirm core state.
use super::{Phase, service::Actor};
use crate::traffic::ConnectionAction;
impl Actor {
    pub(super) async fn cleanup_previous_proxy(
        &self,
        previous: Option<&str>,
    ) -> Result<(), String> {
        let Some(previous) = previous else {
            return Ok(());
        };
        let repository =
            crate::storage::Repository::open(self.paths.clone()).map_err(|e| e.to_string())?;
        let settings =
            crate::preferences::values(&repository, crate::preferences::SettingsKind::General)
                .map_err(|e| e.to_string())?;
        if settings
            .get("AutoCloseConnection")
            .is_none_or(|value| value != "true")
        {
            return Ok(());
        }
        let client = &self.running.as_ref().ok_or("内核尚未就绪")?.launch.client;
        let before: crate::overview::statistics::Connections =
            serde_json::from_value(client.json("/connections").await?)
                .map_err(|_| "连接回读无效")?;
        let ids = before
            .connections
            .iter()
            .filter(|connection| connection.chains.iter().any(|name| name == previous))
            .map(|connection| connection.id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        use futures_util::{StreamExt, stream};
        let failures = stream::iter(
            ids.iter()
                .map(|id| async move { client.close_connections(Some(id)).await.err() }),
        )
        .buffer_unordered(10)
        .filter_map(async move |error| error)
        .collect::<Vec<_>>()
        .await;
        if !failures.is_empty() {
            return Err(format!(
                "节点已切换；连接清理失败：{}",
                crate::failure::join(&failures)
            ));
        }
        let after: crate::overview::statistics::Connections =
            serde_json::from_value(client.json("/connections").await?)
                .map_err(|_| "节点已切换；连接清理回读无效")?;
        if after
            .connections
            .iter()
            .any(|connection| ids.contains(&connection.id))
        {
            return Err("节点已切换；原节点连接仍存在".into());
        }
        Ok(())
    }
    pub(super) async fn connection_action(
        &mut self,
        action: ConnectionAction,
    ) -> Result<(), String> {
        self.snapshot.operation_module = Some("connections");
        let (generation, epoch, id) = match action {
            ConnectionAction::Close(target) => (target.generation, target.epoch, Some(target.id)),
            ConnectionAction::CloseAll { generation, epoch } => (generation, epoch, None),
        };
        if generation != self.snapshot.generation || epoch != self.snapshot.monitor_epoch {
            return Err("内核配置已变更，请刷新连接".into());
        }
        let client = &self
            .running
            .as_ref()
            .filter(|_| self.snapshot.phase == Phase::Ready)
            .ok_or("内核尚未就绪")?
            .launch
            .client;
        client.close_connections(id.as_deref()).await?;
        let observed: crate::overview::statistics::Connections =
            serde_json::from_value(client.json("/connections").await?)
                .map_err(|_| "连接回读无效")?;
        if observed
            .connections
            .iter()
            .any(|connection| id.as_ref().is_none_or(|id| id == &connection.id))
        {
            return Err("关闭请求已执行，仍有连接存在或新建".into());
        }
        self.completed(
            if id.is_some() {
                "连接已关闭"
            } else {
                "全部连接已关闭"
            }
            .into(),
        );
        Ok(())
    }
}
