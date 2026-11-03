//! One storage writer owns ordering; UI readers only access committed snapshots.
use crate::storage::{AppPaths, DirectoryKind, Namespace, Repository, StorageError, Values};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, RwLock},
    thread::{self, JoinHandle},
};

#[cfg(test)]
use crate::storage::atomic_write;

type Snapshot = Arc<RwLock<BTreeMap<Namespace, Values>>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RequestId(u64);

#[cfg(test)]
pub(crate) struct DocumentWrite {
    pub(crate) path: PathBuf,
    pub(crate) content: String,
}

pub(crate) enum Command {
    /// Queue barrier: prior durable operations have returned before this replies.
    Flush,
    Log {
        level: &'static str,
        module: &'static str,
        message: String,
    },
    #[cfg(test)]
    Save {
        namespace: Namespace,
        values: Values,
        documents: Vec<DocumentWrite>,
    },
    Backup(Namespace),
    Restore(Namespace),
    OpenDirectory(DirectoryKind),
    OpenDocument(PathBuf),
    Diagnostics,
}
impl Command {
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::Flush => "等待存储任务",
            Self::Log { .. } => "写入程序日志",
            #[cfg(test)]
            Self::Save { .. } => "保存配置",
            Self::Backup(_) => "备份本页设置",
            Self::Restore(_) => "读取备份草稿",
            Self::OpenDirectory(_) => "打开应用目录",
            Self::OpenDocument(_) => "打开配置文件",
            Self::Diagnostics => "导出诊断信息",
        }
    }
}
pub(crate) enum Outcome {
    Saved,
    Backup(PathBuf),
    Restored(Values),
    Directory(PathBuf),
    Document(PathBuf),
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ServiceError {
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error("{0}")]
    Unavailable(String),
    #[error("后台存储执行端已停止")]
    Closed,
    #[error("存储任务较多，请稍后重试")]
    QueueFull,
    #[error("请求编号已耗尽")]
    RequestIdExhausted,
}
type Response = Result<Outcome, ServiceError>;
struct Request {
    command: Command,
    reply: async_channel::Sender<Response>,
}
pub(crate) struct Pending {
    id: RequestId,
    reply: async_channel::Receiver<Response>,
}
impl Pending {
    pub(crate) fn id(&self) -> RequestId {
        self.id
    }
    pub(crate) async fn receive(self) -> Response {
        self.reply.recv().await.map_err(|_| ServiceError::Closed)?
    }
}

