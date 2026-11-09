//! Serialized subscription operations share the lifecycle owner with configuration application.
use super::{Phase, launch::Launch, service::Actor};
use crate::{
    profiles::{model::Document, repository, store},
    storage::Values,
};
use std::collections::BTreeMap;
#[derive(Clone)]
pub(super) struct DeletedProfile {
    document: Document,
    position: usize,
    enhancement: Values,
    credentials: (String, String),
}
#[derive(Clone)]
pub(crate) enum ProfileCommand {
    Restore {
        ids: Vec<u64>,
    },
    Enhance {
        id: u64,
        expected: Values,
        values: Values,
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
impl Actor {
    pub(super) fn refresh_profiles(&mut self) -> Result<(), String> {
        let (overrides, _) = crate::profiles::override_repository::read(&self.paths)?;
        self.snapshot
            .settings
            .insert("override-index".into(), overrides);
        let (values, _) = repository::read(&self.paths)?;
        self.snapshot
            .settings
            .insert("profile-index".into(), values);
        Ok(())
    }
    pub(super) async fn profiles(&mut self, command: ProfileCommand) -> Result<(), String> {
        self.snapshot.operation_module = Some("profiles");
        self.snapshot.operation_committed = false;
        if let ProfileCommand::Restore { ids } = command {
            return self.restore_profiles(ids).await;
        }
        if matches!(&command,ProfileCommand::Update {ids,..} if ids.is_empty()) {
            return Ok(());
        }
        if let ProfileCommand::Enhance {
            id,
            expected,
            values,
        } = command
        {
            return self.enhance_profile(id, expected, values).await;
        }
        let (expected, old) = repository::read(&self.paths)?;
        let (mut documents, update_ids, automatic) = match command {
            ProfileCommand::Enhance { .. } | ProfileCommand::Restore { .. } => {
                return Err("订阅请求无效".into());
            }
            ProfileCommand::Save {
                expected: revision,
                documents,
            } => {
                if expected != revision {
                    return Err("订阅已变更，请重新打开编辑".into());
                }
                (documents, vec![], false)
            }
            ProfileCommand::Update { ids, automatic } => (old.clone(), ids, automatic),
        };
        let selected = crate::configuration::selected_profile(&self.paths)?;
        let mut attempted = 0;
        // Batch updates commit each resource independently; a failed resource retains its last content.
        if !update_ids.is_empty() {
            let mut errors = Vec::new();
            let mut seen = std::collections::BTreeSet::new();
            for id in update_ids.into_iter().filter(|id| seen.insert(*id)) {
                let Some(document) = documents.iter_mut().find(|doc| doc.id == id) else {
                    errors.push((format!("订阅 {id}"), "订阅不存在".into()));
                    continue;
                };
                if !document.is_remote() || (automatic && !document.automatic_updates()) {
                    continue;
                }
                attempted += 1;
                self.profile_attempts
                    .insert(id, chrono::Utc::now().timestamp());
                let mut candidate = document.clone();
                match self.fetch_profile(&mut candidate, false).await {
                    Ok(()) => {
                        let mut batch = documents.clone();
                        if let Some(document) = batch.iter_mut().find(|doc| doc.id == id) {
                            *document = candidate;
                        }
                        match self.commit_profiles(&documents, &batch, selected).await {
                            Ok(()) => documents = batch,
                            Err(error) => errors.push((format!("订阅 {id}"), error)),
                        }
                    }
                    Err(error) => errors.push((format!("订阅 {id}"), error)),
                }
            }
            if !errors.is_empty() {
                return Err(crate::failure::scoped(errors));
            }
            if attempted > 0 {
                self.completed(format!("已更新 {attempted} 项订阅"));
            }
            return Ok(());
        }
        let mut errors = Vec::new();
        // Credentials are read only for downloads or changed fields, never for rendering.
        let vault = crate::profiles::CredentialStore::new(&self.paths);
        let mut secrets = BTreeMap::new();
        let mut previous_secrets = BTreeMap::new();
        for document in &mut documents {
            if document.credentials.changed() {
                let previous = vault.read(document.id).await?;
                let resolved = document.credentials.resolve(previous.clone());
                document.token.clone_from(&resolved.0);
                document.age_key.clone_from(&resolved.1);
                previous_secrets.insert(document.id, previous);
                secrets.insert(document.id, resolved);
            }
            let initial = !old.iter().any(|doc| doc.id == document.id);
            if document.is_remote() && document.content.is_empty() {
                self.fetch_profile(document, initial).await?;
            }
            repository::validate(document)?;
        }
        let selection = if (old.is_empty() && selected.is_none())
            || selected.is_some_and(|id| !documents.iter().any(|doc| doc.id == id))
        {
            documents.first().map(|doc| doc.id)
        } else {
            selected
        };
        // Save credentials before committing a record that depends on them; restore on failure.
        let mut removed = Vec::new();
        let repository = crate::storage::Repository::open(self.paths.clone())
            .map_err(|error| crate::logging::diagnostic_error("无法读取订阅恢复材料", error))?;
        for (position, document) in old
            .iter()
            .enumerate()
            .filter(|(_, doc)| !documents.iter().any(|value| value.id == doc.id))
        {
            let namespace =
                crate::storage::Namespace::parse(&format!("profile-enhance-{}", document.id))
                    .map_err(|error| crate::logging::rejected("订阅增强身份无效", error))?;
            let enhancement = repository
                .read_limited(&namespace, crate::configuration::INPUT_LIMIT)
                .map_err(|error| {
                    crate::logging::diagnostic_error("无法读取订阅增强恢复材料", error)
                })?;
            removed.push(DeletedProfile {
                document: document.clone(),
                position,
                enhancement,
                credentials: vault.read(document.id).await?,
            });
        }
        for (id, (token, age)) in &secrets {
            if let Err(error) = vault.write(*id, token, age).await {
                return Err(match vault.restore(previous_secrets).await {
                    Ok(()) => error,
                    Err(recovery) => format!("{error}；{recovery}"),
                });
            }
        }
        let result = self.commit_profiles(&old, &documents, selection).await;
        if let Err(error) = result {
            return Err(match vault.restore(previous_secrets).await {
                Ok(()) => error,
                Err(recovery) => format!("{error}；{recovery}"),
            });
        } else {
            self.snapshot.operation_committed = true;
            for entry in removed {
                self.deleted_profiles.insert(entry.document.id, entry);
            }
            while self.deleted_profiles.len() > 128
                || self
                    .deleted_profiles
                    .values()
                    .map(|entry| {
                        entry.document.content.len()
                            + entry.enhancement.values().map(String::len).sum::<usize>()
                    })
                    .sum::<usize>()
                    > crate::configuration::INPUT_LIMIT as usize
            {
                self.deleted_profiles.pop_first();
            }
            for document in old
                .iter()
                .filter(|doc| !documents.iter().any(|value| value.id == doc.id))
            {
                if let Err(error) = vault.delete(document.id).await {
                    errors.push(error);
                }
            }
        }
        if !errors.is_empty() {
            return Err(format!("订阅已保存；{}", crate::failure::join(&errors)));
        }
        self.completed("订阅已保存".into());
        Ok(())
    }
    async fn restore_profiles(&mut self, ids: Vec<u64>) -> Result<(), String> {
        let (_, old) = repository::read(&self.paths)?;
        let mut entries = Vec::new();
        for id in ids {
            if old.iter().any(|doc| doc.id == id) {
                continue;
            }
            entries.push(
                self.deleted_profiles
                    .get(&id)
                    .cloned()
                    .ok_or("撤销记录已失效")?,
            );
        }
        if entries.is_empty() {
            return Ok(());
        }
        entries.sort_by_key(|entry| entry.position);
        let mut documents = old.clone();
        for entry in &entries {
            documents.insert(entry.position.min(documents.len()), entry.document.clone());
        }
        let recovery = repository::Recovery::capture(&self.paths, &old, &documents)?;
        let store = crate::storage::Repository::open(self.paths.clone())
            .map_err(|error| crate::logging::diagnostic_error("无法打开订阅恢复存储", error))?;
        let vault = crate::profiles::CredentialStore::new(&self.paths);
        let mut previous = BTreeMap::new();
        let result = async {
            for entry in &entries {
                let id = entry.document.id;
                previous.insert(id, vault.read(id).await?);
                if !entry.credentials.0.is_empty() || !entry.credentials.1.is_empty() {
                    vault
                        .write(id, &entry.credentials.0, &entry.credentials.1)
                        .await?;
                }
                if !entry.enhancement.is_empty() {
                    let namespace =
                        crate::storage::Namespace::parse(&format!("profile-enhance-{id}"))
                            .map_err(|error| crate::logging::rejected("订阅增强身份无效", error))?;
                    store
                        .save(&namespace, &entry.enhancement)
                        .map_err(|error| {
                            crate::logging::diagnostic_error("订阅增强恢复失败", error)
                        })?;
                }
            }
            let selection = crate::configuration::selected_profile(&self.paths)?;
            self.commit_profiles(&old, &documents, selection).await
        }
        .await;
        if let Err(error) = result {
            let mut errors = vec![error];
            if let Err(error) = recovery.restore() {
                errors.push(error);
            }
            if let Err(error) = vault.restore(previous).await {
                errors.push(error);
            }
            if let Err(error) = self.refresh_profiles() {
                errors.push(error);
            }
            return Err(crate::failure::join(&errors));
        }
        for entry in entries {
            self.deleted_profiles.remove(&entry.document.id);
        }
        self.completed("订阅已恢复".into());
        Ok(())
    }
    async fn enhance_profile(
        &mut self,
        id: u64,
        expected: Values,
        values: Values,
    ) -> Result<(), String> {
        let source = crate::configuration::Source::from_profile(&self.paths, id)?;
        crate::configuration::enhancement::apply(source.content(), &values)?;
        let repository = crate::storage::Repository::open(self.paths.clone())
            .map_err(|error| crate::logging::diagnostic_error("无法读取订阅增强存储", error))?;
        let namespace = crate::storage::Namespace::parse(&format!("profile-enhance-{id}"))
            .map_err(|error| crate::logging::rejected("订阅增强身份无效", error))?;
        let previous = repository
            .read_limited(&namespace, crate::configuration::INPUT_LIMIT)
            .map_err(|error| crate::logging::diagnostic_error("无法读取订阅增强", error))?;
        if previous != expected {
            return Err("订阅增强已变更，请重新打开编辑".into());
        }
        let path = repository.document_path(&namespace);
        let old_bytes = match std::fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(crate::logging::diagnostic_error(
                    "无法读取订阅增强文件".into(),
                    error,
                ));
            }
        };
        let active = self.running.as_ref().is_some_and(|running| {
            running
                .launch
                .source
                .profile()
                .is_some_and(|profile| profile.id() == id)
        });
        let candidate = if active {
            let launch = Launch::with_enhancement(&self.paths, source, values.clone()).await?;
            Some(launch)
        } else {
            None
        };
        let write_namespace = namespace.clone();
        let write_values = values.clone();
        let commit = move || {
            repository
                .save(&write_namespace, &write_values)
                .map_err(|error| crate::logging::diagnostic_error("订阅增强保存失败".into(), error))
        };
        let result = if let Some(candidate) = candidate {
            self.apply_profile_candidate(candidate, commit).await
        } else {
            commit()
        };
        if let Err(error) = result {
            let restored = match old_bytes {
                Some(bytes) => crate::storage::atomic_write(&path, &bytes),
                None => match std::fs::remove_file(&path) {
                    Ok(()) => Ok(()),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    Err(error) => Err(error.into()),
                },
            };
            return Err(if restored.is_ok() {
                error
            } else {
                format!("{error}；订阅增强文件恢复失败")
            });
        }
        self.snapshot
            .settings
            .insert(namespace.name().into(), values);
        self.completed("订阅增强已保存".into());
        Ok(())
    }
    pub(super) async fn fetch_profile(
        &mut self,
        document: &mut Document,
        initial: bool,
    ) -> Result<(), String> {
        if self.transfer.is_some() {
            return Err("已有下载正在执行".into());
        }
        let saved = (if document.kind == crate::profiles::model::DocumentKind::Profile {
            crate::profiles::CredentialStore::new(&self.paths)
        } else {
            crate::profiles::CredentialStore::overrides(&self.paths)
        })
        .read(document.id)
        .await?;
        let (token, age) = document.credentials.resolve(saved);
        document.token = token;
        document.age_key = age;
        let preferences = crate::storage::Repository::open(self.paths.clone())
            .map_err(|error| crate::logging::diagnostic_error("无法读取下载设置", error))?;
        let settings =
            crate::preferences::values(&preferences, crate::preferences::SettingsKind::General)
                .map_err(|error| crate::logging::diagnostic_error("无法读取下载设置", error))?;
        let agent = settings
            .get("UserAgent")
            .filter(|value| !value.is_empty())
            .map(String::as_str)
            .unwrap_or(crate::network::DEFAULT_USER_AGENT);
        use super::download::SubscriptionTransport;
        let local = self
            .running
            .as_ref()
            .filter(|_| self.snapshot.phase == Phase::Ready)
            .and(self.snapshot.preferences.as_ref())
            .and_then(|preferences| {
                let values = preferences.values();
                let port = ["MixedPort", "HttpPort"].into_iter().find_map(|key| {
                    values
                        .get(key)
                        .and_then(|value| value.parse::<u16>().ok())
                        .filter(|port| *port > 0)
                })?;
                Some(SubscriptionTransport::Proxy {
                    url: format!("http://127.0.0.1:{port}"),
                    authentication: values
                        .get("Authentication")
                        .and_then(|value| value.lines().next())
                        .filter(|value| !value.is_empty())
                        .map(str::to_owned),
                })
            });
        let strategy = if document.self_proxy {
            local.ok_or("订阅下载代理需要已就绪的内核 HTTP 或混合端口")?
        } else if document.with_proxy {
            SubscriptionTransport::System
        } else {
            SubscriptionTransport::Direct
        };
        if matches!(strategy, SubscriptionTransport::Proxy { .. })
            && let Some(error) = self
                .snapshot
                .preferences
                .as_ref()
                .and_then(super::Preferences::local_proxy_error)
        {
            return Err(error);
        }
        self.progress(
            if document.kind == crate::profiles::model::DocumentKind::Profile {
                "下载订阅"
            } else {
                "下载覆写"
            },
        );
        let (bytes, headers) = self.fetch_profile_once(document, agent, &strategy).await?;
        crate::profiles::remote::apply(document, &bytes, &headers, initial)
    }

