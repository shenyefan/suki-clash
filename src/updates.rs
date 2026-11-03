//! Velopack owns installed packages; application HTTP and lifecycle policies remain ours.
use crate::{
    network::{Http, Transport},
    storage::{AppPaths, DirectoryKind},
};
use sha2::{Digest as _, Sha256};
use std::{
    path::{Component, Path},
    time::Duration,
};
use tokio::io::AsyncWriteExt as _;
use velopack::{
    UpdateCheck, UpdateInfo, UpdateManager, UpdateOptions, VelopackAssetFeed, sources::FileSource,
};

const PACKAGE_ID: &str = "SukiClash";
const PACKAGE_LIMIT: u64 = 1024 * 1024 * 1024;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct State {
    pub(crate) available: Option<String>,
    pub(crate) downloaded: Option<String>,
    pub(crate) checked: bool,
    pub(crate) manual_url: Option<String>,
}
pub(crate) enum Check {
    Managed(Option<Box<Release>>),
    Manual,
}
pub(crate) struct Release {
    info: UpdateInfo,
    directory: tempfile::TempDir,
    pub(crate) staged: bool,
}
pub(crate) fn channel() -> String {
    let os = match std::env::consts::OS {
        "windows" => "win",
        "macos" => "osx",
        _ => "linux",
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    };
    format!("{os}-{arch}")
}
fn repository() -> Result<&'static str, String> {
    let repository =
        option_env!("SUKI_UPDATE_REPOSITORY").ok_or("当前构建未配置 GitHub 更新仓库")?;
    if repository.split('/').count() != 2
        || repository.split('/').any(|part| {
            part.is_empty()
                || !part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        })
    {
        return Err("应用更新仓库配置无效".into());
    }
    Ok(repository)
}
pub(crate) fn manual_releases_url() -> Result<String, String> {
    let repository = if option_env!("SUKI_UPDATE_REPOSITORY").is_some() {
        repository()?
    } else {
        "shenyefan/suki-clash"
    };
    Ok(format!("https://github.com/{repository}/releases/latest"))
}
fn manager(source: &Path) -> Result<UpdateManager, velopack::Error> {
    UpdateManager::new(
        FileSource::new(source),
        Some(UpdateOptions {
            ExplicitChannel: Some(channel()),
            MaximumDeltasBeforeFallback: -1,
            ..Default::default()
        }),
        None,
    )
}
fn validate_feed(bytes: &[u8]) -> Result<VelopackAssetFeed, String> {
    let mut feed: VelopackAssetFeed =
        serde_json::from_slice(bytes).map_err(|_| "应用更新清单格式无效")?;
    feed.Assets.retain(|asset| asset.Type == "Full");
    for asset in &feed.Assets {
        let expected = format!("{PACKAGE_ID}-{}-{}-full.nupkg", asset.Version, channel());
        if asset.PackageId != PACKAGE_ID
            || asset.FileName != expected
            || !asset
                .Version
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".-+".contains(&b))
            || asset.FileName.contains('/')
            || asset.FileName.contains('\\')
            || Path::new(&asset.FileName)
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
            || asset.Size == 0
            || asset.Size > PACKAGE_LIMIT
            || asset.SHA256.len() != 64
            || !asset.SHA256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err("应用更新包的身份、路径、大小或摘要无效".into());
        }
    }
    Ok(feed)
}
async fn request(http: &Http, url: &str) -> Result<reqwest::Response, String> {
    http.client(&Transport::System, true)?
        .get(url)
        .header("User-Agent", crate::network::DEFAULT_USER_AGENT)
        .timeout(Duration::from_secs(600))
        .send()
        .await
        .map_err(crate::network::error)?
        .error_for_status()
        .map_err(crate::network::error)
}
pub(crate) async fn check(http: &Http, paths: &AppPaths) -> Result<Check, String> {
    // Detect an unmanaged developer build before accessing the network.
    match manager(&paths.directory(DirectoryKind::Cache)) {
        Ok(_) => {}
        Err(velopack::Error::NotInstalled(_)) => return Ok(Check::Manual),
        Err(error) => return Err(error.to_string()),
    }
    let directory = tempfile::Builder::new()
        .prefix("app-update-")
        .tempdir_in(paths.directory(DirectoryKind::Cache))
        .map_err(|error| error.to_string())?;
    let url = format!(
        "https://github.com/{}/releases/latest/download/releases.{}.json",
        repository()?,
        channel()
    );
    let mut response = request(http, &url).await?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(crate::network::error)? {
        if bytes.len().saturating_add(chunk.len()) > 2 * 1024 * 1024 {
            return Err("应用更新清单超过2MiB上限".into());
        }
        bytes.extend(chunk);
    }
    let feed = validate_feed(&bytes)?;
    let encoded = serde_json::to_vec(&feed).map_err(|error| error.to_string())?;
    tokio::fs::write(
        directory
            .path()
            .join(format!("releases.{}.json", channel())),
        encoded,
    )
    .await
    .map_err(|error| error.to_string())?;
    let source = directory.path().to_path_buf();
    let (update, staged) = tokio::task::spawn_blocking(move || {
        let manager = manager(&source).map_err(|error| error.to_string())?;
        let staged = manager
            .get_update_pending_restart()
            .map(|asset| asset.Version);
        manager
            .check_for_updates()
            .map(|update| (update, staged))
            .map_err(|error| format!("应用更新检查失败：{error}"))
    })
    .await
    .map_err(|_| "应用更新任务异常退出")??;
    match update {
        UpdateCheck::UpdateAvailable(info) => Ok(Check::Managed(Some(Box::new(Release {
            staged: staged.as_deref() == Some(info.TargetFullRelease.Version.as_str()),
            info: *info,
            directory,
        })))),
        UpdateCheck::NoUpdateAvailable => Ok(Check::Managed(None)),
        UpdateCheck::RemoteIsEmpty => Err("应用更新源没有可用发布包".into()),
    }
}