pub(crate) struct Service {
    sender: async_channel::Sender<Request>,
    snapshot: Snapshot,
    paths: Option<AppPaths>,
    startup_error: Option<String>,
    next_id: u64,
    logs: crate::logging::Feed,
    worker: Option<JoinHandle<()>>,
}
impl Service {
    pub(crate) fn start(repository: Repository) -> Result<Self, StorageError> {
        let startup_error = crate::preferences::initialize(&repository)
            .and_then(|()| {
                if let Some(values) = repository
                    .snapshot()?
                    .get(&Namespace::parse("profile-index")?)
                {
                    crate::profiles::repository::decode(values).map_err(std::io::Error::other)?;
                }
                Ok(())
            })
            .err()
            .map(|error| error.to_string());
        let mut initial = repository.available_snapshot();
        if let Some(values) = initial.get(&Namespace::parse("profile-index")?)
            && crate::profiles::repository::decode(values).is_err()
        {
            initial.remove(&Namespace::parse("profile-index")?);
        }
        if let Ok(preferences) = crate::core::Preferences::load(repository.paths()) {
            initial.insert(Namespace::parse("core")?, preferences.values());
        }
        let snapshot = Arc::new(RwLock::new(initial));
        let writer_snapshot = snapshot.clone();
        let paths = repository.paths().clone();
        let (sender, receiver) = async_channel::bounded::<Request>(64);
        let logs = crate::logging::Feed::default();
        let writer_logs = logs.clone();
        // Diagnostic logging remains available when its settings cannot be read.
        // This policy does not replace or persist the invalid user configuration.
        let mut log = ProgramLog::new(&repository, &logs)?;
        if let Some(error) = &startup_error {
            let record =
                crate::logging::Record::program(crate::logging::Level::Error, "storage", error);
            logs.publish(record.clone());
            if log.sink.is_some() {
                let _ = log.write(&logs, &record);
            }
        }
        let worker = thread::Builder::new()
            .name("suki-storage".into())
            .spawn(move || {
                while let Ok(request) = receiver.recv_blocking() {
                    if let Command::Log {
                        level,
                        module,
                        message,
                    } = &request.command
                    {
                        let result = (|| -> Result<(), StorageError> {
                            let configured = log.configure(&repository, &writer_logs);
                            let minimum = if configured.is_ok() { log.policy.0 } else { crate::logging::Level::Trace };
                            let level = crate::logging::Level::parse(level);
                            if level < minimum || minimum == crate::logging::Level::Silent { return Ok(()); }
                            let record = crate::logging::Record::program(level, module, message);
                            writer_logs.publish(record.clone());
                            configured?;
                            log.write(&writer_logs, &record)
                        })();
                        let _ = request
                            .reply
                            .try_send(result.map(|()| Outcome::Saved).map_err(ServiceError::from));
                        continue;
                    }
                    #[cfg(not(test))]
                    let log_settings_changed=false;
                    #[cfg(test)]
                    let log_settings_changed = matches!(&request.command, Command::Save { namespace, .. } if namespace.name() == "settings");
                    let mut result = execute(&repository, &writer_snapshot, request.command)
                        .map_err(ServiceError::from);
                    if result.is_ok() && log_settings_changed {
                        let applied = log.configure(&repository, &writer_logs);
                        if let Err(error) = applied {
                            result = Err(StorageError::from(std::io::Error::other(format!("设置已保存，程序日志设置应用失败：{error}"))).into());
                        }
                    }
                    // The window may have gone away. Its pending write still completes.
                    let _ = request.reply.try_send(result);
                }
            })?;
        Ok(Self {
            sender,
            snapshot,
            paths: Some(paths),
            startup_error,
            next_id: 0,
            logs,
            worker: Some(worker),
        })
    }
    pub(crate) fn logs(&self) -> crate::logging::Feed {
        self.logs.clone()
    }
    pub(crate) fn paths(&self) -> Option<&AppPaths> {
        self.paths.as_ref()
    }
    #[cfg(test)]
    pub(crate) fn startup_error(&self) -> Option<&str> {
        self.startup_error.as_deref()
    }
    pub(crate) fn unavailable(paths: Option<AppPaths>, message: String) -> Self {
        let snapshot = paths
            .as_ref()
            .and_then(|paths| Repository::open(paths.clone()).ok())
            .map(|repository| repository.available_snapshot())
            .unwrap_or_default();
        let (sender, receiver) = async_channel::bounded(1);
        receiver.close();
        let logs = crate::logging::Feed::default();
        logs.publish(crate::logging::Record::program(
            crate::logging::Level::Error,
            "storage",
            &message,
        ));
        Self {
            sender,
            snapshot: Arc::new(RwLock::new(snapshot)),
            paths,
            startup_error: Some(message),
            next_id: 0,
            logs,
            worker: None,
        }
    }
    pub(crate) fn read(&self, namespace: &Namespace) -> Values {
        self.snapshot
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .get(namespace)
            .cloned()
            .unwrap_or_default()
    }
    pub(crate) fn submit(&mut self, command: Command) -> Result<Pending, ServiceError> {
        if self.worker.is_none()
            && let Some(error) = &self.startup_error
        {
            return Err(ServiceError::Unavailable(error.clone()));
        }
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or(ServiceError::RequestIdExhausted)?;
        let (reply, receiver) = async_channel::bounded(1);
        self.sender
            .try_send(Request { command, reply })
            .map_err(|error| match error {
                async_channel::TrySendError::Full(_) => ServiceError::QueueFull,
                async_channel::TrySendError::Closed(_) => ServiceError::Closed,
            })?;
        Ok(Pending {
            id: RequestId(self.next_id),
            reply: receiver,
        })
    }
    pub(crate) fn flush(&mut self) -> Result<Option<Pending>, ServiceError> {
        if self.worker.is_none() && self.startup_error.is_some() {
            return Ok(None);
        }
        self.submit(Command::Flush).map(Some)
    }
    pub(crate) fn shutdown(&mut self) -> Option<JoinHandle<()>> {
        self.sender.close();
        self.worker.take()
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        self.sender.close();
    }
}

