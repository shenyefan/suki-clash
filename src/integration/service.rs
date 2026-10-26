//! One storage writer owns ordering; UI readers only access committed snapshots.
use crate::storage::{
    AppPaths, DirectoryKind, Namespace, Repository, StorageError, Values, atomic_write,
};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, RwLock},
    thread::{self, JoinHandle},
};

type Snapshot = Arc<RwLock<BTreeMap<Namespace, Values>>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RequestId(u64);

pub(crate) struct DocumentWrite {
    pub(crate) path: PathBuf,
    pub(crate) content: String,
}

pub(crate) enum Command {
    Save {
        namespace: Namespace,
        values: Values,
        documents: Vec<DocumentWrite>,
    },
    Backup(Namespace),
    Restore(Namespace),
    OpenDirectory(DirectoryKind),
    OpenDocument(PathBuf),
}
impl Command {
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::Save { .. } => "保存配置",
            Self::Backup(_) => "备份本页设置",
            Self::Restore(_) => "读取备份草稿",
            Self::OpenDirectory(_) => "打开应用目录",
            Self::OpenDocument(_) => "打开配置文件",
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
    paths: AppPaths,
    next_id: u64,
    worker: Option<JoinHandle<()>>,
}
impl Service {
    pub(crate) fn start(repository: Repository) -> Result<Self, StorageError> {
        let snapshot = Arc::new(RwLock::new(repository.snapshot()?));
        let writer_snapshot = snapshot.clone();
        let paths = repository.paths().clone();
        let (sender, receiver) = async_channel::bounded::<Request>(64);
        let worker = thread::Builder::new()
            .name("suki-storage".into())
            .spawn(move || {
                while let Ok(request) = receiver.recv_blocking() {
                    let result = execute(&repository, &writer_snapshot, request.command)
                        .map_err(ServiceError::from);
                    // The window may have gone away. Its pending write still completes.
                    let _ = request.reply.try_send(result);
                }
            })?;
        Ok(Self {
            sender,
            snapshot,
            paths,
            next_id: 0,
            worker: Some(worker),
        })
    }
    pub(crate) fn paths(&self) -> &AppPaths {
        &self.paths
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

fn execute(
    repository: &Repository,
    snapshot: &Snapshot,
    command: Command,
) -> Result<Outcome, StorageError> {
    match command {
        Command::Save {
            namespace,
            values,
            documents,
        } => {
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
                        failures.join("；")
                    ))
                    .into());
                }
                return Err(cause);
            }
            snapshot
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
        Command::Backup(namespace) => repository.backup(&namespace).map(Outcome::Backup),
        Command::Restore(namespace) => repository.restore(&namespace).map(Outcome::Restored),
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
    fn worker_drains_accepted_writes_and_publishes_only_committed_values() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        let mut service = Service::start(Repository::open(paths.clone(), None).unwrap()).unwrap();
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
            Repository::open(paths, None)
                .unwrap()
                .read(&namespace())
                .unwrap(),
            values("second")
        );
        assert!(service.submit(Command::Backup(namespace())).is_err());
    }
    #[test]
    fn failed_index_write_restores_document_and_keeps_snapshot_unchanged() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        let repository = Repository::open(paths.clone(), None).unwrap();
        repository.save(&namespace(), &values("before")).unwrap();
        let mut service = Service::start(repository).unwrap();
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
        assert_eq!(service.read(&namespace()), values("before"));
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
        let mut service = Service::start(Repository::open(paths.clone(), None).unwrap()).unwrap();
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
        assert!(service.read(&namespace()).is_empty());
    }
}
