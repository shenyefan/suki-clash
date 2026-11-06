//! Bounded per-file application/core logs with age-based retention, owned off the UI thread.
mod feed;
mod redact;
mod writer;
use crate::storage::{AppPaths, DirectoryKind};
pub(crate) use feed::{Feed, Level, Record, core_record};
pub(crate) use redact::redact;
pub(crate) use writer::{install, start};
/// Record the original cause without changing the error returned to the UI.
#[track_caller]
pub(crate) fn diagnostic_error<T: std::fmt::Display>(
    message: T,
    cause: impl std::fmt::Display,
) -> T {
    report(log::Level::Error, message, cause)
}
/// Input or configuration that failed validation is recoverable by the user, so it is a WARNING.
#[track_caller]
pub(crate) fn rejected<T: std::fmt::Display>(message: T, cause: impl std::fmt::Display) -> T {
    report(log::Level::Warn, message, cause)
}
#[track_caller]
fn report<T: std::fmt::Display>(level: log::Level, message: T, cause: impl std::fmt::Display) -> T {
    let location = std::panic::Location::caller();
    let module = location
        .file()
        .trim_start_matches("src/")
        .trim_end_matches(".rs")
        .replace(['/', '\\'], "::");
    let target = format!("suki_clash::{module}");
    log::log!(
        target: &target,
        level,
        "{message}（{cause:#}），位于 {} 第 {} 行",
        location.file(),
        location.line()
    );
    message
}
#[track_caller]
pub(crate) fn error_message(error: impl std::fmt::Display) -> String {
    diagnostic_error(error.to_string(), format_args!("{error:#}"))
}
pub(crate) fn panic_message(payload: &(dyn std::any::Any + Send)) -> &str {
    payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .unwrap_or("非文本 panic payload")
}
pub(crate) fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!(
            "程序发生 panic，{info}\n{}",
            std::backtrace::Backtrace::force_capture()
        );
        previous(info);
    }));
}
pub(crate) fn error_chain(error: &(dyn std::error::Error + 'static)) -> String {
    let mut details = error.to_string();
    let mut causes = Vec::new();
    let mut source = error.source();
    while let Some(error) = source {
        causes.push(error.to_string());
        source = error.source();
    }
    if !causes.is_empty() {
        details.push_str(&format!("（{}）", causes.join("，")));
    }
    details
}
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write as _},
    path::PathBuf,
    time::{Duration, SystemTime},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    Program,
    Core,
}
impl Source {
    fn prefix(self) -> &'static str {
        match self {
            Self::Program => "program-",
            Self::Core => "core-",
        }
    }
}
pub(crate) struct Sink {
    directory: PathBuf,
    prefix: String,
    file: File,
    size: u64,
    part: u64,
    limit: u64,
    days: u32,
    count: Option<usize>,
}
impl Sink {
    pub(crate) fn new(paths: &AppPaths, source: Source, days: u32, limit: u64) -> io::Result<Self> {
        let directory = paths.directory(DirectoryKind::Logs);
        fs::create_dir_all(&directory)?;
        cleanup(&directory, source.prefix(), days, None)?;
        let mut nonce = [0u8; 8];
        getrandom::fill(&mut nonce).map_err(io::Error::other)?;
        let nonce = nonce
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let prefix = format!(
            "{}{}-{nonce}",
            source.prefix(),
            chrono::Utc::now().format("%Y%m%dT%H%M%S")
        );
        let file = create(&directory.join(format!("{prefix}-0.log")))?;
        Ok(Self {
            directory,
            prefix,
            file,
            size: 0,
            part: 0,
            limit: limit.max(1024),
            days,
            count: None,
        })
    }
    pub(crate) fn configure(&mut self, days: u32, limit: u64) -> io::Result<()> {
        self.days = days;
        self.limit = limit.max(1024);
        cleanup(
            &self.directory,
            if self.prefix.starts_with("core-") {
                "core-"
            } else {
                "program-"
            },
            days,
            Some(
                &self
                    .directory
                    .join(format!("{}-{}.log", self.prefix, self.part)),
            ),
        )
    }
    pub(crate) fn configure_program(
        &mut self,
        days: u32,
        limit: u64,
        count: usize,
    ) -> io::Result<()> {
        self.configure(days, limit)?;
        self.count = Some(count.max(1));
        self.prune_count()
    }
    fn prune_count(&self) -> io::Result<()> {
        let Some(count) = self.count else {
            return Ok(());
        };
        let current = self
            .directory
            .join(format!("{}-{}.log", self.prefix, self.part));
        let mut files = Vec::new();
        for entry in fs::read_dir(&self.directory)? {
            let entry = entry?;
            let metadata = fs::symlink_metadata(entry.path())?;
            if entry.file_name().to_string_lossy().starts_with("program-")
                && entry.path().extension().is_some_and(|ext| ext == "log")
                && metadata.is_file()
                && !metadata.file_type().is_symlink()
                && entry.path() != current
            {
                files.push((metadata.modified()?, entry.path()));
            }
        }
        files.sort();
        let excess = files.len().saturating_add(1).saturating_sub(count);
        for (_, path) in files.into_iter().take(excess) {
            fs::remove_file(path)?;
        }
        Ok(())
    }
    /// One call is one complete UTF-8 record. A large record owns a whole part;
    /// the hard limit rejects it before any file bytes are changed.
    pub(crate) fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        const MAX_RECORD_BYTES: usize = 64 * 1024;
        if bytes.len() > MAX_RECORD_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "单条日志超过64KiB上限",
            ));
        }
        std::str::from_utf8(bytes)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "日志记录不是有效UTF-8"))?;
        if bytes.is_empty() {
            return Ok(());
        }
        if self.size > 0 && self.size.saturating_add(bytes.len() as u64) > self.limit {
            self.file.flush()?;
            let part = self
                .part
                .checked_add(1)
                .ok_or_else(|| io::Error::other("日志分卷编号耗尽"))?;
            let file = create(&self.directory.join(format!("{}-{part}.log", self.prefix)))?;
            self.file = file;
            self.part = part;
            self.size = 0;
            self.prune_count()?;
        }
        self.file.write_all(bytes)?;
        self.size += bytes.len() as u64;
        self.file.flush()
    }
}
fn create(path: &std::path::Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    options.open(path)
}
fn cleanup(
    directory: &std::path::Path,
    prefix: &str,
    days: u32,
    active: Option<&std::path::Path>,
) -> io::Result<()> {
    if days == 0 {
        return Ok(());
    }
    let max_age = Duration::from_secs(days.max(1) as u64 * 86400);
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with(prefix) || !name.ends_with(".log") || active == Some(path.as_path()) {
            continue;
        }
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            continue;
        }
        if metadata
            .modified()
            .ok()
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .is_some_and(|age| age > max_age)
        {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn error_chain_keeps_nested_native_reason_and_display_message_is_unchanged() {
        #[derive(Debug, thiserror::Error)]
        #[error("transport failed")]
        struct Transport(#[source] io::Error);
        let error = Transport(io::Error::from_raw_os_error(13));
        let details = error_chain(&error);
        assert!(details.starts_with("transport failed（"));
        assert!(details.contains("13"));
        assert_eq!(diagnostic_error("原有 Toast", details), "原有 Toast");
    }
    #[test]
    fn logs_rotate_between_complete_records_and_do_not_touch_unowned_files() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        paths.create().unwrap();
        let unrelated = paths.directory(DirectoryKind::Logs).join("unrelated.log");
        fs::write(&unrelated, b"keep").unwrap();
        let mut sink = Sink::new(&paths, Source::Core, 7, 1024).unwrap();
        for _ in 0..3 {
            sink.write("[INFO] 一条完整记录\n".repeat(32).as_bytes())
                .unwrap();
        }
        let logs = fs::read_dir(paths.directory(DirectoryKind::Logs))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("core-")
            })
            .collect::<Vec<_>>();
        assert_eq!(logs.len(), 3);
        assert!(
            logs.iter()
                .all(|path| fs::metadata(path).unwrap().len() <= 1024)
        );
        assert_eq!(fs::read(unrelated).unwrap(), b"keep");
        assert!(logs.iter().all(|path| {
            fs::read_to_string(path)
                .unwrap()
                .ends_with("一条完整记录\n")
        }));
        sink.write("中".repeat(1000).as_bytes()).unwrap();
        let before = sink.size;
        assert!(sink.write(&vec![b'x'; 64 * 1024 + 1]).is_err());
        assert_eq!(sink.size, before);
    }
    #[test]
    fn seven_day_cleanup_preserves_current_file_other_sources_and_unowned_files() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        paths.create().unwrap();
        let directory = paths.directory(DirectoryKind::Logs);
        let past = SystemTime::now() - Duration::from_secs(8 * 86400);
        for name in ["program-expired.log", "core-expired.log", "unrelated.log"] {
            let file = File::create(directory.join(name)).unwrap();
            file.set_times(fs::FileTimes::new().set_modified(past))
                .unwrap();
        }
        let mut sink = Sink::new(&paths, Source::Program, 0, 1024).unwrap();
        let active = directory.join(format!("{}-0.log", sink.prefix));
        sink.file
            .set_times(fs::FileTimes::new().set_modified(past))
            .unwrap();
        sink.configure(7, 1024).unwrap();
        assert!(!directory.join("program-expired.log").exists());
        assert!(directory.join("core-expired.log").exists());
        assert!(directory.join("unrelated.log").exists());
        assert!(active.exists());
        sink.write(b"still recording\n").unwrap();
        assert_eq!(fs::read_to_string(active).unwrap(), "still recording\n");
    }
}