/// The storage worker remains alive when only its optional file writer is broken.
struct ProgramLog {
    sink: Option<crate::logging::Sink>,
    policy: (crate::logging::Level, u32, u64, usize),
    failure: Option<String>,
}
impl ProgramLog {
    fn new(repository: &Repository, feed: &crate::logging::Feed) -> Result<Self, StorageError> {
        let mut writer = Self {
            sink: None,
            policy: policy_values(&crate::preferences::defaults(
                crate::preferences::SettingsKind::General,
            ))?,
            failure: None,
        };
        let _ = writer.configure(repository, feed);
        Ok(writer)
    }
    fn configure(
        &mut self,
        repository: &Repository,
        feed: &crate::logging::Feed,
    ) -> Result<(), StorageError> {
        let result = (|| {
            let policy = program_policy(repository)?;
            let created = self.sink.is_none();
            if created {
                self.sink = Some(crate::logging::Sink::new(
                    repository.paths(),
                    crate::logging::Source::Program,
                    policy.1,
                    policy.2,
                )?);
            }
            if let Some(sink) = &mut self.sink
                && (created || self.policy != policy || self.failure.is_some())
            {
                sink.configure_program(policy.1, policy.2, policy.3)?;
            }
            self.policy = policy;
            Ok(())
        })();
        if let Err(error) = &result {
            self.fail(feed, error);
        }
        result
    }
    fn write(
        &mut self,
        feed: &crate::logging::Feed,
        record: &crate::logging::Record,
    ) -> Result<(), StorageError> {
        let result = self
            .sink
            .as_mut()
            .ok_or_else(|| StorageError::from(std::io::Error::other("程序日志文件不可用")))?
            .write(record.line().as_bytes())
            .map_err(StorageError::from);
        if let Err(error) = &result {
            self.fail(feed, error);
        } else {
            self.failure = None;
        }
        result
    }
    fn fail(&mut self, feed: &crate::logging::Feed, error: &StorageError) {
        self.sink = None;
        let error = format!("程序日志文件不可用；保留内存诊断：{error}");
        if self.failure.as_ref() != Some(&error) {
            feed.publish(crate::logging::Record::program(
                crate::logging::Level::Error,
                "logging",
                &error,
            ));
            self.failure = Some(error);
        }
    }
}

fn program_policy(
    repository: &Repository,
) -> Result<(crate::logging::Level, u32, u64, usize), StorageError> {
    let values = crate::preferences::values(repository, crate::preferences::SettingsKind::General)?;
    crate::preferences::validate(crate::preferences::SettingsKind::General, &values)
        .map_err(std::io::Error::other)?;
    policy_values(&values)
}

fn policy_values(
    values: &Values,
) -> Result<(crate::logging::Level, u32, u64, usize), StorageError> {
    let number = |key: &str| -> Result<u64, StorageError> {
        values
            .get(key)
            .and_then(|value| value.parse().ok())
            .ok_or_else(|| std::io::Error::other(format!("日志设置 {key} 无效")).into())
    };
    let days = match values.get("AutoLogClean").map(String::as_str) {
        Some("1d") => 1,
        Some("7d") => 7,
        Some("30d") => 30,
        Some("90d") => 90,
        _ => 0,
    };
    Ok((
        crate::logging::Level::parse(
            values
                .get("AppLogLevel")
                .map(String::as_str)
                .unwrap_or("info"),
        ),
        days,
        number("AppLogMaxSize")?.saturating_mul(1024),
        number("AppLogMaxCount")? as usize,
    ))
}