impl Release {
    pub(crate) fn version(&self) -> &str {
        &self.info.TargetFullRelease.Version
    }
    pub(crate) async fn download(
        &self,
        http: &Http,
        mut cancel: tokio::sync::watch::Receiver<bool>,
        mut progress: impl FnMut(u64, u64),
    ) -> Result<(), String> {
        let asset = &self.info.TargetFullRelease;
        let url = format!(
            "https://github.com/{}/releases/download/v{}/{}",
            repository()?,
            asset.Version,
            asset.FileName
        );
        let transfer = async {
            let response = request(http, &url).await?;
            stage_response(response, asset, self.directory.path(), &mut progress).await
        };
        tokio::select! {
            biased;
            _=cancel.changed()=>return Err("应用退出，更新下载已取消".into()),
            result=transfer=>result?,
        }
        let source = self.directory.path().to_path_buf();
        let update = self.info.clone();
        tokio::task::spawn_blocking(move || {
            let manager = manager(&source).map_err(|error| error.to_string())?;
            manager
                .download_updates(&update, None)
                .map_err(|error| format!("应用更新包准备失败：{error}"))?;
            if manager
                .get_update_pending_restart()
                .as_ref()
                .map(|asset| &asset.Version)
                != Some(&update.TargetFullRelease.Version)
            {
                return Err("应用更新包未确认就绪".into());
            }
            Ok(())
        })
        .await
        .map_err(|_| "应用更新包准备任务异常退出")?
    }
}
async fn stage_response(
    mut response: reqwest::Response,
    asset: &velopack::VelopackAsset,
    directory: &Path,
    progress: &mut impl FnMut(u64, u64),
) -> Result<(), String> {
    if response
        .content_length()
        .is_some_and(|size| size != asset.Size)
    {
        return Err("应用更新包长度与清单不一致".into());
    }
    let temporary =
        tempfile::NamedTempFile::new_in(directory).map_err(|error| error.to_string())?;
    let (file, path) = temporary.into_parts();
    let mut file = tokio::fs::File::from_std(file);
    let mut received = 0;
    let mut digest = Sha256::new();
    while let Some(chunk) = response.chunk().await.map_err(crate::network::error)? {
        received += chunk.len() as u64;
        if received > asset.Size {
            return Err("应用更新包超过声明大小".into());
        }
        digest.update(&chunk);
        file.write_all(&chunk)
            .await
            .map_err(|error| error.to_string())?;
        progress(received, asset.Size);
    }
    let digest = digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if received != asset.Size || !digest.eq_ignore_ascii_case(&asset.SHA256) {
        return Err("应用更新包大小或SHA-256校验失败".into());
    }
    file.sync_all().await.map_err(|error| error.to_string())?;
    drop(file);
    path.persist(directory.join(&asset.FileName))
        .map_err(|error| error.to_string())?;
    Ok::<_, String>(())
}

