//! Connection commands share the lifecycle command owner and confirm core state.
use super::{Phase, service::Actor};
use crate::traffic::ConnectionAction;
impl Actor {
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
