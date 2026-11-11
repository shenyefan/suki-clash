use std::{collections::VecDeque, time::Duration};

use anyhow::Result;
use flexi_logger::{
    Cleanup, Criterion, DeferredNow, FileSpec, Naming, WriteMode,
    writers::{
        ArcFileLogWriter, FileLogWriter, FileLogWriterBuilder, FileLogWriterHandle, LogWriter,
    },
};
use log::{Level, Record};
use once_cell::sync::{Lazy, OnceCell};
use parking_lot::{Mutex, RwLock};
use tracing_log::NormalizeEvent as _;
use tracing_subscriber::{
    fmt::{FmtContext, FormatEvent, FormatFields, format::Writer, writer::MakeWriterExt as _},
    registry::LookupSpan,
};

use crate::core::paths::{ensure_persistent_state_layout, service_paths};
use crate::core::platform_security;
use crate::core::structure::WriterConfig;

// Buffered writes avoid flexi's one-syscall-per-line default; the background
// flusher bounds the hard-kill loss window.
const WRITE_BUFFER_CAPACITY: usize = 64 * 1024;
const WRITE_FLUSH_INTERVAL: Duration = Duration::from_millis(500);

const SERVICE_LOG_MAX_SIZE: u64 = 10 * 1024 * 1024;
const SERVICE_LOG_MAX_FILES: usize = 3;

static GLOBAL_WRITER: RwLock<Option<FileLogWriter>> = RwLock::new(None);
static SERVICE_LOG: OnceCell<(ArcFileLogWriter, FileLogWriterHandle)> = OnceCell::new();

/// Logs to stdout and to the service log file, since launchd and the Windows SCM discard stdout.
pub fn init_service_logging() {
    let (file, file_error) = match service_log_writer()
        .and_then(|writer| writer.try_build_with_handle().map_err(Into::into))
    {
        Ok((writer, handle)) => (Some((writer, handle)), None),
        Err(error) => (None, Some(error)),
    };
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .event_format(ServiceFormat)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        );
    let installed = if let Some((writer, handle)) = file {
        let output = writer.clone();
        let result = subscriber
            .with_writer(std::io::stdout.and(move || output.clone()))
            .try_init();
        if result.is_ok() {
            let _ = SERVICE_LOG.set((writer, handle));
        }
        result
    } else {
        subscriber.with_writer(std::io::stdout).try_init()
    };
    if let Err(error) = installed {
        eprintln!("无法初始化服务日志：{error:#}");
    }
    if let Some(error) = file_error {
        tracing::warn!("服务日志文件不可用，仅输出到标准输出：{error:#}");
    }
}

struct ServiceFormat;

impl<S, N> FormatEvent<S, N> for ServiceFormat
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        context: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> std::fmt::Result {
        let normalized = event.normalized_metadata();
        let metadata = normalized.as_ref().unwrap_or_else(|| event.metadata());
        let origin = match metadata.target() {
            "suki_clash_service" => "service",
            target => target
                .strip_prefix("suki_clash_service::")
                .unwrap_or(target),
        };
        let level = match *metadata.level() {
            tracing::Level::WARN => "WARNING",
            _ => metadata.level().as_str(),
        };
        let mut message = String::new();
        context
            .field_format()
            .format_fields(Writer::new(&mut message), event)?;
        writeln!(
            writer,
            "{} {level} [{origin}] {}",
            DeferredNow::new().now().to_rfc3339(),
            message.replace('\n', "\\n").replace('\r', "\\r")
        )
    }
}

/// Flushes buffered service log lines before the process exits.
pub fn flush_service_log() {
    if let Some((writer, _handle)) = SERVICE_LOG.get() {
        let _ = LogWriter::flush(&**writer);
    }
}

