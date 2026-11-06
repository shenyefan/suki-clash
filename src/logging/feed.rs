//! Bounded live snapshots shared by background producers and the foreground reader.
use super::Source;
use std::{
    collections::{BTreeSet, VecDeque},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering},
        mpsc::{SyncSender, TrySendError},
    },
};

/// Pending records for the log thread. Above three quarters only WARNING and
/// ERROR are accepted so a burst cannot push out the failures that explain it.
pub(crate) const QUEUE: usize = 4096;
pub(crate) enum Message {
    Record(Record),
    Wake,
    Stop,
}
#[derive(Default)]
struct Shared {
    minimum: AtomicU8,
    reload: AtomicBool,
    dropped: AtomicU64,
    queued: AtomicUsize,
    queue: OnceLock<SyncSender<Message>>,
}

pub(crate) const CAPACITY: usize = 2000;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Level {
    Trace,
    Debug,
    Info,
    Warning,
    Error,
    Silent,
}
impl Level {
    pub(crate) fn parse(value: &str) -> Self {
        match value.to_ascii_lowercase().as_str() {
            "trace" => Self::Trace,
            "debug" => Self::Debug,
            "warn" | "warning" => Self::Warning,
            "error" | "fatal" | "panic" => Self::Error,
            "silent" => Self::Silent,
            _ => Self::Info,
        }
    }
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Trace => "TRACE",
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warning => "WARNING",
            Self::Error => "ERROR",
            Self::Silent => "SILENT",
        }
    }
}
#[derive(Clone, Debug)]
pub(crate) struct Record {
    pub(crate) id: u64,
    pub(crate) time: String,
    pub(crate) level: Level,
    pub(crate) source: Source,
    pub(crate) origin: String,
    pub(crate) message: String,
}
impl Record {
    pub(crate) fn program(level: Level, module: &str, message: &str) -> Self {
        Self {
            id: 0,
            time: chrono::Local::now().to_rfc3339(),
            level,
            source: Source::Program,
            origin: module.into(),
            message: super::redact(message),
        }
    }
    pub(crate) fn line(&self) -> String {
        format!(
            "{} {} [{}] {}\n",
            self.time,
            self.level.label(),
            self.origin,
            self.message.replace('\n', "\\n").replace('\r', "\\r")
        )
    }
    pub(crate) fn parts(&self) -> Vec<Self> {
        const BYTES: usize = 16 * 1024;
        if self.message.len() <= BYTES {
            return vec![self.clone()];
        }
        let mut chunks = Vec::new();
        let mut chunk = String::new();
        for ch in self.message.chars() {
            if chunk.len() + ch.len_utf8() > BYTES {
                chunks.push(std::mem::take(&mut chunk));
            }
            chunk.push(ch);
        }
        if !chunk.is_empty() {
            chunks.push(chunk);
        }
        let count = chunks.len();
        chunks
            .into_iter()
            .enumerate()
            .map(|(index, text)| Self {
                id: self.id,
                time: self.time.clone(),
                level: self.level,
                source: self.source,
                origin: self.origin.clone(),
                message: format!("[{}/{count}] {text}", index + 1),
            })
            .collect()
    }
}
#[derive(Default)]
struct State {
    next: u64,
    files: BTreeSet<String>,
    entries: VecDeque<Record>,
}
#[derive(Clone, Default)]
pub(crate) struct Feed(Arc<Mutex<State>>, Arc<Shared>);
impl Feed {
    pub(crate) fn attach(&self, queue: SyncSender<Message>) {
        let _ = self.1.queue.set(queue);
    }
    pub(crate) fn set_minimum(&self, level: Level) {
        self.1.minimum.store(level as u8, Ordering::Relaxed);
    }
    pub(crate) fn allows(&self, level: Level) -> bool {
        level as u8 >= self.1.minimum.load(Ordering::Relaxed)
    }
    /// Memory and file entry point for program logs; records below the selected level are discarded here.
    pub(crate) fn emit(&self, level: Level, module: &str, message: &str) {
        if !self.allows(level) {
            return;
        }
        let record = Record::program(level, module, message);
        self.publish(record.clone());
        let used = self.1.queued.load(Ordering::Relaxed);
        let accepted = used < QUEUE && (used * 4 <= QUEUE * 3 || level >= Level::Warning);
        if !accepted || !self.send(Message::Record(record)) {
            self.1.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
    /// Ask the log thread to re-read the program log policy.
    pub(crate) fn reload(&self) {
        self.1.reload.store(true, Ordering::Release);
        self.send(Message::Wake);
    }
    pub(crate) fn take_reload(&self) -> bool {
        self.1.reload.swap(false, Ordering::AcqRel)
    }
    pub(crate) fn take_dropped(&self) -> u64 {
        self.1.dropped.swap(0, Ordering::Relaxed)
    }
    pub(crate) fn received(&self) {
        self.1.queued.fetch_sub(1, Ordering::Relaxed);
    }
    /// The thread drains every earlier record before it stops.
    pub(crate) fn close(&self) {
        if let Some(queue) = self.1.queue.get() {
            self.1.queued.fetch_add(1, Ordering::Relaxed);
            if queue.send(Message::Stop).is_err() {
                self.1.queued.fetch_sub(1, Ordering::Relaxed);
            }
        }
    }
    fn send(&self, message: Message) -> bool {
        let Some(queue) = self.1.queue.get() else {
            return false;
        };
        self.1.queued.fetch_add(1, Ordering::Relaxed);
        match queue.try_send(message) {
            Ok(()) => true,
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
                self.1.queued.fetch_sub(1, Ordering::Relaxed);
                false
            }
        }
    }
    pub(crate) fn track(&self, sink: &super::Sink) {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .files
            .insert(sink.prefix.clone());
    }
    pub(crate) fn file_prefixes(&self) -> BTreeSet<String> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .files
            .clone()
    }
    pub(crate) fn publish(&self, record: Record) {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        for mut record in record.parts() {
            state.next += 1;
            record.id = state.next;
            state.entries.push_back(record);
            let source = state.entries.back().map(|record| record.source);
            if state
                .entries
                .iter()
                .filter(|record| Some(record.source) == source)
                .count()
                > CAPACITY / 2
                && let Some(index) = state
                    .entries
                    .iter()
                    .position(|record| Some(record.source) == source)
            {
                state.entries.remove(index);
            }
        }
    }
    pub(crate) fn since(&self, cursor: u64) -> Vec<Record> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entries
            .iter()
            .filter(|r| r.id > cursor)
            .cloned()
            .collect()
    }
}