fn execute(
    repository: &Repository,
    _snapshot: &Snapshot,
    command: Command,
) -> Result<Outcome, StorageError> {
    match command {
        Command::Flush => Ok(Outcome::Saved),
        Command::Log { .. } => Ok(Outcome::Saved),
        #[cfg(test)]
        Command::Save {
            namespace,
            values,
            documents,
        } => {
            // Do not replace a damaged document with a form derived from defaults.
            repository.read(&namespace)?;
            let mut previous = Vec::new();
            for document in &documents {
                // Paths originate in the app, but stay inside this repository even if a caller errs.
                if !document.path.starts_with(repository.paths().data())
                    || document
                        .path
                        .components()
                        .any(|part| matches!(part, std::path::Component::ParentDir))
                {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "配置文件路径超出应用数据目录",
                    )
                    .into());
                }
                let old = match std::fs::read(&document.path) {
                    Ok(bytes) => Some(bytes),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                    Err(error) => return Err(error.into()),
                };
                previous.push((document.path.clone(), old));
            }
            let result = (|| {
                for document in &documents {
                    atomic_write(&document.path, document.content.as_bytes())?;
                }
                repository.save(&namespace, &values)
            })();
            if let Err(cause) = result {
                let mut failures = Vec::new();
                for (path, bytes) in previous {
                    let restored = if let Some(bytes) = bytes {
                        atomic_write(&path, &bytes)
                    } else {
                        match std::fs::remove_file(&path) {
                            Ok(()) => Ok(()),
                            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                            Err(error) => Err(error.into()),
                        }
                    };
                    if let Err(error) = restored {
                        failures.push(error.to_string());
                    }
                }
                if !failures.is_empty() {
                    return Err(std::io::Error::other(format!(
                        "{cause}；文件恢复也失败：{}",
                        crate::failure::join(&failures)
                    ))
                    .into());
                }
                return Err(cause);
            }
            _snapshot
                .write()
                .unwrap_or_else(|error| error.into_inner())
                .insert(namespace, values);
            Ok(Outcome::Saved)
        }
        Command::OpenDocument(path) => {
            if !path.starts_with(repository.paths().data())
                || path
                    .components()
                    .any(|part| matches!(part, std::path::Component::ParentDir))
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "配置文件路径超出应用数据目录",
                )
                .into());
            }
            if !std::fs::metadata(&path)?.is_file() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "配置路径不是文件",
                )
                .into());
            }
            Ok(Outcome::Document(path))
        }
        Command::Backup(namespace) if matches!(namespace.name(), "core" | "resources") => {
            let preferences = crate::configuration::Preferences::load(repository.paths())
                .map_err(std::io::Error::other)?;
            repository
                .backup_values(&namespace, &preferences.values())
                .map(Outcome::Backup)
        }
        Command::Backup(namespace) => repository.backup(&namespace).map(Outcome::Backup),
        Command::Restore(namespace) => repository.restore(&namespace).map(Outcome::Restored),
        Command::Diagnostics => crate::diagnostics::export(repository)
            .map(Outcome::Document)
            .map_err(|error| std::io::Error::other(error).into()),
        Command::OpenDirectory(kind) => Ok(Outcome::Directory(repository.paths().directory(kind))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn namespace() -> Namespace {
        Namespace::parse("settings").unwrap()
    }
    fn values(value: &str) -> Values {
        Values::from([("theme".into(), value.into())])
    }
    #[test]
    fn exit_barrier_finishes_prior_writes_without_destroying_storage() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        let mut service = Service::start(Repository::open(paths.clone()).unwrap()).unwrap();
        let namespace = Namespace::parse("dns").unwrap();
        let first = crate::preferences::defaults(crate::preferences::SettingsKind::Dns);
        let save = service
            .submit(Command::Save {
                namespace: namespace.clone(),
                values: first.clone(),
                documents: vec![],
            })
            .unwrap();
        service
            .flush()
            .unwrap()
            .unwrap()
            .reply
            .recv_blocking()
            .unwrap()
            .unwrap();
        assert!(save.reply.recv_blocking().unwrap().is_ok());
        assert_eq!(service.read(&namespace), first);
        let directory = service
            .submit(Command::OpenDirectory(DirectoryKind::Config))
            .unwrap();
        assert!(matches!(
            directory.reply.recv_blocking().unwrap(),
            Ok(Outcome::Directory(_))
        ));
        service.shutdown().unwrap().join().unwrap();
    }
    #[test]
    fn damaged_settings_keep_logging_and_unrelated_writes_available_and_recover_after_repair() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        let repository = Repository::open(paths.clone()).unwrap();
        crate::preferences::initialize(&repository).unwrap();
        let broken = paths.settings().join("settings.json");
        let original = std::fs::read(&broken).unwrap();
        std::fs::write(&broken, br#"{"schema_version":1,"general":false}"#).unwrap();
        let mut service = Service::start(repository).unwrap();
        assert!(service.startup_error().unwrap().contains("settings.json"));
        let log = service
            .submit(Command::Log {
                level: "ERROR",
                module: "core",
                message: "retry-error".into(),
            })
            .unwrap();
        assert!(log.reply.recv_blocking().unwrap().is_err());
        let save = service
            .submit(Command::Save {
                namespace: Namespace::parse("dns").unwrap(),
                values: crate::preferences::defaults(crate::preferences::SettingsKind::Dns),
                documents: vec![],
            })
            .unwrap();
        assert!(save.reply.recv_blocking().unwrap().is_ok());
        let rejected = service
            .submit(Command::Save {
                namespace: namespace(),
                values: values("new"),
                documents: vec![],
            })
            .unwrap();
        assert!(rejected.reply.recv_blocking().unwrap().is_err());
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&std::fs::read(&broken).unwrap()).unwrap()
                ["general"],
            false
        );
        std::fs::write(&broken, original).unwrap();
        let saved = service
            .submit(Command::Save {
                namespace: namespace(),
                values: crate::preferences::defaults(crate::preferences::SettingsKind::General),
                documents: vec![],
            })
            .unwrap();
        assert!(saved.reply.recv_blocking().unwrap().is_ok());
        let recovered = service
            .submit(Command::Log {
                level: "ERROR",
                module: "core",
                message: "recovered".into(),
            })
            .unwrap();
        assert!(recovered.reply.recv_blocking().unwrap().is_ok());
        service.shutdown().unwrap().join().unwrap();
        let records = service.logs().since(0);
        assert!(
            records
                .iter()
                .any(|record| record.message.contains("settings.json"))
        );
        assert!(records.iter().any(|record| record.message == "retry-error"));
        let text = std::fs::read_dir(paths.directory(DirectoryKind::Logs))
            .unwrap()
            .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
            .collect::<String>();
        assert!(text.contains("recovered"));
        assert!(!text.contains("retry-error"));
    }
    #[test]
    fn file_log_failure_keeps_commands_live_and_recovers_on_the_next_request() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        let repository = Repository::open(paths.clone()).unwrap();
        let log_dir = paths.directory(DirectoryKind::Logs);
        std::fs::remove_dir(&log_dir).unwrap();
        std::fs::write(&log_dir, b"blocked").unwrap();
        let mut service = Service::start(repository).unwrap();
        let feed = service.logs();
        for message in ["first", "second"] {
            let request = service
                .submit(Command::Log {
                    level: "ERROR",
                    module: "core",
                    message: message.into(),
                })
                .unwrap();
            assert!(request.reply.recv_blocking().unwrap().is_err());
        }
        assert_eq!(
            feed.since(0)
                .iter()
                .filter(|record| record.origin == "logging")
                .count(),
            1
        );
        for command in [
            Command::OpenDirectory(DirectoryKind::Config),
            Command::Diagnostics,
        ] {
            let request = service.submit(command).unwrap();
            assert!(request.reply.recv_blocking().unwrap().is_ok());
        }
        std::fs::remove_file(&log_dir).unwrap();
        std::fs::create_dir_all(&log_dir).unwrap();
        let request = service
            .submit(Command::Log {
                level: "ERROR",
                module: "core",
                message: "recovered".into(),
            })
            .unwrap();
        assert!(request.reply.recv_blocking().unwrap().is_ok());
        service.shutdown().unwrap().join().unwrap();
        let saved = std::fs::read_dir(log_dir)
            .unwrap()
            .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
            .collect::<String>();
        assert!(saved.contains("recovered"));
        assert!(feed.since(0).iter().any(|record| record.message == "first"));
    }
    #[test]
    fn unavailable_storage_retains_healthy_settings_and_rejects_writes() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        let repository = Repository::open(paths.clone()).unwrap();
        let dns = Namespace::parse("profile-enhance-1").unwrap();
        let healthy = Values::from([("rules".into(), "prepend: []".into())]);
        repository.save(&dns, &healthy).unwrap();
        let healthy = repository.read(&dns).unwrap();
        let broken = paths.settings().join("settings.json");
        std::fs::write(&broken, b"broken").unwrap();
        let mut service = Service::unavailable(Some(paths), "设置读取失败".into());
        assert_eq!(service.read(&dns), healthy);
        assert!(matches!(
            service.submit(Command::Save {
                namespace: namespace(),
                values: Values::new(),
                documents: vec![]
            }),
            Err(ServiceError::Unavailable(_))
        ));
        assert_eq!(std::fs::read(broken).unwrap(), b"broken");
        assert!(service.shutdown().is_none());
        let mut missing = Service::unavailable(None, "无法确定应用目录".into());
        assert!(missing.paths().is_none());
        assert!(
            missing
                .submit(Command::OpenDirectory(DirectoryKind::Config))
                .is_err()
        );
    }
    #[test]
    fn program_level_and_retention_hot_update_without_changing_core_logs() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        let mut service = Service::start(Repository::open(paths.clone()).unwrap()).unwrap();
        let feed = service.logs();
        service
            .submit(Command::Log {
                level: "DEBUG",
                module: "storage",
                message: "hidden-debug".into(),
            })
            .unwrap();
        service
            .submit(Command::Log {
                level: "ERROR",
                module: "profiles",
                message: "visible-error".into(),
            })
            .unwrap();
        let mut values = crate::preferences::defaults(crate::preferences::SettingsKind::General);
        values.insert("AppLogLevel".into(), "trace".into());
        values.insert("AppLogMaxSize".into(), "1".into());
        values.insert("AppLogMaxCount".into(), "2".into());
        service
            .submit(Command::Save {
                namespace: namespace(),
                values,
                documents: vec![],
            })
            .unwrap();
        let core_path = paths.directory(DirectoryKind::Logs).join("core-retain.log");
        std::fs::write(&core_path, b"core").unwrap();
        for _ in 0..5 {
            service
                .submit(Command::Log {
                    level: "DEBUG",
                    module: "settings",
                    message: "x".repeat(900),
                })
                .unwrap();
        }
        service.shutdown().unwrap().join().unwrap();
        let records = feed.since(0);
        assert_eq!(records.len(), 6);
        assert_eq!(records[0].origin, "profiles");
        assert!(
            records
                .iter()
                .all(|record| record.source == crate::logging::Source::Program)
        );
        assert!(
            !records
                .iter()
                .any(|record| record.message == "hidden-debug")
        );
        let files = std::fs::read_dir(paths.directory(DirectoryKind::Logs))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("program-")
            })
            .collect::<Vec<_>>();
        assert_eq!(files.len(), 2);
        assert!(
            files
                .iter()
                .all(|path| std::fs::metadata(path).unwrap().len() <= 1024)
        );
        let text = files
            .iter()
            .map(|path| std::fs::read_to_string(path).unwrap())
            .collect::<String>();
        assert!(text.contains("[settings]"));
        assert_eq!(std::fs::read(core_path).unwrap(), b"core");
    }
    #[test]
    fn worker_drains_accepted_writes_and_publishes_only_committed_values() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        let mut service = Service::start(Repository::open(paths.clone()).unwrap()).unwrap();
        let first = service
            .submit(Command::Save {
                namespace: namespace(),
                values: values("first"),
                documents: vec![],
            })
            .unwrap();
        let second = service
            .submit(Command::Save {
                namespace: namespace(),
                values: values("second"),
                documents: vec![],
            })
            .unwrap();
        assert!(first.id() < second.id());
        service.shutdown().unwrap().join().unwrap();
        assert!(first.reply.recv_blocking().unwrap().is_ok());
        assert!(second.reply.recv_blocking().unwrap().is_ok());
        assert_eq!(service.read(&namespace()), values("second"));
        assert_eq!(
            Repository::open(paths).unwrap().read(&namespace()).unwrap(),
            values("second")
        );
        assert!(service.submit(Command::Backup(namespace())).is_err());
    }
    #[test]
    fn failed_index_write_restores_document_and_keeps_snapshot_unchanged() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        let repository = Repository::open(paths.clone()).unwrap();
        repository.save(&namespace(), &values("before")).unwrap();
        let mut service = Service::start(repository).unwrap();
        let before_snapshot = service.read(&namespace());
        let file = paths.data().join("profiles/3.yaml");
        atomic_write(&file, b"before").unwrap();
        fs_make_index_unwritable(&paths);
        let pending = service
            .submit(Command::Save {
                namespace: namespace(),
                values: values("after"),
                documents: vec![DocumentWrite {
                    path: file.clone(),
                    content: "after".into(),
                }],
            })
            .unwrap();
        service.shutdown().unwrap().join().unwrap();
        assert!(pending.reply.recv_blocking().unwrap().is_err());
        assert_eq!(std::fs::read(file).unwrap(), b"before");
        assert_eq!(service.read(&namespace()), before_snapshot);
    }
    fn fs_make_index_unwritable(paths: &AppPaths) {
        let path = paths.settings().join("settings.json");
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
    }
    #[test]
    fn escaped_document_path_cannot_write_outside_repository() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        let mut service = Service::start(Repository::open(paths.clone()).unwrap()).unwrap();
        let before_snapshot = service.read(&namespace());
        let outside = root.path().join("outside.yaml");
        let pending = service
            .submit(Command::Save {
                namespace: namespace(),
                values: values("after"),
                documents: vec![DocumentWrite {
                    path: outside.clone(),
                    content: "secret".into(),
                }],
            })
            .unwrap();
        service.shutdown().unwrap().join().unwrap();
        assert!(pending.reply.recv_blocking().unwrap().is_err());
        assert!(!outside.exists());
        assert_eq!(service.read(&namespace()), before_snapshot);
    }
}
