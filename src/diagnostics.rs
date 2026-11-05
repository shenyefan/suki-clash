//! Explicit diagnostic export, including redacted logs from this application run.
use crate::{
    preferences::{self, SettingsKind},
    storage::{AppPaths, DirectoryKind, Repository},
};
use std::{
    io::{Read as _, Write as _},
    path::PathBuf,
};

pub(crate) fn export(
    repository: &Repository,
    logs: &crate::logging::Feed,
) -> Result<PathBuf, String> {
    let paths = repository.paths();
    let mut failures = Vec::new();
    // Check the native files independently: a damaged settings.json must not mask YAML errors.
    for name in ["mihomo.yaml", "dns.yaml", "sniffer.yaml"] {
        let path = paths.settings().join(name);
        match std::fs::read(&path) {
            Ok(bytes) => {
                if let Err(error) = serde_yaml_ng::from_slice::<serde_yaml_ng::Mapping>(&bytes) {
                    let location = error
                        .location()
                        .map(|location| {
                            format!("，第 {} 行第 {} 列", location.line(), location.column())
                        })
                        .unwrap_or_default();
                    failures.push(format!("{name}：YAML 格式无效{location}"));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => failures.push(format!("{name}：{:?}", error.kind())),
        }
    }
    if let Err(error) = crate::configuration::Preferences::load(paths) {
        failures.push(error);
    }
    for kind in SettingsKind::ALL
        .into_iter()
        .filter(|kind| !matches!(kind, SettingsKind::Core | SettingsKind::Resources))
    {
        match preferences::values(repository, kind) {
            Ok(values) => {
                if let Err(error) = preferences::validate(kind, &values) {
                    failures.push(format!("{}：{error}", kind.id()));
                }
            }
            Err(error) => failures.push(error.to_string()),
        }
    }
    if let Err(error) = crate::configuration::selected_profile(paths) {
        failures.push(error);
    }
    if let Err(error) = crate::proxies::chain_config::Config::load(paths) {
        failures.push(error);
    }
    let environment = format!(
        "Suki Clash {}\nOS: {}\nArchitecture: {}\nTime: {}\nCore installed: {}\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        chrono::Utc::now().to_rfc3339(),
        crate::bootstrap::require_core(paths).is_ok()
    );
    // File metadata is sufficient to locate broken inputs without exporting subscription secrets.
    let mut files = String::new();
    for name in ["settings.json", "mihomo.yaml", "dns.yaml", "sniffer.yaml"] {
        let path = paths.settings().join(name);
        let status = match std::fs::symlink_metadata(&path) {
            Ok(meta) => {
                if meta.is_file() {
                    format!("{} bytes", meta.len())
                } else {
                    "not a regular file".into()
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => "missing".into(),
            Err(error) => format!("{:?}", error.kind()),
        };
        files.push_str(&format!("{name}: {status}\n"));
    }
    let report = if failures.is_empty() {
        "配置检查通过".into()
    } else {
        redact(&crate::failure::join(&failures), paths)
    };
    let directory = paths.directory(DirectoryKind::Backups);
    let mut temporary = tempfile::NamedTempFile::new_in(&directory)
        .map_err(|error| crate::logging::diagnostic_error("无法创建诊断文件", error))?;
    {
        let mut zip = zip::ZipWriter::new(temporary.as_file_mut());
        for (name, content) in [
            ("environment.txt", environment),
            ("settings-check.txt", report),
            ("files.txt", files),
        ] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .map_err(|error| crate::logging::diagnostic_error("无法写入诊断包", error))?;
            zip.write_all(content.as_bytes())
                .map_err(|error| crate::logging::diagnostic_error("无法写入诊断信息", error))?;
        }
        export_logs(&mut zip, paths, logs)?;
        zip.finish()
            .map_err(|error| crate::logging::diagnostic_error("无法完成诊断包", error))?;
    }
    temporary
        .as_file_mut()
        .sync_all()
        .map_err(|error| crate::logging::diagnostic_error("无法保存诊断包", error))?;
    let path = directory.join(format!(
        "suki-diagnostics-{}.zip",
        chrono::Utc::now().format("%Y%m%dT%H%M%S%.f")
    ));
    temporary
        .persist_noclobber(&path)
        .map_err(|error| crate::logging::diagnostic_error("无法提交诊断包", error))?;
    Ok(path)
}
fn export_logs(
    zip: &mut zip::ZipWriter<&mut std::fs::File>,
    paths: &AppPaths,
    logs: &crate::logging::Feed,
) -> Result<(), String> {
    // Only registered writers belong to this run, including earlier core
    // generations and rotated parts. Never collect older runs by modification time.
    const LIMIT: usize = 64 * 1024 * 1024;
    let prefixes = logs.file_prefixes();
    let mut status = vec![format!(
        "本次程序运行 PID：{}。仅导出本次运行登记的日志文件。\n磁盘日志总上限 64 MiB；内存尾部每种来源最多 1000 条，可能与文件重叠。\n配置正文不导出；日志中的凭据、订阅 URL 和应用目录已脱敏。",
        std::process::id()
    )];
    let mut write = |name: &str, text: &str| -> Result<(), String> {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .map_err(|error| crate::logging::diagnostic_error("无法写入诊断日志", error))?;
        zip.write_all(redact(&crate::logging::redact(text), paths).as_bytes())
            .map_err(|error| crate::logging::diagnostic_error("无法写入诊断日志".to_owned(), error))
    };
    let mut program = String::new();
    let mut core = String::new();
    for record in logs.since(0) {
        match record.source {
            crate::logging::Source::Program => program.push_str(&record.line()),
            crate::logging::Source::Core => core.push_str(&record.line()),
        }
    }
    write("logs/program-current.log", &program)?;
    write("logs/core-current.log", &core)?;
    let directory = paths.directory(DirectoryKind::Logs);
    let entries = match std::fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) => {
            status.push(format!("日志目录不可读：{}；已保留内存日志", error.kind()));
            return write("logs/export-status.txt", &status.join("\n"));
        }
    };
    let mut files = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => files.push(entry),
            Err(error) => status.push(format!("日志目录条目未导出：{error}")),
        }
    }
    files.sort_by_key(|entry| entry.file_name());
    let mut bytes = 0;
    for entry in files {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".log")
            || !prefixes.iter().any(|prefix| {
                name.strip_prefix(prefix)
                    .is_some_and(|tail| tail.starts_with('-'))
            })
        {
            continue;
        }
        if bytes == LIMIT {
            status.push("磁盘日志达到 64 MiB 上限，剩余内容未导出".into());
            break;
        }
        let read = (|| -> std::io::Result<Vec<u8>> {
            let metadata = std::fs::symlink_metadata(entry.path())?;
            if !metadata.file_type().is_file() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "日志不是普通文件",
                ));
            }
            let mut content = Vec::new();
            std::fs::File::open(entry.path())?
                .take((LIMIT - bytes) as u64)
                .read_to_end(&mut content)?;
            if metadata.len() > content.len() as u64 {
                status.push(format!("{name} 达到导出大小上限，后续内容未导出"));
            }
            Ok(content)
        })();
        match read {
            Ok(content) => {
                bytes += content.len();
                write(&format!("logs/{name}"), &String::from_utf8_lossy(&content))?;
            }
            Err(error) => status.push(format!("{name} 未导出：{error}")),
        }
    }
    status.push(format!(
        "本次登记 {} 个日志写入批次，已读取 {bytes} 字节磁盘日志",
        prefixes.len()
    ));
    write("logs/export-status.txt", &status.join("\n"))
}
fn redact(message: &str, paths: &AppPaths) -> String {
    let mut text = message.to_owned();
    for directory in [
        DirectoryKind::Config,
        DirectoryKind::Data,
        DirectoryKind::Cache,
    ] {
        text = text.replace(
            paths.directory(directory).to_string_lossy().as_ref(),
            "<app>",
        );
    }
    // Validation reasons can contain user supplied endpoints. Keep the reason, omit values.
    for pattern in [r"(?i)https?://[^\s；，]+", r"AGE-SECRET-KEY-[A-Za-z0-9]+"] {
        if let Ok(regex) = regex::Regex::new(pattern) {
            text = regex.replace_all(&text, "<redacted>").into_owned();
        }
    }
    text
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn export_checks_all_broken_inputs_without_including_sources_or_secrets() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        let repository = Repository::open(paths.clone()).unwrap();
        std::fs::write(paths.settings().join("mihomo.yaml"), b"port: [").unwrap();
        std::fs::write(paths.settings().join("dns.yaml"), b"dns: [").unwrap();
        std::fs::write(paths.settings().join("settings.json"), b"{private-token").unwrap();
        let path = export(&repository, &crate::logging::Feed::default()).unwrap();
        let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
        use std::io::Read as _;
        let mut check = String::new();
        zip.by_name("settings-check.txt")
            .unwrap()
            .read_to_string(&mut check)
            .unwrap();
        assert!(check.contains("mihomo.yaml"));
        assert!(check.contains("dns.yaml"));
        assert!(check.contains("settings.json"));
        assert!(!check.contains("private-token"));
        assert!(!check.contains(root.path().to_str().unwrap()));
        assert_eq!(
            std::fs::read(paths.settings().join("settings.json")).unwrap(),
            b"{private-token"
        );
    }
    #[test]
    fn export_includes_this_runs_rotated_logs_and_memory_tail_with_secrets_redacted() {
        use crate::logging::{Feed, Level, Record, Sink, Source};
        use std::io::Read as _;
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        let repository = Repository::open(paths.clone()).unwrap();
        let mut previous = Sink::new(&paths, Source::Program, 7, 1024).unwrap();
        previous.write(b"OUTSIDE_CURRENT_RUN\n").unwrap();
        let feed = Feed::default();
        let mut program = Sink::new(&paths, Source::Program, 7, 1024).unwrap();
        feed.track(&program);
        program
            .write(
                b"phase ready token=private-value https://example.test/subscription/private-path\n",
            )
            .unwrap();
        let mut core = Sink::new(&paths, Source::Core, 7, 1024).unwrap();
        feed.track(&core);
        for part in 0..3 {
            core.write(format!("core-generation-{part} {}\n", "x".repeat(1000)).as_bytes())
                .unwrap();
        }
        feed.publish(Record::program(
            Level::Error,
            "storage",
            "memory-fallback password=secret-value",
        ));
        feed.publish(crate::logging::core_record(
            "level=warning msg=\"live core tail\"",
            false,
        ));
        let path = export(&repository, &feed).unwrap();
        let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
        let mut contents = String::new();
        let mut disk_parts = 0;
        for index in 0..zip.len() {
            let mut file = zip.by_index(index).unwrap();
            let name = file.name().unwrap();
            if name.starts_with("logs/core-") && name != "logs/core-current.log" {
                disk_parts += 1;
            }
            file.read_to_string(&mut contents).unwrap();
        }
        assert_eq!(disk_parts, 3);
        for expected in [
            "phase ready",
            "core-generation-0",
            "core-generation-2",
            "memory-fallback",
            "live core tail",
        ] {
            assert!(contents.contains(expected), "{expected}");
        }
        for secret in [
            "private-value",
            "private-path",
            "secret-value",
            "OUTSIDE_CURRENT_RUN",
        ] {
            assert!(!contents.contains(secret), "{secret}");
        }
    }
}
