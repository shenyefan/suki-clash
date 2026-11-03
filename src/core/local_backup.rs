//! Local files share the complete archive and restoration workflow with Gist.
use super::service::Actor;
use crate::backup::{
    Archive,
    local::{self, Entry, Id},
};
use std::path::PathBuf;

#[derive(Clone)]
pub(crate) enum Command {
    List,
    Create,
    Import(PathBuf),
    Export { id: Id, target: PathBuf },
    Restore(Id),
    Delete(Id),
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct State {
    pub(crate) entries: Vec<Entry>,
    pub(crate) exported: Option<PathBuf>,
}
impl Actor {
    async fn decode_local_backup(&self, content: &str) -> Result<Archive, String> {
        let key = if content.starts_with("-----BEGIN AGE ENCRYPTED FILE-----") {
            Some(
                crate::profiles::CredentialStore::gist(&self.paths)
                    .read(0)
                    .await?
                    .1,
            )
        } else {
            None
        };
        Archive::decode(content, key.as_deref().filter(|key| !key.is_empty()))
    }
    pub(super) async fn local_backup(&mut self, command: Command) -> Result<(), String> {
        self.snapshot.operation_module = Some("backup");
        self.snapshot.local_backup.exported = None;
        let message = match command {
            Command::List => None,
            Command::Create => {
                self.progress("创建本地备份");
                let content = Archive::capture(&self.paths)?.encode(None)?;
                local::store(&self.paths, &content)?;
                self.snapshot.operation_committed = true;
                Some("本地备份已创建")
            }
            Command::Import(path) => {
                self.progress("校验并导入备份");
                let content = local::read_path(&path)?;
                let archive = self.decode_local_backup(&content).await?;
                let staged = archive.stage(&self.paths)?;
                super::launch::Launch::preview(&staged.paths).await?;
                local::store(&self.paths, &content)?;
                self.snapshot.operation_committed = true;
                Some("备份已导入")
            }
            Command::Export { id, target } => {
                local::export(&self.paths, &id, &target)?;
                self.snapshot.local_backup.exported = Some(target);
                self.snapshot.operation_committed = true;
                Some("备份已导出")
            }
            Command::Restore(id) => {
                let content = local::read(&self.paths, &id)?;
                let archive = self.decode_local_backup(&content).await?;
                self.restore_archive(archive).await?;
                None
            }
            Command::Delete(id) => {
                local::delete(&self.paths, &id)?;
                self.snapshot
                    .local_backup
                    .entries
                    .retain(|entry| entry.id != id);
                self.snapshot.operation_committed = true;
                Some("本地备份已删除")
            }
        };
        self.snapshot.local_backup.entries = local::list(&self.paths).map_err(|error| {
            if self.snapshot.operation_committed {
                format!("备份操作已完成；历史刷新失败：{error}")
            } else {
                error
            }
        })?;
        if let Some(message) = message {
            self.completed(message.into());
        }
        Ok(())
    }
}