/// Mihomo emits logfmt on its process pipes. Unstructured diagnostics remain intact.
pub(crate) fn core_record(line: &str, stderr: bool) -> Record {
    let fields = fields(line);
    let message = fields
        .iter()
        .find(|(key, _)| key == "msg")
        .map(|(_, value)| value.as_str())
        .unwrap_or(line.trim());
    let level = fields
        .iter()
        .find(|(key, _)| key == "level")
        .map(|(_, value)| Level::parse(value))
        .unwrap_or(if stderr { Level::Error } else { Level::Info });
    let time = fields
        .iter()
        .find(|(key, _)| key == "time")
        .map(|(_, value)| value.clone())
        .unwrap_or_else(|| chrono::Local::now().to_rfc3339());
    let origin = message
        .strip_prefix('[')
        .and_then(|rest| rest.split_once(']'))
        .map(|(name, _)| name)
        .filter(|name| !name.is_empty() && name.len() <= 64)
        .unwrap_or("mihomo");
    Record {
        id: 0,
        time,
        level,
        source: Source::Core,
        origin: origin.into(),
        message: message.into(),
    }
}
fn fields(line: &str) -> Vec<(String, String)> {
    let mut rest = line.trim();
    let mut result = Vec::new();
    while !rest.is_empty() {
        let Some((key, tail)) = rest.split_once('=') else {
            break;
        };
        if key.contains(char::is_whitespace) {
            break;
        }
        if tail.starts_with('"') {
            let mut escaped = false;
            let mut end = None;
            for (ix, ch) in tail.char_indices().skip(1) {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    end = Some(ix);
                    break;
                }
            }
            let Some(end) = end else { break };
            let Ok(value) = serde_json::from_str::<String>(&tail[..=end]) else {
                break;
            };
            result.push((key.into(), value));
            rest = tail[end + 1..].trim_start();
        } else {
            let end = tail.find(char::is_whitespace).unwrap_or(tail.len());
            result.push((key.into(), tail[..end].into()));
            rest = tail[end..].trim_start();
        }
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn core_logfmt_preserves_quoted_content_and_origin() {
        let entry = core_record(
            r#"time="2026-10-28T09:00:00+08:00" level=warning msg="[DNS] failed: \"offline\"""#,
            false,
        );
        assert_eq!(entry.level, Level::Warning);
        assert_eq!(entry.origin, "DNS");
        assert_eq!(entry.message, "[DNS] failed: \"offline\"");
        assert_eq!(
            core_record("unstructured fatal diagnostic", true).level,
            Level::Error
        );
    }
    #[test]
    fn concurrent_feed_is_bounded_and_cursor_is_stable() {
        let feed = Feed::default();
        for _ in 0..CAPACITY + 3 {
            feed.publish(Record::program(Level::Debug, "storage", "saved"));
        }
        let records = feed.since(0);
        assert_eq!(records.len(), CAPACITY / 2);
        assert_eq!(records[0].id, (CAPACITY / 2 + 4) as u64);
        assert!(feed.since(records.last().unwrap().id).is_empty());
    }
    #[test]
    fn full_queue_keeps_warnings_longer_and_reports_drops() {
        let feed = Feed::default();
        let (sender, _receiver) = std::sync::mpsc::sync_channel(QUEUE);
        feed.attach(sender);
        for _ in 0..QUEUE {
            feed.emit(Level::Info, "storage", "info");
        }
        assert_eq!(feed.take_dropped(), (QUEUE / 4 - 1) as u64);
        for _ in 0..QUEUE / 4 - 1 {
            feed.emit(Level::Error, "storage", "error");
        }
        assert_eq!(feed.take_dropped(), 0);
        feed.emit(Level::Error, "storage", "overflow");
        assert_eq!(feed.take_dropped(), 1);
    }
    #[test]
    fn minimum_level_discards_before_memory() {
        let feed = Feed::default();
        feed.set_minimum(Level::Warning);
        feed.emit(Level::Info, "core", "hidden");
        feed.emit(Level::Warning, "core", "shown");
        assert_eq!(feed.since(0).len(), 1);
    }
}
