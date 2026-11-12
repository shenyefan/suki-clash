//! Explicit diagnostic export, including redacted logs from this application run.
mod configuration;
mod environment;
use crate::storage::{AppPaths, DirectoryKind, Repository};
use std::{
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
};

pub(crate) fn export(
    repository: &Repository,
    logs: &crate::logging::Feed,
    target: &Path,
    service_log: Result<&str, &str>,
) -> Result<PathBuf, String> {
    let paths = repository.paths();
    let directory = target.parent().ok_or("诊断包保存位置无效")?;
    let mut temporary = tempfile::NamedTempFile::new_in(directory)
        .map_err(|error| crate::logging::diagnostic_error("无法创建诊断文件", error))?;
    {
        let mut zip = zip::ZipWriter::new(temporary.as_file_mut());
        for warning in configuration::export(&mut zip, paths)? {
            logs.emit(crate::logging::Level::Warning, "diagnostics", &warning);
        }
        match service_log {
            Ok(text) => {
                zip.start_file(
                    "logs/service-current.log",
                    zip::write::SimpleFileOptions::default(),
                )
                .map_err(|error| crate::logging::diagnostic_error("无法写入服务日志", error))?;
                zip.write_all(redact(text, paths).as_bytes())
                    .map_err(|error| crate::logging::diagnostic_error("无法写入服务日志", error))?;
            }
            Err(error) => logs.emit(
                crate::logging::Level::Warning,
                "diagnostics",
                &format!("服务日志未导出：{error}"),
            ),
        }
        for warning in export_logs(&mut zip, paths, logs)? {
            logs.emit(crate::logging::Level::Warning, "diagnostics", &warning);
        }
        let report = environment::collect(paths);
        let (environment, _) = configuration::sanitize(
            "environment.json",
            &serde_json::to_vec(&report)
                .map_err(|error| crate::logging::diagnostic_error("无法编码诊断环境", error))?,
            paths,
        )?;
        zip.start_file("environment.txt", zip::write::SimpleFileOptions::default())
            .map_err(|error| crate::logging::diagnostic_error("无法写入诊断环境", error))?;
        zip.write_all(environment.as_bytes())
            .map_err(|error| crate::logging::diagnostic_error("无法写入诊断环境", error))?;
        zip.finish()
            .map_err(|error| crate::logging::diagnostic_error("无法完成诊断包", error))?;
    }
    temporary
        .as_file_mut()
        .sync_all()
        .map_err(|error| crate::logging::diagnostic_error("无法保存诊断包", error))?;
    temporary
        .persist(target)
        .map_err(|error| crate::logging::diagnostic_error("无法提交诊断包", error))?;
    Ok(target.to_path_buf())
}
fn export_logs(
    zip: &mut zip::ZipWriter<&mut std::fs::File>,
    paths: &AppPaths,
    logs: &crate::logging::Feed,
) -> Result<Vec<String>, String> {
    // Only registered writers belong to this run, including earlier core
    // generations and rotated parts. Never collect older runs by modification time.
    const LIMIT: usize = 64 * 1024 * 1024;
    let prefixes = logs.file_prefixes();
    let mut status = Vec::new();
    let mut write = |name: &str, text: &str| -> Result<(), String> {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .map_err(|error| crate::logging::diagnostic_error("无法写入诊断日志", error))?;
        zip.write_all(redact(text, paths).as_bytes())
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
            return Ok(status);
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
    Ok(status)
}
fn redact(message: &str, paths: &AppPaths) -> String {
    let mut text = crate::logging::redact(message);
    for directory in [
        DirectoryKind::Config,
        DirectoryKind::Data,
        DirectoryKind::Cache,
        DirectoryKind::Runtime,
        DirectoryKind::Logs,
    ] {
        text = text.replace(
            paths.directory(directory).to_string_lossy().as_ref(),
            "<app>",
        );
    }
    if let Some(home) = dirs::home_dir() {
        text = text.replace(home.to_string_lossy().as_ref(), "<home>");
    }
    text
}
#[cfg(test)]
mod tests {
    #[test]
    fn export_uses_the_chosen_destination_and_reports_missing_service_logs() {
        use std::io::Read as _;
        let root = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        let paths = super::AppPaths::isolated(root.path());
        let repository = super::Repository::open(paths.clone()).unwrap();
        let target = destination.path().join("chosen.zip");
        let output = super::export(
            &repository,
            &crate::logging::Feed::default(),
            &target,
            Err("service unavailable"),
        )
        .unwrap();
        assert_eq!(output, target);
        let mut zip = zip::ZipArchive::new(std::fs::File::open(output).unwrap()).unwrap();
        assert!(zip.by_name("logs/service-current.log").is_err());
        let mut status = String::new();
        zip.by_name("logs/program-current.log")
            .unwrap()
            .read_to_string(&mut status)
            .unwrap();
        assert!(status.contains("服务日志未导出：service unavailable"));
        assert!(!paths.data().join("backups").join("chosen.zip").exists());
    }
    use super::*;
    fn export(repository: &Repository, logs: &crate::logging::Feed) -> Result<PathBuf, String> {
        let target = repository.paths().data().join("chosen-diagnostics.zip");
        super::export(
            repository,
            logs,
            &target,
            Ok("service diagnostic tail token=service-private\n"),
        )
    }
    #[test]
    fn export_includes_actual_settings_and_configuration_with_nested_secrets_removed() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        let repository = Repository::open(paths.clone()).unwrap();
        let settings = serde_json::json!({"schema_version":1,
            "general":{"theme_index":"dark","auto_run":true,"download_directory":paths.data().join("downloads")},
            "sysproxy":{"mode":"manual","proxy_guard":true},
            "dns":{"dns_override_enabled":true},
            "backup":{"encrypt":true,"recipient":"private-recipient"},
            "profiles":{"selected_profile_id":1,"items":[{"id":1,"name":"private subscription","url":"https://subscription.test/private-path?token=url-private"}]},
            "core":{"enable_external_controller":true}
        });
        std::fs::write(
            paths.settings().join("settings.json"),
            serde_json::to_vec(&settings).unwrap(),
        )
        .unwrap();
        let mihomo = "mixed-port: 7890\nexternal-controller: 127.0.0.1:9090\nsecret: controller-private\nauthentication: [user-private:password-private]\ntun: {enable: true, stack: mixed}\n";
        std::fs::write(paths.settings().join("mihomo.yaml"), mihomo).unwrap();
        std::fs::write(paths.settings().join("dns.yaml"), "enable: true\nlisten: 127.0.0.1:1053\nnameserver: [https://dns.example.test/dns-query?token=dns-private, 1.1.1.1]\n").unwrap();
        std::fs::write(
            paths.settings().join("sniffer.yaml"),
            "enable: true\noverride-destination: false\n",
        )
        .unwrap();
        for area in ["profiles", "overrides"] {
            std::fs::create_dir_all(paths.data().join(area)).unwrap();
        }
        let profile = "proxies:\n- name: test-node\n  type: vless\n  server: private-server.test\n  port: 443\n  uuid: uuid-private\n  headers: {Authorization: header-private}\n  reality-opts: {private-key: key-private}\nrules: [MATCH,DIRECT]\n";
        std::fs::write(paths.data().join("profiles/1.yaml"), profile).unwrap();
        std::fs::write(paths.data().join("overrides/2.yaml"), "ipv6: false\n").unwrap();
        std::fs::write(
            paths.data().join("overrides/3.js"),
            "const credential = 'script-private';",
        )
        .unwrap();
        let enhance = serde_json::json!({"version":1,"values":{"proxies":"prepend: [{name: extra, type: trojan, password: enhance-private}]"}});
        std::fs::write(
            paths.indexes().join("profile-enhance-1.json"),
            serde_json::to_vec(&enhance).unwrap(),
        )
        .unwrap();
        std::fs::write(paths.indexes().join("service-owner.token"), "owner-private").unwrap();
        let runtime = paths.directory(DirectoryKind::Core).join("run-test");
        std::fs::create_dir(&runtime).unwrap();
        std::fs::write(runtime.join("candidate.yaml"), mihomo).unwrap();
        let output = export(&repository, &crate::logging::Feed::default()).unwrap();
        let mut zip = zip::ZipArchive::new(std::fs::File::open(output).unwrap()).unwrap();
        let mut files = std::collections::BTreeMap::new();
        for index in 0..zip.len() {
            let mut file = zip.by_index(index).unwrap();
            let name = file.name().unwrap().to_string();
            let mut text = String::new();
            file.read_to_string(&mut text).unwrap();
            files.insert(name, text);
        }
        assert_eq!(
            files
                .keys()
                .filter(|name| !name.contains('/'))
                .collect::<Vec<_>>(),
            vec!["environment.txt"]
        );
        assert!(
            files
                .keys()
                .all(|name| !name.ends_with("export-status.txt"))
        );
        let environment: serde_json::Value =
            serde_json::from_str(&files["environment.txt"]).unwrap();
        assert_eq!(
            environment["application"]["version"],
            env!("CARGO_PKG_VERSION")
        );
        assert!(environment["system"]["architecture"].is_string());
        assert!(environment["system"]["network_interfaces"].is_array());
        assert!(environment["environment_variables"].is_object());
        assert_eq!(
            environment
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<std::collections::BTreeSet<_>>(),
            ["application", "environment_variables", "system"]
                .into_iter()
                .collect()
        );
        let app: serde_json::Value =
            serde_json::from_str(&files["configuration/settings/settings.json"]).unwrap();
        assert_eq!(app["general"]["auto_run"], true);
        assert_eq!(app["general"]["theme_index"], "dark");
        assert_eq!(app["sysproxy"]["proxy_guard"], true);
        assert_eq!(app["profiles"]["items"][0]["id"], 1);
        let native: serde_yaml_ng::Value =
            serde_yaml_ng::from_str(&files["configuration/settings/mihomo.yaml"]).unwrap();
        assert_eq!(native["mixed-port"], 7890);
        assert_eq!(native["tun"]["enable"], true);
        assert_eq!(native["external-controller"], "127.0.0.1:9090");
        for name in [
            "configuration/settings/dns.yaml",
            "configuration/settings/sniffer.yaml",
            "configuration/profiles/1.yaml",
            "configuration/overrides/2.yaml",
            "configuration/indexes/profile-enhance-1.json",
            "configuration/runtime/run-test/candidate.yaml",
        ] {
            assert!(files.contains_key(name), "{name}");
        }
        let contents = files.values().cloned().collect::<String>();
        assert!(contents.contains("test-node"));
        assert!(contents.contains("1.1.1.1"));
        assert!(files["logs/service-current.log"].contains("service diagnostic tail"));
        assert!(!contents.contains("service-private"));
        for secret in [
            "controller-private",
            "password-private",
            "user-private",
            "private-recipient",
            "dns-private",
            "private-server.test",
            "uuid-private",
            "header-private",
            "key-private",
            "script-private",
            "subscription-private",
            "private-path",
            "url-private",
            "enhance-private",
            "owner-private",
            root.path().to_str().unwrap(),
        ] {
            assert!(!contents.contains(secret), "leaked {secret}");
        }
        assert_eq!(
            std::fs::read_to_string(paths.data().join("profiles/1.yaml")).unwrap(),
            profile
        );
        assert_eq!(
            std::fs::read_to_string(paths.settings().join("mihomo.yaml")).unwrap(),
            mihomo
        );
    }

    #[cfg(unix)]
    #[test]
    fn export_skips_symlinked_configuration_and_reports_partial_capture() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        let repository = Repository::open(paths.clone()).unwrap();
        let outside = root.path().join("outside.yaml");
        std::fs::write(&outside, "password: outside-private").unwrap();
        std::os::unix::fs::symlink(&outside, paths.settings().join("dns.yaml")).unwrap();
        std::os::unix::fs::symlink(root.path(), paths.data().join("profiles")).unwrap();
        let output = export(&repository, &crate::logging::Feed::default()).unwrap();
        let mut zip = zip::ZipArchive::new(std::fs::File::open(output).unwrap()).unwrap();
        assert!(zip.by_name("configuration/settings/dns.yaml").is_err());
        let mut status = String::new();
        zip.by_name("logs/program-current.log")
            .unwrap()
            .read_to_string(&mut status)
            .unwrap();
        assert!(status.contains("来源不是普通文件"));
        assert!(status.contains("profiles：目录未导出"));
        assert!(!status.contains("outside-private"));
    }

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
        zip.by_name("logs/program-current.log")
            .unwrap()
            .read_to_string(&mut check)
            .unwrap();
        assert!(check.contains("mihomo.yaml"));
        assert!(check.contains("dns.yaml"));
        assert!(check.contains("settings.json"));
        assert!(!check.contains("private-token"));
        assert!(!check.contains(root.path().to_str().unwrap()));
        let mut masked = String::new();
        zip.by_name("configuration/settings/settings.json.redacted.txt")
            .unwrap()
            .read_to_string(&mut masked)
            .unwrap();
        assert!(masked.starts_with('{'));
        assert!(!masked.contains("private-token"));
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