pub(crate) struct Installer {
    manager: UpdateManager,
    asset: velopack::VelopackAsset,
}
impl Installer {
    pub(crate) fn launch(self) -> Result<(), String> {
        self.manager
            .wait_exit_then_apply_updates(&self.asset, false, true, Vec::<String>::new())
            .map_err(|error| format!("无法启动应用更新：{error}"))
    }
}
pub(crate) fn prepare_installer(expected: &str) -> Result<Installer, String> {
    let manager = manager(&std::env::temp_dir()).map_err(|error| error.to_string())?;
    let asset = manager
        .get_update_pending_restart()
        .filter(|asset| asset.Version == expected)
        .ok_or("下载的应用更新已失效，请重新检查")?;
    Ok(Installer { manager, asset })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn feed(filename: String) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"Assets":[{"PackageId":PACKAGE_ID,"Version":"0.2.0","Type":"Full","FileName":filename,"SHA256":"a".repeat(64),"Size":20}]})).unwrap()
    }
    #[test]
    fn feed_rejects_other_channels_packages_and_paths() {
        let valid = feed(format!("SukiClash-0.2.0-{}-full.nupkg", channel()));
        assert!(validate_feed(&valid).is_ok());
        for name in [
            "../package.nupkg",
            "SukiClash-0.2.0-other-full.nupkg",
            "package.exe",
        ] {
            assert!(validate_feed(&feed(name.into())).is_err());
        }
        let mut value: serde_json::Value = serde_json::from_slice(&valid).unwrap();
        value["Assets"][0]["SHA256"] = "".into();
        assert!(validate_feed(&serde_json::to_vec(&value).unwrap()).is_err());
    }
    #[tokio::test]
    async fn bad_update_never_replaces_staged_package_and_valid_bytes_report_progress() {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
        for valid in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let target = root.path().join("update.nupkg");
            std::fs::write(&target, b"old package").unwrap();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut buf = [0; 1024];
                assert!(stream.read(&mut buf).await.unwrap() > 0);
                stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\npackage",
                    )
                    .await
                    .unwrap();
            });
            let response = Http::default()
                .client(&Transport::Direct, false)
                .unwrap()
                .get(format!("http://{address}"))
                .send()
                .await
                .unwrap();
            let asset = velopack::VelopackAsset {
                FileName: "update.nupkg".into(),
                Size: 7,
                SHA256: if valid {
                    Sha256::digest(b"package")
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect()
                } else {
                    "0".repeat(64)
                },
                ..Default::default()
            };
            let mut observed = Vec::new();
            assert_eq!(
                stage_response(response, &asset, root.path(), &mut |received, total| {
                    observed.push((received, total))
                })
                .await
                .is_ok(),
                valid
            );
            server.await.unwrap();
            assert_eq!(
                std::fs::read(target).unwrap(),
                if valid {
                    b"package".as_slice()
                } else {
                    b"old package".as_slice()
                }
            );
            assert_eq!(observed.last(), Some(&(7, 7)));
            assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
        }
    }

    #[tokio::test]
    async fn developer_build_update_check_fails_before_network_access() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        paths.create().unwrap();
        assert!(matches!(
            check(&Http::default(), &paths).await.ok(),
            Some(Check::Manual)
        ));
        assert_eq!(
            manual_releases_url().unwrap(),
            "https://github.com/shenyefan/suki-clash/releases/latest"
        );
        assert_eq!(
            std::fs::read_dir(paths.directory(DirectoryKind::Cache))
                .unwrap()
                .count(),
            0
        );
    }
}
