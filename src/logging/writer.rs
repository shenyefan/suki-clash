//! Program log thread: it owns the log file and applies the settings policy, apart from the storage queue.
use super::{Feed, Level, Record, Sink, Source, feed::Message};
use crate::storage::{Repository, StorageError, Values};
use std::{
    sync::mpsc::{Receiver, RecvTimeoutError, sync_channel},
    thread::{self, JoinHandle},
    time::Duration,
};

const RETRY: Duration = if cfg!(test) {
    Duration::from_millis(50)
} else {
    Duration::from_secs(5)
};
thread_local! {
    /// The log thread reads settings that may log their own failures; those stay in memory.
    static WRITING_LOG: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

type Policy = (Level, u32, u64, usize);

#[derive(Default)]
struct ProgramLog {
    sink: Option<Sink>,
    policy: Option<Policy>,
    failure: Option<String>,
    missed: u64,
}
impl ProgramLog {
    fn reload(&mut self, repository: &Repository, feed: &Feed) {
        let result = (|| -> Result<(), StorageError> {
            let policy = program_policy(repository)?;
            let created = self.sink.is_none();
            if created {
                let sink = Sink::new(repository.paths(), Source::Program, policy.1, policy.2)?;
                feed.track(&sink);
                self.sink = Some(sink);
            }
            if let Some(sink) = &mut self.sink
                && (created || self.policy != Some(policy))
            {
                sink.configure_program(policy.1, policy.2, policy.3)?;
            }
            if self.policy != Some(policy)
                && let Err(error) = prune_script_logs(repository.paths(), policy.1, policy.3)
            {
                feed.publish(Record::program(
                    Level::Warning,
                    "logging",
                    &format!("清理脚本日志失败：{error}"),
                ));
            }
            self.policy = Some(policy);
            feed.set_minimum(policy.0);
            Ok(())
        })();
        match result {
            Ok(()) => {
                if self.failure.take().is_some() {
                    let message =
                        format!("日志文件已恢复（期间 {} 条记录仅保存在内存）", self.missed);
                    self.missed = 0;
                    self.record(feed, Level::Info, &message);
                }
            }
            Err(error) => {
                feed.set_minimum(Level::Trace);
                self.fail(feed, &error);
            }
        }
    }
    fn record(&mut self, feed: &Feed, level: Level, message: &str) {
        let record = Record::program(level, "logging", message);
        feed.publish(record.clone());
        self.write(feed, &record);
    }
    fn write(&mut self, feed: &Feed, record: &Record) {
        let Some(sink) = &mut self.sink else {
            self.missed += 1;
            return;
        };
        if let Err(error) = record
            .parts()
            .into_iter()
            .try_for_each(|part| sink.write(part.line().as_bytes()))
        {
            self.missed += 1;
            self.fail(feed, &error);
        }
    }
    fn fail(&mut self, feed: &Feed, error: &dyn std::fmt::Display) {
        self.sink = None;
        let message = format!("程序日志文件不可用：{error}，记录暂存于内存");
        if self.failure.as_ref() != Some(&message) {
            feed.publish(Record::program(Level::Error, "logging", &message));
            self.failure = Some(message);
        }
    }
}

/// Script execution reports follow the program log retention days and file count.
fn prune_script_logs(
    paths: &crate::storage::AppPaths,
    days: u32,
    count: usize,
) -> std::io::Result<()> {
    let directory = paths
        .directory(crate::storage::DirectoryKind::Logs)
        .join("overrides");
    let entries = match std::fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry?;
        let metadata = std::fs::symlink_metadata(entry.path())?;
        if metadata.is_file()
            && !metadata.file_type().is_symlink()
            && entry.path().extension().is_some_and(|ext| ext == "json")
        {
            files.push((metadata.modified()?, entry.path()));
        }
    }
    files.sort();
    let now = std::time::SystemTime::now();
    let max_age = Duration::from_secs(u64::from(days) * 86400);
    let expired = |modified: &std::time::SystemTime| {
        days > 0 && now.duration_since(*modified).is_ok_and(|age| age > max_age)
    };
    let excess = files.len().saturating_sub(count);
    for (index, (modified, path)) in files.into_iter().enumerate() {
        if index < excess || expired(&modified) {
            std::fs::remove_file(path)?;
        }
    }
    Ok(())
}

