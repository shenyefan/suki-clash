//! Connection commands share the lifecycle owner; readback tracks the requested IDs.
use super::{Phase, client::Client, service::Actor};
use crate::traffic::ConnectionAction;
use std::collections::BTreeSet;

type Connections = crate::overview::statistics::Connections;

async fn connection_ids(client: &Client) -> Result<Connections, String> {
    serde_json::from_value(client.json("/connections").await?).map_err(|_| "连接回读无效".into())
}

async fn close_ids(client: &Client, ids: BTreeSet<String>) -> Result<(), String> {
    if ids.is_empty() {
        return Ok(());
    }
    use futures_util::{StreamExt, stream};
    let mut failures = stream::iter(
        ids.iter()
            .map(|id| async move { client.close_connections(Some(id)).await.err() }),
    )
    .buffer_unordered(10)
    .filter_map(async move |error| error)
    .collect::<Vec<_>>()
    .await;
    match connection_ids(client).await {
        Ok(after) => {
            if after
                .connections
                .iter()
                .any(|connection| ids.contains(&connection.id))
            {
                failures.push("目标连接仍存在".into());
            }
        }
        Err(error) => failures.push(error),
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(crate::failure::join(&failures))
    }
}

pub(super) async fn close_current_connections(client: &Client) -> Result<(), String> {
    let before = connection_ids(client).await?;
    close_ids(
        client,
        before
            .connections
            .into_iter()
            .map(|connection| connection.id)
            .collect(),
    )
    .await
}

impl Actor {
    fn auto_close_connections(&self) -> Result<bool, String> {
        let repository = crate::storage::Repository::open(self.paths.clone())
            .map_err(|error| error.to_string())?;
        let settings =
            crate::preferences::values(&repository, crate::preferences::SettingsKind::General)
                .map_err(|error| error.to_string())?;
        Ok(settings
            .get("AutoCloseConnection")
            .is_some_and(|value| value == "true"))
    }
    pub(super) async fn cleanup_all_after_commit(&self, committed: &str) -> Result<(), String> {
        let cleanup = async {
            if !self.auto_close_connections()? {
                return Ok(());
            }
            let client = &self.running.as_ref().ok_or("内核尚未就绪")?.launch.client;
            close_current_connections(client).await
        }
        .await;
        cleanup.map_err(|error| format!("{committed}；连接清理未完成：{error}"))
    }
    pub(super) async fn cleanup_previous_proxy(
        &self,
        previous: Option<&str>,
    ) -> Result<(), String> {
        let Some(previous) = previous else {
            return Ok(());
        };
        let cleanup = async {
            if !self.auto_close_connections()? {
                return Ok(());
            }
            let client = &self.running.as_ref().ok_or("内核尚未就绪")?.launch.client;
            let before = connection_ids(client).await?;
            let ids = before
                .connections
                .into_iter()
                .filter(|connection| connection.chains.iter().any(|name| name == previous))
                .map(|connection| connection.id)
                .collect();
            close_ids(client, ids).await
        }
        .await;
        cleanup.map_err(|error| format!("节点选择已提交；连接清理未完成：{error}"))
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
        if let Some(id) = &id {
            close_ids(client, BTreeSet::from([id.clone()])).await?;
        } else {
            close_current_connections(client).await?;
        }
        self.completed(
            if id.is_some() {
                "连接已关闭"
            } else {
                "目标连接已全部关闭"
            }
            .into(),
        );
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::close_current_connections;
    use crate::core::client::{Client, Endpoint};
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    async fn server(
        after: &str,
    ) -> (
        tempfile::TempDir,
        Client,
        tokio::task::JoinHandle<Vec<String>>,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("connections.sock");
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let after = after.to_owned();
        let task = tokio::spawn(async move {
            let mut requests = Vec::new();
            for body in [r#"{"uploadTotal":0,"downloadTotal":0,"connections":[{"id":"old","upload":0,"download":0,"metadata":{}}]}"#.to_owned(), String::new(), after] {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                let mut chunk = [0u8; 1024];
                while !bytes.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    let count = socket.read(&mut chunk).await.unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&chunk[..count]);
                }
                requests.push(String::from_utf8(bytes).unwrap().lines().next().unwrap().to_owned());
                let response = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                socket.write_all(response.as_bytes()).await.unwrap();
            }
            requests
        });
        (
            directory,
            Client::new(Endpoint::Unix(path), String::new()),
            task,
        )
    }

    #[tokio::test]
    async fn closing_snapshot_ids_does_not_close_or_reject_new_connections() {
        let (_directory, client, task) = server(r#"{"uploadTotal":0,"downloadTotal":0,"connections":[{"id":"new","upload":0,"download":0,"metadata":{}}]}"#).await;
        close_current_connections(&client).await.unwrap();
        assert_eq!(
            task.await.unwrap(),
            [
                "GET /connections HTTP/1.1",
                "DELETE /connections/old HTTP/1.1",
                "GET /connections HTTP/1.1"
            ]
        );
    }
    #[tokio::test]
    async fn closing_snapshot_ids_rejects_an_unclosed_target() {
        let (_directory, client, task) = server(r#"{"uploadTotal":0,"downloadTotal":0,"connections":[{"id":"old","upload":0,"download":0,"metadata":{}}]}"#).await;
        assert!(
            close_current_connections(&client)
                .await
                .unwrap_err()
                .contains("目标连接仍存在")
        );
        task.await.unwrap();
    }
}