fn service_log_writer() -> anyhow::Result<FileLogWriterBuilder> {
    ensure_persistent_state_layout()?;
    let directory = service_paths()?.logs_dir();
    platform_security::ensure_private_service_directory(&directory)?;
    Ok(FileLogWriter::builder(
        FileSpec::default()
            .directory(directory)
            .basename(crate::SERVICE_SLUG)
            .suppress_timestamp(),
    )
    .write_mode(WriteMode::BufferAndFlushWith(
        WRITE_BUFFER_CAPACITY,
        WRITE_FLUSH_INTERVAL,
    ))
    // A restart must not rotate the previous run away.
    .append()
    .rotate(
        Criterion::Size(SERVICE_LOG_MAX_SIZE),
        Naming::TimestampsCustomFormat {
            current_infix: Some("latest"),
            format: "%Y-%m-%d_%H-%M-%S",
        },
        Cleanup::KeepLogFiles(SERVICE_LOG_MAX_FILES),
    ))
}

fn service_writer_builder(config: &WriterConfig) -> FileLogWriterBuilder {
    FileLogWriter::builder(
        FileSpec::default()
            .directory(config.directory.clone())
            .basename("service")
            .suppress_timestamp(),
    )
    .format(|writer, now, record| {
        write!(
            writer,
            "[{}] {}",
            now.format("%Y-%m-%d %H:%M:%S%.3f"),
            record.args()
        )
    })
    .write_mode(WriteMode::BufferAndFlushWith(
        WRITE_BUFFER_CAPACITY,
        WRITE_FLUSH_INTERVAL,
    ))
    .rotate(
        Criterion::Size(config.max_log_size),
        Naming::TimestampsCustomFormat {
            current_infix: Some("latest"),
            format: "%Y-%m-%d_%H-%M-%S",
        },
        Cleanup::KeepLogFiles(config.max_log_files),
    )
}

/// Resets the writer in place instead of rebuilding it: flexi's flusher thread
/// cannot be stopped, so every dropped writer would leak a thread and an fd.
pub fn set_or_update_writer(config: &WriterConfig) -> Result<()> {
    let builder = service_writer_builder(config);
    let mut slot = GLOBAL_WRITER.write();
    match slot.as_ref() {
        Some(writer) => {
            // Propagate the drain failure so the buffered tail stays retryable.
            writer.flush()?;
            writer.reset(&builder)?;
        }
        None => *slot = Some(builder.try_build()?),
    }
    Ok(())
}

pub fn write_core_line(level: Level, line: &str) {
    let guard = GLOBAL_WRITER.read();
    if let Some(writer) = guard.as_ref() {
        let mut now = DeferredNow::default();
        let args = format_args!("{line}");
        let record = Record::builder()
            .args(args)
            .level(level)
            .target("service")
            .build();
        let _ = writer.write(&mut now, &record);
    }
}

pub fn flush_writer() {
    let guard = GLOBAL_WRITER.read();
    if let Some(writer) = guard.as_ref() {
        let _ = writer.flush();
    }
}

const LOGS_QUEUE_LEN: usize = 100;

pub struct LogRing {
    started_at: std::time::SystemTime,
    inner: Mutex<(u64, VecDeque<crate::CoreLogEntry>)>,
}

impl LogRing {
    fn new() -> Self {
        Self {
            started_at: std::time::SystemTime::now(),
            inner: Mutex::new((0, VecDeque::with_capacity(LOGS_QUEUE_LEN))),
        }
    }

    pub fn append_log(&self, log: String, stderr: bool) {
        let mut guard = self.inner.lock();
        if guard.1.len() >= LOGS_QUEUE_LEN {
            guard.1.pop_front();
        }
        guard.0 = guard.0.saturating_add(1);
        let sequence = guard.0;
        let line = if log.len() > 64 * 1024 {
            "level=warning msg=\"Oversized core log line discarded\"".into()
        } else {
            log
        };
        guard.1.push_back(crate::CoreLogEntry {
            sequence,
            stderr,
            line,
        });
    }

    pub fn get_logs(
        &self,
        generation: u64,
        after: u64,
        stream_started_at: Option<std::time::SystemTime>,
    ) -> crate::CoreLogBatch {
        let guard = self.inner.lock();
        let after = if stream_started_at == Some(self.started_at) {
            after
        } else {
            0
        };
        crate::CoreLogBatch {
            stream_started_at: self.started_at,
            generation,
            oldest: guard
                .1
                .front()
                .map(|entry| entry.sequence)
                .unwrap_or(guard.0.saturating_add(1)),
            next: guard.0,
            entries: guard
                .1
                .iter()
                .filter(|entry| entry.sequence > after)
                .cloned()
                .collect(),
        }
    }