pub(crate) fn start(repository: Repository, feed: Feed) -> std::io::Result<JoinHandle<()>> {
    let mut log = ProgramLog::default();
    log.reload(&repository, &feed);
    let (sender, receiver) = sync_channel(super::feed::QUEUE);
    feed.attach(sender);
    thread::Builder::new()
        .name("suki-log".into())
        .spawn(move || run(repository, feed, receiver, log))
}

fn run(repository: Repository, feed: Feed, receiver: Receiver<Message>, mut log: ProgramLog) {
    WRITING_LOG.set(true);
    loop {
        let message = if log.failure.is_some() {
            match receiver.recv_timeout(RETRY) {
                Ok(message) => Some(message),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match receiver.recv() {
                Ok(message) => Some(message),
                Err(_) => break,
            }
        };
        if message.is_some() {
            feed.received();
        }
        if matches!(message, Some(Message::Stop)) {
            break;
        }
        if feed.take_reload() || message.is_none() {
            log.reload(&repository, &feed);
        }
        if let Some(Message::Record(record)) = message
            && feed.allows(record.level)
        {
            log.write(&feed, &record);
        }
        let dropped = feed.take_dropped();
        if dropped > 0 {
            log.record(
                &feed,
                Level::Warning,
                &format!("日志队列已满，丢弃 {dropped} 条日志"),
            );
        }
    }
}

/// Route `log` macros of this application to the program log.
pub(crate) fn install(feed: Feed) {
    if let Err(error) = log::set_boxed_logger(Box::new(ProgramLogger(feed.clone()))) {
        feed.emit(Level::Error, "logging", &error.to_string());
        return;
    }
    log::set_max_level(log::LevelFilter::Trace);
}
struct ProgramLogger(Feed);
impl log::Log for ProgramLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        ["suki_clash", "suki_clash_service"].iter().any(|root| {
            metadata.target() == *root || metadata.target().starts_with(&format!("{root}::"))
        }) && self.0.allows(Level::parse(metadata.level().as_str()))
    }
    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let level = Level::parse(record.level().as_str());
        let module = record
            .target()
            .strip_prefix("suki_clash::")
            .or_else(|| record.target().strip_prefix("suki_clash_service::"))
            .unwrap_or("application");
        let message = record.args().to_string();
        if WRITING_LOG.get() {
            self.0.publish(Record::program(level, module, &message));
        } else {
            self.0.emit(level, module, &message);
        }
    }
    fn flush(&self) {}
}

