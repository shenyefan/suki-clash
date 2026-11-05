//! Drain both child pipes even when file persistence fails; publish redacted complete lines.
use crate::{
    logging::{Feed, Level, Record, Sink, Source},
    storage::AppPaths,
};
use tokio::{
    io::AsyncReadExt as _,
    process::{ChildStderr, ChildStdout},
    task::JoinHandle,
};
pub(super) type LogSettings = (Vec<String>, (u32, u64));
pub(super) fn capture(
    paths: AppPaths,
    feed: Feed,
    stdout: ChildStdout,
    stderr: ChildStderr,
    redactions: Vec<String>,
    limits: (u32, u64),
    suppressed: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> (
    JoinHandle<Result<(), String>>,
    tokio::sync::watch::Sender<LogSettings>,
) {
    let (settings, mut updates) = tokio::sync::watch::channel((redactions, limits));
    let task = tokio::spawn(async move {
        let mut output = Output::new(feed, suppressed);
        match Sink::new(&paths, Source::Core, limits.0, limits.1) {
            Ok(sink) => {
                output.feed.track(&sink);
                output.sink = Some(sink);
            }
            Err(error) => output.fail(format!("无法创建内核日志文件：{error}")),
        }
        let (mut stdout, mut stderr) = (stdout, stderr);
        let (mut out, mut err) = (Lines::default(), Lines::default());
        let (mut out_buffer, mut err_buffer) = ([0u8; 4096], [0u8; 4096]);
        let (mut out_done, mut err_done) = (false, false);
        let mut settings_open = true;
        while !out_done || !err_done {
            tokio::select! {
                changed=updates.changed(), if settings_open=>{
                    settings_open = changed.is_ok();
                    if settings_open {
                        let (_, limits)=updates.borrow_and_update().clone();
                        if let Some(sink)=&mut output.sink && let Err(error)=sink.configure(limits.0,limits.1) {
                            output.fail(format!("无法应用内核日志保留设置：{error}"));
                        }
                    }
                },
                result=stdout.read(&mut out_buffer),if !out_done=>{
                    let count=result.map_err(|error| crate::logging::diagnostic_error("读取内核标准输出失败", error))?; out_done=count==0;
                    out.append(&mut output,&out_buffer[..count],out_done,&updates.borrow().0,false);
                },
                result=stderr.read(&mut err_buffer),if !err_done=>{
                    let count=result.map_err(|error| crate::logging::diagnostic_error("读取内核错误输出失败", error))?; err_done=count==0;
                    err.append(&mut output,&err_buffer[..count],err_done,&updates.borrow().0,true);
                },
            }
        }
        output.failure.map_or(Ok(()), Err)
    });
    (task, settings)
}
pub(super) struct Output {
    sink: Option<Sink>,
    feed: Feed,
    failure: Option<String>,
    suppressed: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl Output {
    pub(super) fn new(
        feed: Feed,
        suppressed: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        Self {
            sink: None,
            feed,
            failure: None,
            suppressed,
        }
    }
    pub(super) fn configure(&mut self, paths: &AppPaths, limits: (u32, u64)) {
        if self.sink.is_none() {
            match Sink::new(paths, Source::Core, limits.0, limits.1) {
                Ok(sink) => {
                    self.feed.track(&sink);
                    self.sink = Some(sink);
                }
                Err(error) => self.fail(format!("无法创建内核日志文件：{error}")),
            }
        } else if let Some(sink) = &mut self.sink
            && let Err(error) = sink.configure(limits.0, limits.1)
        {
            self.fail(format!("无法设置内核日志策略：{error}"));
        }
    }

    fn fail(&mut self, error: String) {
        if self.failure.is_none() {
            self.feed
                .publish(Record::program(Level::Error, "logging", &error));
            self.failure = Some(error);
        }
    }
    pub(super) fn write(&mut self, bytes: &[u8], secrets: &[String], stderr: bool) {
        if self.suppressed.load(std::sync::atomic::Ordering::Acquire) {
            return;
        }
        let mut line = String::from_utf8_lossy(bytes).into_owned();
        for secret in secrets.iter().filter(|secret| !secret.is_empty()) {
            line = line.replace(secret, "[REDACTED]");
        }
        if line.trim().is_empty() {
            return;
        }
        self.feed
            .publish(crate::logging::core_record(&line, stderr));
        if !line.ends_with('\n') {
            line.push('\n');
        }
        if let Some(sink) = &mut self.sink
            && let Err(error) = sink.write(line.as_bytes())
        {
            self.fail(format!("内核日志保存失败：{error}"));
        }
    }
}
#[derive(Default)]
struct Lines {
    pending: Vec<u8>,
    discarding: bool,
}
impl Lines {
    fn append(
        &mut self,
        output: &mut Output,
        bytes: &[u8],
        eof: bool,
        secrets: &[String],
        stderr: bool,
    ) {
        // A discarded line is skipped through its newline, so fragments cannot leak secrets.
        for byte in bytes {
            if *byte == b'\n' {
                if !self.discarding {
                    output.write(&std::mem::take(&mut self.pending), secrets, stderr);
                }
                self.discarding = false;
            } else if !self.discarding {
                self.pending.push(*byte);
                if self.pending.len() > 64 * 1024 {
                    self.pending.clear();
                    self.discarding = true;
                    output.write(
                        b"level=warning msg=\"Oversized core log line discarded\"",
                        &[],
                        false,
                    );
                }
            }
        }
        if eof && !self.discarding && !self.pending.is_empty() {
            output.write(&std::mem::take(&mut self.pending), secrets, stderr);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[tokio::test]
    async fn capture_drains_real_pipes_after_settings_sender_is_closed() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        let mut command = tokio::process::Command::new("/bin/sh");
        command
            .arg("-c")
            .arg("printf 'level=info msg=\"[TCP] stdout\"\\n'; printf 'last diagnostic\\n' >&2")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = command.spawn().unwrap();
        let feed = Feed::default();
        let (task, settings) = capture(
            paths,
            feed.clone(),
            child.stdout.take().unwrap(),
            child.stderr.take().unwrap(),
            vec![],
            (7, 1024),
            Default::default(),
        );
        drop(settings);
        child.wait().await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let records = feed.since(0);
        assert_eq!(records.len(), 2);
        assert!(records.iter().any(|record| record.origin == "TCP"));
        assert!(
            records
                .iter()
                .any(|record| record.level == Level::Error && record.message == "last diagnostic")
        );
    }
    #[test]
    fn split_lines_are_redacted_once_in_both_live_and_saved_output() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        let feed = Feed::default();
        let mut output = Output {
            sink: Some(Sink::new(&paths, Source::Core, 7, 1024 * 1024).unwrap()),
            feed: feed.clone(),
            failure: None,
            suppressed: Default::default(),
        };
        let mut lines = Lines::default();
        let secrets = vec!["secret-token".into()];
        lines.append(
            &mut output,
            b"level=info msg=\"[TCP] secret-",
            false,
            &secrets,
            false,
        );
        assert!(feed.since(0).is_empty());
        lines.append(
            &mut output,
            b"token\"\nlevel=error msg=\"failed\"",
            true,
            &secrets,
            false,
        );
        let records = feed.since(0);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].origin, "TCP");
        assert!(!records[0].message.contains("secret-token"));
        assert_eq!(records[1].level, Level::Error);
        let saved = std::fs::read_dir(paths.directory(crate::storage::DirectoryKind::Logs))
            .unwrap()
            .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
            .collect::<String>();
        assert!(!saved.contains("secret-token"));
        assert!(saved.contains("[REDACTED]"));
    }
    #[test]
    fn oversized_line_drops_the_entire_tail_and_next_line_recovers() {
        let feed = Feed::default();
        let mut output = Output {
            sink: None,
            feed: feed.clone(),
            failure: None,
            suppressed: Default::default(),
        };
        let mut lines = Lines::default();
        lines.append(&mut output, &vec![b'x'; 64 * 1024 + 1], false, &[], false);
        lines.append(
            &mut output,
            b"sensitive-tail\nlevel=info msg=\"next\"\n",
            false,
            &[],
            false,
        );
        let records = feed.since(0);
        assert_eq!(records.len(), 2);
        assert_eq!(records[1].message, "next");
        assert!(
            !records
                .iter()
                .any(|record| record.message.contains("sensitive-tail"))
        );
    }
}
