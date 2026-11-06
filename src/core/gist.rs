//! Gist operations are serialized with configuration and source changes by the lifecycle actor.
use super::{Phase, launch::Launch, service::Actor};
use crate::{profiles::CredentialEdits, storage::AppPaths};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub(crate) enum Command {
    Inspect,
    Configure {
        expected: Settings,
        settings: Settings,
        credentials: CredentialEdits,
    },
    Create,
    List {
        page: u32,
    },
    Download(String),
    Restore(String),
    Delete(String),
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Settings {
    pub(crate) encrypt: bool,
    pub(crate) recipient: String,
}
impl Settings {
    fn load(paths: &AppPaths) -> Result<Self, String> {
        crate::preferences::app::read(paths, "backup")
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct State {
    pub(crate) settings: Settings,
    pub(crate) token_saved: bool,
    pub(crate) key_saved: bool,
    pub(crate) entries: Vec<Entry>,
    pub(crate) page: u32,
    pub(crate) has_more: bool,
    pub(crate) downloaded: Option<std::path::PathBuf>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) id: String,
    pub(crate) created_at: String,
    pub(crate) description: String,
}
impl Actor {
    async fn inspect_gist(&mut self) -> Result<(), String> {
        self.snapshot.gist.settings = Settings::load(&self.paths)?;
        let (token, key) = crate::profiles::CredentialStore::gist(&self.paths)
            .read(0)
            .await?;
        self.snapshot.gist.token_saved = !token.is_empty();
        self.snapshot.gist.key_saved = !key.is_empty();
        Ok(())
    }
    pub(super) async fn gist(&mut self, command: Command) -> Result<(), String> {
        self.snapshot.operation_module = Some("backup");
        self.snapshot.gist.downloaded = None;
        match command {
            Command::Inspect => self.inspect_gist().await,
            Command::Configure {
                expected,
                settings,
                credentials,
            } => {
                if Settings::load(&self.paths)? != expected {
                    return Err("备份配置已变更，请重新打开编辑".into());
                }
                if settings.encrypt {
                    settings
                        .recipient
                        .parse::<age::x25519::Recipient>()
                        .map_err(|error| crate::logging::rejected("Age接收方公钥无效", error))?;
                }
                let vault = crate::profiles::CredentialStore::gist(&self.paths);
                let previous = vault.read(0).await?;
                let desired = credentials.resolve(previous.clone());
                if !desired.1.is_empty() {
                    desired
                        .1
                        .parse::<age::x25519::Identity>()
                        .map_err(|error| crate::logging::rejected("Age私钥无效", error))?;
                }
                if desired != previous {
                    vault.write(0, &desired.0, &desired.1).await?;
                }
                if let Err(cause) = crate::preferences::app::write(&self.paths, "backup", &settings)
                {
                    return Err(match vault.write(0, &previous.0, &previous.1).await {
                        Ok(()) => cause,
                        Err(error) => format!("{cause}；备份凭据恢复失败：{error}"),
                    });
                }
                self.snapshot.operation_committed = true;
                self.inspect_gist().await
            }
            command => self.gist_transfer(command).await,
        }
    }
    async fn gist_transfer(&mut self, command: Command) -> Result<(), String> {
        let (token, key) = crate::profiles::CredentialStore::gist(&self.paths)
            .read(0)
            .await?;
        if token.is_empty() {
            return Err("请先配置GitHub Token".into());
        }
        let settings = Settings::load(&self.paths)?;
        let remote = crate::backup::Gist::new(&self.http, &token)?;
        match command {
            Command::Create => {
                self.progress("创建 Gist 备份");
                let archive = crate::backup::Archive::capture(&self.paths)?;
                let content =
                    archive.encode(settings.encrypt.then_some(settings.recipient.as_str()))?;
                remote.create(&content).await?;
                self.snapshot.operation_committed = true;
                self.completed("Gist备份已创建".into());
            }
            Command::List { page } => {
                self.progress("读取 Gist 备份历史");
                let result = remote.list(page).await?;
                self.snapshot.gist.has_more = result.has_more;
                let entries = result.entries;
                self.snapshot.gist.page = page;
                self.snapshot.gist.entries = entries
                    .into_iter()
                    .map(|entry| Entry {
                        id: entry.id,
                        created_at: chrono::DateTime::parse_from_rfc3339(&entry.updated_at)
                            .map(|time| {
                                time.with_timezone(&chrono::Local)
                                    .format("%Y-%m-%d %H:%M:%S %:z")
                                    .to_string()
                            })
                            .unwrap_or(entry.updated_at),
                        description: entry.description,
                    })
                    .collect();
            }
            Command::Download(id) => {
                self.progress("下载 Gist 备份");
                let content = remote.download(&id).await?;
                // Keep the original archive; decryption and full validation belong to Restore.
                let directory = self.paths.directory(crate::storage::DirectoryKind::Backups);
                std::fs::create_dir_all(&directory)
                    .map_err(|error| crate::logging::diagnostic_error("无法创建备份目录", error))?;
                let path = directory.join(format!("gist-{id}.suki-backup"));
                crate::storage::atomic_write(&path, content.as_bytes())
                    .map_err(crate::logging::error_message)?;
                self.snapshot.gist.downloaded = Some(path);
            }
            Command::Restore(id) => {
                self.progress("下载并校验 Gist 备份");
                let content = remote.download(&id).await?;
                let archive = crate::backup::Archive::decode(
                    &content,
                    (!key.is_empty()).then_some(key.as_str()),
                )?;
                self.restore_archive(archive).await?;
            }
            Command::Delete(id) => {
                self.progress("删除 Gist 备份");
                remote.delete(&id).await?;
                self.snapshot.gist.entries.retain(|entry| entry.id != id);
                self.snapshot.operation_committed = true;
                self.completed("Gist备份已删除".into());
            }
            Command::Inspect | Command::Configure { .. } => return Err("备份请求无效".into()),
        }
        Ok(())
    }
    pub(super) async fn restore_archive(
        &mut self,
        archive: crate::backup::Archive,
    ) -> Result<(), String> {
        self.progress("恢复备份");
        let staged = archive.stage(&self.paths)?;
        Launch::preview(&staged.paths).await?;
        let old_profiles = crate::profiles::repository::read(&self.paths)?.1;
        let old_overrides = crate::profiles::override_repository::read(&self.paths)?.1;
        let new_profiles = crate::profiles::repository::read(&staged.paths)?.1;
        let new_overrides = crate::profiles::override_repository::read(&staged.paths)?.1;
        let mut credentials = Vec::new();
        for (overrides, before, after) in [
            (false, &old_profiles, &new_profiles),
            (true, &old_overrides, &new_overrides),
        ] {
            let vault = if overrides {
                crate::profiles::CredentialStore::overrides(&self.paths)
            } else {
                crate::profiles::CredentialStore::new(&self.paths)
            };
            for document in after {
                if !before.iter().any(|old| {
                    old.id == document.id && old.kind == document.kind && old.url == document.url
                }) {
                    let previous = vault.read(document.id).await?;
                    if !previous.0.is_empty() || !previous.1.is_empty() {
                        credentials.push((overrides, document.id, previous));
                    }
                }
            }
        }
        let restored = archive.apply(&self.paths)?;
        let result = async {
            for (overrides, id, _) in &credentials {
                let vault = if *overrides {
                    crate::profiles::CredentialStore::overrides(&self.paths)
                } else {
                    crate::profiles::CredentialStore::new(&self.paths)
                };
                vault.delete(*id).await?;
            }
            if self.running.is_some() {
                let candidate = Launch::prepare(&self.paths).await?;
                self.apply_profile_candidate(candidate, || Ok(())).await?;
            } else {
                Launch::preview(&self.paths).await?;
            }
            Ok::<(), String>(())
        }
        .await;
        if let Err(cause) = result {
            let mut errors = vec![cause];
            if let Err(error) = restored.rollback(&self.paths) {
                errors.push(format!("备份文件恢复失败：{error}"));
            }
            for (overrides, id, (token, key)) in credentials {
                let vault = if overrides {
                    crate::profiles::CredentialStore::overrides(&self.paths)
                } else {
                    crate::profiles::CredentialStore::new(&self.paths)
                };
                if let Err(error) = vault.write(id, &token, &key).await {
                    errors.push(format!("订阅凭据恢复失败：{error}"));
                }
            }
            if let Err(error) = self.refresh_restored_settings() {
                errors.push(format!("原设置回读失败：{error}"));
            }
            if let Err(error) = self.sync_system_proxy().await {
                errors.push(format!("原系统代理恢复失败：{error}"));
            }
            return Err(crate::failure::join(&errors));
        }
        restored.finish();
        self.snapshot.operation_committed = true;
        self.refresh_restored_settings()
            .map_err(|error| format!("备份已恢复；设置回读失败：{error}"))?;
        self.publish(
            if self.running.is_some() {
                Phase::Ready
            } else {
                Phase::Stopped
            },
            None,
        );
        if let Err(error) = self.sync_system_proxy().await {
            return Err(format!("备份已恢复；{error}"));
        }
        self.completed("备份已恢复".into());
        Ok(())
    }
    fn refresh_restored_settings(&mut self) -> Result<(), String> {
        self.snapshot.preferences = Some(super::Preferences::load(&self.paths)?);
        let repository = crate::storage::Repository::open(self.paths.clone())
            .map_err(crate::logging::error_message)?;
        for kind in crate::preferences::SettingsKind::ALL {
            let values = crate::preferences::values(&repository, kind)
                .map_err(crate::logging::error_message)?;
            self.snapshot.settings.insert(kind.id().into(), values);
        }
        self.snapshot.proxy_preferences =
            crate::proxies::preferences::Preferences::load(&self.paths)?;
        self.refresh_profiles()?;
        let (values, _) = crate::profiles::override_repository::read(&self.paths)?;
        self.snapshot
            .settings
            .insert("override-index".into(), values);
        Ok(())
    }
}