fn program_policy(repository: &Repository) -> Result<Policy, StorageError> {
    let values = crate::preferences::values(repository, crate::preferences::SettingsKind::General)?;
    crate::preferences::validate(crate::preferences::SettingsKind::General, &values)
        .map_err(std::io::Error::other)?;
    policy_values(&values)
}
fn policy_values(values: &Values) -> Result<Policy, StorageError> {
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
        Level::parse(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{AppPaths, DirectoryKind};

    fn saved(paths: &AppPaths) -> String {
        std::fs::read_dir(paths.directory(DirectoryKind::Logs))
            .map(|entries| {
                entries
                    .filter_map(|entry| std::fs::read_to_string(entry.unwrap().path()).ok())
                    .collect()
            })
            .unwrap_or_default()
    }
    fn wait(condition: impl Fn() -> bool) {
        for _ in 0..200 {
            if condition() {
                return;
            }
            thread::sleep(Duration::from_millis(25));
        }
        panic!("等待日志线程超时");
    }
    fn prepared() -> (tempfile::TempDir, AppPaths, Repository) {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        let repository = Repository::open(paths.clone()).unwrap();
        crate::preferences::initialize(&repository).unwrap();
        (root, paths, repository)
    }

    #[test]
    fn selected_level_filters_before_memory_and_file_and_reloads_after_settings_change() {
        let (_root, paths, repository) = prepared();
        let feed = Feed::default();
        let worker = start(repository.clone(), feed.clone()).unwrap();
        feed.emit(Level::Debug, "storage", "hidden-debug");
        feed.emit(Level::Error, "profiles", "visible-error");
        let mut values = crate::preferences::defaults(crate::preferences::SettingsKind::General);
        values.insert("AppLogLevel".into(), "trace".into());
        repository
            .save(
                &crate::storage::Namespace::parse("settings").unwrap(),
                &values,
            )
            .unwrap();
        feed.reload();
        wait(|| feed.allows(Level::Trace));
        feed.emit(Level::Debug, "storage", "visible-debug");
        feed.close();
        worker.join().unwrap();
        let memory = feed.since(0);
        assert!(memory.iter().all(|record| record.message != "hidden-debug"));
        assert!(
            memory
                .iter()
                .any(|record| record.message == "visible-debug")
        );
        let text = saved(&paths);
        assert!(!text.contains("hidden-debug"));
        assert!(text.contains("[storage] visible-debug"));
        assert!(text.contains("[profiles] visible-error"));
    }

    #[test]
    fn script_reports_follow_program_log_retention() {
        let (_root, paths, _repository) = prepared();
        let directory = paths.directory(DirectoryKind::Logs).join("overrides");
        std::fs::create_dir_all(&directory).unwrap();
        for id in 0..5 {
            std::fs::write(directory.join(format!("{id}.json")), b"{}").unwrap();
        }
        let old = std::fs::File::options()
            .write(true)
            .open(directory.join("0.json"))
            .unwrap();
        old.set_modified(std::time::SystemTime::now() - Duration::from_secs(10 * 86400))
            .unwrap();
        prune_script_logs(&paths, 7, 3).unwrap();
        let mut left = std::fs::read_dir(&directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        left.sort();
        assert_eq!(left.len(), 3);
        assert!(!left.contains(&"0.json".to_string()));
        prune_script_logs(&paths, 0, 1).unwrap();
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
    }

    #[test]
    fn broken_settings_keep_memory_diagnostics_and_recover_after_repair() {
        let (_root, paths, repository) = prepared();
        let settings = paths.settings().join("settings.json");
        let original = std::fs::read(&settings).unwrap();
        std::fs::write(&settings, br#"{"schema_version":1,"general":false}"#).unwrap();
        let feed = Feed::default();
        let worker = start(repository.clone(), feed.clone()).unwrap();
        feed.emit(Level::Error, "core", "while-broken");
        std::fs::write(&settings, original).unwrap();
        wait(|| saved(&paths).contains("日志文件已恢复"));
        feed.emit(Level::Error, "core", "after-repair");
        feed.close();
        worker.join().unwrap();
        assert!(
            feed.since(0)
                .iter()
                .any(|record| record.message == "while-broken")
        );
        let text = saved(&paths);
        assert!(text.contains("after-repair"));
        assert!(!text.contains("while-broken"));
    }

    #[test]
    fn file_failure_is_reported_once_and_recovery_is_written_with_the_gap() {
        let (_root, paths, repository) = prepared();
        let directory = paths.directory(DirectoryKind::Logs);
        std::fs::remove_dir_all(&directory).unwrap();
        std::fs::write(&directory, b"blocked").unwrap();
        let feed = Feed::default();
        let worker = start(repository.clone(), feed.clone()).unwrap();
        feed.emit(Level::Error, "core", "first");
        feed.emit(Level::Error, "core", "second");
        wait(|| {
            feed.since(0)
                .iter()
                .any(|record| record.message == "second")
        });
        assert_eq!(
            feed.since(0)
                .iter()
                .filter(|record| record.origin == "logging")
                .count(),
            1
        );
        std::fs::remove_file(&directory).unwrap();
        std::fs::create_dir_all(&directory).unwrap();
        wait(|| saved(&paths).contains("日志文件已恢复"));
        feed.close();
        worker.join().unwrap();
        assert!(saved(&paths).contains("期间 2 条记录仅保存在内存"));
    }
}