    async fn fetch_profile_once(
        &mut self,
        document: &Document,
        agent: &str,
        transport: &super::download::SubscriptionTransport,
    ) -> Result<(Vec<u8>, reqwest::header::HeaderMap), String> {
        self.transfer = Some(super::download::Transfer::subscription(
            &self.http,
            &self.paths,
            document,
            agent,
            transport,
        )?);
        let updates = &self.updates;
        let observer = &self.observer;
        let snapshot = &mut self.snapshot;
        let transfer = self.transfer.as_mut().ok_or("下载任务已失效")?;
        let bytes = transfer
            .finish(|received, total| {
                if let Some(progress) = &mut snapshot.progress {
                    progress.received = received;
                    progress.total = total;
                }
                snapshot.revision = snapshot.revision.saturating_add(1);
                Self::send_snapshot(snapshot, updates, observer);
            })
            .await;
        let headers = transfer.response_headers();
        self.transfer = None;
        Ok((bytes?, headers))
    }

    pub(super) async fn apply_profile_candidate(
        &mut self,
        mut candidate: Launch,
        commit: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), String> {
        let running = self.running.as_ref().ok_or("内核尚未运行")?;
        let client = running.launch.client.clone();
        let previous_yaml = running.launch.yaml().await?;
        let previous: serde_yaml_ng::Value = serde_yaml_ng::from_str(&running.launch.composed)
            .map_err(|error| crate::logging::rejected("旧内核配置无效", error))?;
        candidate.reuse_control(&running.launch).await?;
        let mut candidate = self.validate(candidate).await?;
        if candidate.privileged_network()?
            && self
                .running
                .as_ref()
                .is_some_and(|running| !running.is_service())
            && super::privileged::tun_service_required().await?
        {
            candidate.force_service = true;
            return self.replace_committing(candidate, commit).await;
        }
        let yaml = candidate.yaml().await?;
        let reload = match self.prepare_reload(&candidate).await? {
            Some(path) => path,
            None => return self.replace_committing(candidate, commit).await,
        };
        let expected: serde_yaml_ng::Value = serde_yaml_ng::from_str(&candidate.composed)
            .map_err(|error| crate::logging::rejected("候选内核配置无效", error))?;
        if let Some(settings) = self
            .running
            .as_ref()
            .and_then(|running| running.log_settings.as_ref())
        {
            let (mut redactions, limits) = settings.borrow().clone();
            redactions.extend(candidate.redactions());
            settings.send_replace((redactions, limits));
        }
        self.invalidate_monitor();
        let result = async {
            self.reload_prepared(&reload, &yaml).await?;
            self.snapshot.tun = Some(
                client
                    .wait_configuration_ready(&expected, super::client::CONFIGURATION_READY_TIMEOUT)
                    .await?,
            );
            commit()?;
            self.running
                .as_mut()
                .ok_or("订阅应用期间内核已停止")?
                .launch
                .commit_hot(&candidate, &yaml)?;
            Ok::<(), String>(())
        }
        .await;
        if let Err(error) = result {
            let restored = async {
                self.reload_launch(
                    &self.running.as_ref().ok_or("旧内核不存在")?.launch,
                    &previous_yaml,
                )
                .await?;
                self.snapshot.tun = Some(
                    client
                        .wait_configuration_ready(
                            &previous,
                            super::client::CONFIGURATION_READY_TIMEOUT,
                        )
                        .await?,
                );
                Ok::<(), String>(())
            }
            .await;
            return match restored {
                Ok(()) => Err(format!("{error}；旧内核配置已恢复")),
                Err(recovery) => {
                    let error = format!("{error}；旧内核配置恢复失败：{recovery}");
                    Err(match self.mark_runtime_failed().await {
                        Ok(()) => error,
                        Err(cleanup) => format!("{error}；{cleanup}"),
                    })
                }
            };
        }
        if let Err(error) = self.restore_all_selections().await {
            self.runtime_warning = Some(format!("节点选择恢复失败：{error}"));
        }
        self.snapshot.proxy_names.clear();
        self.snapshot.resources = super::Resources::default();
        self.publish(Phase::Ready, None);
        Ok(())
    }
    async fn commit_profiles(
        &mut self,
        old: &[Document],
        new: &[Document],
        selection: Option<u64>,
    ) -> Result<(), String> {
        let values = store::values(new, selection);
        let selected_source = if let Some(id) = selection {
            crate::configuration::Source::from_profile_values(&values, id)?
        } else {
            crate::configuration::Source::base()
        };
        let active_changed = self.running.as_ref().is_some_and(|running| {
            running.launch.source.profile() != selected_source.profile()
                || selected_source.content() != running.launch.source.content()
        });
        let candidate = if active_changed {
            let source = selected_source;
            let launch = Launch::from_source(&self.paths, source).await?;
            Some(launch)
        } else {
            None
        };
        let recovery = repository::Recovery::capture(&self.paths, old, new)?;
        let paths = self.paths.clone();
        let old = old.to_vec();
        let new = new.to_vec();
        let commit = move || repository::commit(&paths, &old, &new, selection).map(|_| ());
        let result = if let Some(candidate) = candidate {
            self.apply_profile_candidate(candidate, commit).await
        } else {
            commit()
        };
        if let Err(error) = result {
            let restored = recovery.restore();
            let refreshed = self.refresh_profiles();
            let mut errors = vec![error];
            if let Err(recovery) = restored {
                errors.push(recovery);
            }
            if let Err(refresh) = refreshed {
                errors.push(refresh);
            }
            return Err(crate::failure::join(&errors));
        }
        self.refresh_profiles()?;
        self.publish(
            if self.running.is_some() {
                Phase::Ready
            } else {
                Phase::Stopped
            },
            None,
        );
        Ok(())
    }
    pub(super) async fn automatic_profiles(&mut self) -> Result<(), String> {
        let (_, documents) = repository::read(&self.paths)?;
        let now = chrono::Utc::now().timestamp();
        let ids = documents
            .iter()
            .filter(|doc| doc.automatic_updates())
            .filter_map(|doc| {
                let interval = doc.interval.parse::<i64>().ok()?.checked_mul(60)?;
                if interval <= 0 {
                    return None;
                }
                let last = doc
                    .updated_at
                    .unwrap_or(self.profile_schedule_started)
                    .max(self.profile_attempts.get(&doc.id).copied().unwrap_or(0));
                (now.saturating_sub(last) >= interval).then_some(doc.id)
            })
            .collect::<Vec<_>>();
        if ids.is_empty() {
            return Ok(());
        }
        let count = ids.len();
        log::debug!("开始自动更新订阅，共 {count} 项");
        self.snapshot.operation_active = true;
        let result = self
            .profiles(ProfileCommand::Update {
                ids,
                automatic: true,
            })
            .await;
        self.snapshot.operation_active = false;
        self.snapshot.progress = None;
        match &result {
            Ok(()) => log::info!("自动更新订阅完成，共 {count} 项"),
            Err(error) => log::error!("自动更新订阅失败（{error}）"),
        }
        result
    }
}
