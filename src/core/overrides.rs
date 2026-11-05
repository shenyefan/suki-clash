//! Override mutations share the configuration/lifecycle writer, never the generic storage bridge.
use super::{launch::Launch, service::Actor};
use crate::{
    profiles::{model::Document, override_repository as repository, store},
    storage::{AppPaths, Values},
};
#[derive(Clone)]
pub(crate) enum Change {
    Restore {
        ids: Vec<u64>,
    },
    Save {
        expected: Values,
        documents: Vec<Document>,
    },
    Update {
        ids: Vec<u64>,
        automatic: bool,
    },
}
#[derive(Clone)]
pub(super) struct Deleted {
    document: Document,
    position: usize,
    credentials: (String, String),
}
fn report_path(paths: &AppPaths, id: u64) -> std::path::PathBuf {
    paths
        .directory(crate::storage::DirectoryKind::Logs)
        .join("overrides")
        .join(format!("{id}.json"))
}
pub(super) fn save_report(
    paths: &AppPaths,
    report: &crate::configuration::script::Report,
) -> Result<(), String> {
    let bytes = serde_json::to_vec(report)
        .map_err(|error| crate::logging::diagnostic_error("无法编码脚本日志", error))?;
    let path = report_path(paths, report.id);
    crate::storage::atomic_write(&path, &bytes)
        .map_err(|error| crate::logging::diagnostic_error("脚本日志保存失败", error))?;
    let directory = path.parent().ok_or("脚本日志目录无效")?;
    let mut files = std::fs::read_dir(directory)
        .map_err(|error| crate::logging::diagnostic_error("无法读取脚本日志目录", error))?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let metadata = std::fs::symlink_metadata(entry.path()).ok()?;
            (metadata.is_file()
                && !metadata.file_type().is_symlink()
                && entry.path().extension().is_some_and(|ext| ext == "json")
                && entry.path() != path)
                .then(|| (metadata.modified().ok(), entry.path()))
        })
        .collect::<Vec<_>>();
    files.sort();
    let excess = files.len().saturating_sub(127);
    for (_, path) in files.into_iter().take(excess) {
        std::fs::remove_file(path)
            .map_err(|error| crate::logging::diagnostic_error("脚本日志清理失败", error))?;
    }
    Ok(())
}
impl Actor {
    pub(super) fn script_logs(&mut self, id: u64) -> Result<(), String> {
        use std::io::Read as _;
        let file = match std::fs::File::open(report_path(&self.paths, id)) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.snapshot.script_log = Some((id, "暂无执行记录".into()));
                return Ok(());
            }
            Err(error) => {
                return Err(crate::logging::diagnostic_error(
                    "无法读取脚本日志".into(),
                    error,
                ));
            }
        };
        let mut bytes = Vec::new();
        file.take(512 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| crate::logging::diagnostic_error("无法读取脚本日志", error))?;
        if bytes.len() > 512 * 1024 {
            return Err("脚本日志超过上限".into());
        }
        let report: crate::configuration::script::Report = serde_json::from_slice(&bytes)
            .map_err(|error| crate::logging::diagnostic_error("脚本日志格式无效", error))?;
        let mut text = format!(
            "{}  {}\n",
            report.at,
            report.error.as_deref().unwrap_or("执行完成")
        );
        for (level, line) in report.logs {
            text.push_str(&format!("{level}  {line}\n"));
        }
        self.snapshot.script_log = Some((id, text));
        Ok(())
    }
    pub(super) async fn overrides(&mut self, change: Change) -> Result<(), String> {
        self.snapshot.operation_committed = false;
        let (expected, old) = repository::read(&self.paths)?;
        self.snapshot
            .settings
            .insert("override-index".into(), expected.clone());
        let mut restored = vec![];
        let mut desired = match change {
            Change::Restore { ids } => {
                let mut documents = old.clone();
                let mut entries = vec![];
                for id in ids {
                    if documents.iter().any(|doc| doc.id == id) {
                        continue;
                    }
                    entries.push(
                        self.deleted_overrides
                            .get(&id)
                            .cloned()
                            .ok_or("撤销记录已失效")?,
                    );
                }
                entries.sort_by_key(|entry| entry.position);
                for entry in entries {
                    let mut document = entry.document;
                    restored.push(document.id);
                    document.token.clone_from(&entry.credentials.0);
                    document.age_key = entry.credentials.1;
                    document.credentials = crate::profiles::CredentialEdits::from_inputs(
                        &document.token,
                        &document.age_key,
                    );
                    documents.insert(entry.position.min(documents.len()), document);
                }
                documents
            }
            Change::Save {
                expected: revision,
                documents,
            } => {
                if revision != expected {
                    return Err("覆写已变更，请重新打开编辑".into());
                }
                documents
            }
            Change::Update { ids, automatic } => {
                let mut failures = vec![];
                let mut seen = std::collections::BTreeSet::new();
                for id in ids.into_iter().filter(|id| seen.insert(*id)) {
                    let (_, current) = repository::read(&self.paths)?;
                    let Some(mut document) = current.iter().find(|doc| doc.id == id).cloned()
                    else {
                        failures.push((format!("覆写 {id}"), "覆写不存在".into()));
                        continue;
                    };
                    if !document.is_remote() || (automatic && !document.automatic_updates()) {
                        continue;
                    }
                    self.override_attempts
                        .insert(id, chrono::Utc::now().timestamp());
                    let result = async {
                        self.fetch_profile(&mut document, false).await?;
                        let mut desired = current.clone();
                        if let Some(target) = desired.iter_mut().find(|doc| doc.id == id) {
                            *target = document;
                        }
                        self.commit_overrides(&current, &desired).await
                    }
                    .await;
                    if let Err(error) = result {
                        failures.push((format!("覆写 {id}"), error));
                    }
                }
                if !failures.is_empty() {
                    return Err(crate::failure::scoped(failures));
                }
                self.completed("覆写已更新".into());
                return Ok(());
            }
        };
        let vault = crate::profiles::CredentialStore::overrides(&self.paths);
        let mut secrets = std::collections::BTreeMap::new();
        let mut previous = std::collections::BTreeMap::new();
        for document in &mut desired {
            if document.credentials.changed() {
                let saved = vault.read(document.id).await?;
                let resolved = document.credentials.resolve(saved.clone());
                document.token.clone_from(&resolved.0);
                document.age_key.clone_from(&resolved.1);
                previous.insert(document.id, saved);
                secrets.insert(document.id, resolved);
            }
            if document.is_remote() && document.content.is_empty() {
                self.fetch_profile(document, !old.iter().any(|old| old.id == document.id))
                    .await?;
            }
            repository::validate(document)?;
        }
        let mut removed = vec![];
        for (position, document) in old
            .iter()
            .enumerate()
            .filter(|(_, doc)| !desired.iter().any(|new| new.id == doc.id))
        {
            removed.push(Deleted {
                document: document.clone(),
                position,
                credentials: vault.read(document.id).await?,
            });
        }
        for (id, (token, age)) in secrets {
            if let Err(error) = vault.write(id, &token, &age).await {
                let recovery = vault.restore(previous).await;
                return Err(match recovery {
                    Ok(()) => error,
                    Err(other) => format!("{error}；{other}"),
                });
            }
        }
        if let Err(error) = self.commit_overrides(&old, &desired).await {
            return Err(match vault.restore(previous).await {
                Ok(()) => error,
                Err(other) => format!("{error}；{other}"),
            });
        }
        self.snapshot.operation_committed = true;
        for entry in removed {
            self.deleted_overrides.insert(entry.document.id, entry);
        }
        for id in restored {
            self.deleted_overrides.remove(&id);
        }
        while self.deleted_overrides.len() > 128
            || self
                .deleted_overrides
                .values()
                .map(|entry| entry.document.content.len())
                .sum::<usize>()
                > crate::configuration::INPUT_LIMIT as usize
        {
            self.deleted_overrides.pop_first();
        }
        let mut errors = vec![];
        for document in old
            .iter()
            .filter(|doc| !desired.iter().any(|new| new.id == doc.id))
        {
            if let Err(error) = vault.delete(document.id).await {
                errors.push(error);
            }
        }
        if !errors.is_empty() {
            return Err(format!("覆写已保存；{}", crate::failure::join(&errors)));
        }
        self.completed("覆写已保存".into());
        Ok(())
    }
    async fn commit_overrides(
        &mut self,
        old: &[Document],
        desired: &[Document],
    ) -> Result<(), String> {
        for document in desired {
            repository::validate(document)?;
        }
        let values = store::values(desired, None);
        crate::configuration::validate_override_index(&values)
            .map_err(crate::logging::error_message)?;
        let candidate = if let Some(running) = &self.running {
            Some(
                Launch::with_overrides(&self.paths, running.launch.source.clone(), values.clone())
                    .await?,
            )
        } else {
            None
        };
        let recovery = repository::Recovery::capture(&self.paths, old, desired)?;
        let paths = self.paths.clone();
        let old = old.to_vec();
        let desired = desired.to_vec();
        let commit = move || repository::commit(&paths, &old, &desired).map(|_| ());
        let result = if let Some(candidate) = candidate {
            self.apply_profile_candidate(candidate, commit).await
        } else {
            commit()
        };
        if let Err(error) = result {
            return Err(match recovery.restore() {
                Ok(()) => error,
                Err(other) => format!("{error}；{other}"),
            });
        }
        self.snapshot
            .settings
            .insert("override-index".into(), values);
        Ok(())
    }
    pub(super) async fn automatic_overrides(&mut self) -> Result<(), String> {
        let (_, documents) = repository::read(&self.paths)?;
        let now = chrono::Utc::now().timestamp();
        let ids = documents
            .iter()
            .filter(|doc| doc.automatic_updates())
            .filter_map(|doc| {
                let interval = doc.interval.parse::<i64>().ok()?.checked_mul(60)?;
                let last = doc
                    .updated_at
                    .unwrap_or(self.profile_schedule_started)
                    .max(self.override_attempts.get(&doc.id).copied().unwrap_or(0));
                (interval > 0 && now.saturating_sub(last) >= interval).then_some(doc.id)
            })
            .collect::<Vec<_>>();
        if ids.is_empty() {
            return Ok(());
        }
        log::info!("自动更新覆写开始；条目={}", ids.len());
        self.snapshot.operation_active = true;
        self.snapshot.operation_module = Some("overrides");
        let result = self
            .overrides(Change::Update {
                ids,
                automatic: true,
            })
            .await;
        self.snapshot.operation_active = false;
        self.snapshot.progress = None;
        match &result {
            Ok(()) => log::info!("自动更新覆写完成"),
            Err(error) => log::error!("自动更新覆写失败：{error}"),
        }
        result
    }
}