    pub fn clear_logs(&self) {
        // Keep sequence monotonic while the helper lives; old cursors never duplicate new lines.
        self.inner.lock().1.clear();
    }
}

pub static LOG_RING: Lazy<LogRing> = Lazy::new(LogRing::new);

#[cfg(test)]
mod ring_tests {
    use super::*;
    #[test]
    fn log_cursor_keeps_repeated_messages_bounded_and_reports_history_gaps() {
        let ring = LogRing::new();
        ring.append_log("same".into(), false);
        ring.append_log("same".into(), false);
        let first = ring.get_logs(7, 0, Some(ring.started_at));
        assert_eq!(first.entries.len(), 2);
        assert!(
            ring.get_logs(7, first.next, Some(ring.started_at))
                .entries
                .is_empty()
        );
        for _ in 0..101 {
            ring.append_log("next".into(), false);
        }
        let latest = ring.get_logs(7, first.next, Some(ring.started_at));
        assert_eq!(latest.entries.len(), 100);
        assert!(latest.oldest > first.next + 1);
        ring.clear_logs();
        ring.append_log("after restart".into(), false);
        assert!(
            ring.get_logs(8, latest.next, Some(ring.started_at)).entries[0].sequence > latest.next
        );
        ring.append_log("x".repeat(64 * 1024 + 1), false);
        assert!(
            ring.get_logs(8, 0, Some(ring.started_at))
                .entries
                .last()
                .unwrap()
                .line
                .contains("discarded")
        );
        let restarted = LogRing::new();
        restarted.append_log("new helper startup".into(), true);
        let renewed = restarted.get_logs(8, latest.next, Some(ring.started_at));
        assert_eq!(renewed.entries.len(), 1);
        assert!(renewed.entries[0].stderr);
    }

    #[test]
    fn service_file_and_console_use_program_format_and_preserve_original_causes() {
        use std::io::Write as _;
        use std::sync::{Arc, Mutex};

        #[derive(Clone)]
        struct Console(Arc<Mutex<Vec<u8>>>);
        impl std::io::Write for Console {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let root = tempfile::tempdir().unwrap();
        let (mut file, _handle) = FileLogWriter::builder(
            FileSpec::default()
                .directory(root.path())
                .basename("service")
                .suppress_timestamp(),
        )
        .try_build_with_handle()
        .unwrap();
        let console = Console(Default::default());
        let captured = console.0.clone();
        let output = file.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .event_format(ServiceFormat)
            .with_max_level(tracing::Level::TRACE)
            .with_writer((move || console.clone()).and(move || output.clone()))
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(target: "suki_clash_service::core::server", "内核已启动");
            tracing::warn!(target: "suki_clash_service::core::manager", attempts = 3, "准备重试");
            tracing::error!(target: "suki_clash_service::core::ipc", "Failed to write response: 管道已结束。 (os error 109)");
            tracing_log::format_trace(
                &log::Record::builder()
                    .level(log::Level::Warn)
                    .target("suki_clash_service::execution")
                    .args(format_args!("第一行\n第二行\r结束"))
                    .build(),
            )
            .unwrap();
        });
        file.flush().unwrap();
        let console = String::from_utf8(captured.lock().unwrap().clone()).unwrap();
        let saved = std::fs::read_dir(root.path())
            .unwrap()
            .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
            .collect::<String>();
        assert_eq!(saved, console);
        let lines = saved.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 4);
        assert!(lines[0].contains(" INFO [core::server] 内核已启动"));
        assert!(lines[1].contains(" WARNING [core::manager] 准备重试 attempts=3"));
        assert!(
            lines[2].contains(
                " ERROR [core::ipc] Failed to write response: 管道已结束。 (os error 109)"
            )
        );
        assert!(lines[3].contains(" WARNING [execution] 第一行\\n第二行\\r结束"));
        assert!(!saved.contains('\u{1b}'));
        assert!(lines[0].split_once(' ').unwrap().0.contains('T'));
    }
}
