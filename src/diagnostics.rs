//! Explicit diagnostic export. User source contents, URLs, secrets and log bodies are excluded.
use crate::{
    preferences::{self, SettingsKind},
    storage::{AppPaths, DirectoryKind, Repository},
};
use std::{io::Write as _, path::PathBuf};

pub(crate) fn export(repository: &Repository) -> Result<PathBuf, String> {
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
    let mut temporary =
        tempfile::NamedTempFile::new_in(&directory).map_err(|_| "无法创建诊断文件")?;
    {
        let mut zip = zip::ZipWriter::new(temporary.as_file_mut());
        for (name, content) in [
            ("environment.txt", environment),
            ("settings-check.txt", report),
            ("files.txt", files),
        ] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .map_err(|_| "无法写入诊断包")?;
            zip.write_all(content.as_bytes())
                .map_err(|_| "无法写入诊断信息")?;
        }
        zip.finish().map_err(|_| "无法完成诊断包")?;
    }
    temporary
        .as_file_mut()
        .sync_all()
        .map_err(|_| "无法保存诊断包")?;
    let path = directory.join(format!(
        "suki-diagnostics-{}.zip",
        chrono::Utc::now().format("%Y%m%dT%H%M%S%.f")
    ));
    temporary
        .persist_noclobber(&path)
        .map_err(|_| "无法提交诊断包")?;
    Ok(path)
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
        let path = export(&repository).unwrap();
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
}
