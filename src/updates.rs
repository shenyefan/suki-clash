//! Release metadata and downloads belong to the application; self_update installs staged artifacts.
use crate::{
    network::{Http, Transport},
    storage::{AppPaths, DirectoryKind},
};
use sha2::{Digest as _, Sha256};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::io::AsyncWriteExt as _;
const PACKAGE_LIMIT: u64 = 1024 * 1024 * 1024;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct State {
    pub(crate) available: Option<String>,
    pub(crate) downloaded: Option<String>,
    pub(crate) checked: bool,
    pub(crate) manual_url: Option<String>,
    pub(crate) changelog: String,
    pub(crate) comparison_url: Option<String>,
}
pub(crate) enum Check {
    Managed(Option<Box<Release>>),
    Manual,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Asset {
    file_name: String,
    size: u64,
    sha256: String,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: String,
    channel: String,
    portable: Asset,
    installer: Option<Asset>,
    service_version: String,
    service: Asset,
}
pub(crate) struct Release {
    manifest: Manifest,
    update_service: bool,
    pub(crate) staged: bool,
    pub(crate) changelog: Result<String, String>,
    pub(crate) comparison_url: String,
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
fn validate_feed(bytes: &[u8]) -> Result<Manifest, String> {
    let manifest: Manifest = serde_json::from_slice(bytes)
        .map_err(|error| crate::logging::rejected("应用更新清单格式无效", error))?;
    self_update::Release::builder()
        .version(&manifest.version)
        .build()
        .map_err(crate::logging::error_message)?;
    let channel = channel();
    if manifest.channel != channel {
        return Err("应用更新清单的平台不一致".into());
    }
    let prefix = format!("SukiClash-{}-{channel}", manifest.version);
    let suffix = if cfg!(windows) {
        "Update.zip"
    } else if cfg!(target_os = "macos") {
        "Update.tar.gz"
    } else {
        "AppImage"
    };
    let portable_name = if cfg!(target_os = "linux") {
        format!("{prefix}.AppImage")
    } else {
        format!("{prefix}-{suffix}")
    };
    validate_asset(&manifest.portable, &portable_name)?;
    match (&manifest.installer, cfg!(windows)) {
        (Some(asset), true) => validate_asset(asset, &format!("{prefix}-Setup.exe"))?,
        (None, false) => (),
        _ => return Err("应用更新清单的安装包类型无效".into()),
    }
    validate_asset(
        &manifest.service,
        &format!("{prefix}-Service{}", std::env::consts::EXE_SUFFIX),
    )?;
    if manifest.service_version.is_empty() {
        return Err("应用更新清单缺少服务版本".into());
    }
    Ok(manifest)
}
fn validate_asset(asset: &Asset, expected: &str) -> Result<(), String> {
    if asset.file_name != expected
        || asset.size == 0
        || asset.size > PACKAGE_LIMIT
        || asset.sha256.len() != 64
        || !asset.sha256.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("应用更新包的身份、路径、大小或摘要无效".into());
    }
    Ok(())
}
fn installed_edition_at(executable: &Path) -> bool {
    cfg!(windows) && executable.with_file_name("inno-install").is_file()
}
fn service_update_at(executable: &Path, asset: &Asset) -> Result<bool, String> {
    if !cfg!(windows) || installed_edition_at(executable) {
        return Ok(false);
    }
    let service = executable.with_file_name("suki-clash-service.exe");
    match std::fs::metadata(&service) {
        Ok(_) => file_matches(&service, asset).map(|matches| !matches),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(crate::logging::error_message(error)),
    }
}
fn selected_asset<'a>(manifest: &'a Manifest, executable: &Path) -> Result<&'a Asset, String> {
    if installed_edition_at(executable) {
        manifest
            .installer
            .as_ref()
            .ok_or_else(|| "应用更新清单缺少安装包".into())
    } else {
        Ok(&manifest.portable)
    }
}
fn stage_directory(paths: &AppPaths) -> PathBuf {
    paths.directory(DirectoryKind::Cache).join("app-update")
}
async fn request(http: &Http, paths: &AppPaths, url: &str) -> Result<reqwest::Response, String> {
    http.client(&Transport::System, true)?
        .get(crate::network::github_proxy_url(paths, url, "")?)
        .header("User-Agent", crate::network::DEFAULT_USER_AGENT)
        .timeout(Duration::from_secs(600))
        .send()
        .await
        .map_err(crate::network::error)?
        .error_for_status()
        .map_err(crate::network::error)
}
async fn manifest(http: &Http, paths: &AppPaths) -> Result<Manifest, String> {
    let url = format!(
        "https://github.com/{}/releases/latest/download/releases.{}.json",
        repository()?,
        channel()
    );
    let mut response = request(http, paths, &url).await?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(crate::network::error)? {
        if bytes.len().saturating_add(chunk.len()) > 2 * 1024 * 1024 {
            return Err("应用更新清单超过2MiB上限".into());
        }
        bytes.extend(chunk);
    }
    validate_feed(&bytes)
}
pub(crate) async fn check(http: &Http, paths: &AppPaths) -> Result<Check, String> {
    if option_env!("SUKI_UPDATE_REPOSITORY").is_none() {
        return Ok(Check::Manual);
    }
    let manifest = manifest(http, paths).await?;
    if !self_update::version::bump_is_greater(env!("CARGO_PKG_VERSION"), &manifest.version)
        .map_err(crate::logging::error_message)?
    {
        return Ok(Check::Managed(None));
    }
    let executable = std::env::current_exe().map_err(crate::logging::error_message)?;
    let service = manifest.service.clone();
    let service_executable = executable.clone();
    let update_service =
        tokio::task::spawn_blocking(move || service_update_at(&service_executable, &service))
            .await
            .map_err(crate::logging::error_message)??;
    let staged = match tokio::fs::read(stage_directory(paths).join("pending.json")).await {
        Ok(bytes) => {
            validate_feed(&bytes)? == manifest
                && tokio::fs::try_exists(
                    stage_directory(paths).join(&selected_asset(&manifest, &executable)?.file_name),
                )
                .await
                .map_err(crate::logging::error_message)?
                && (!update_service
                    || tokio::fs::try_exists(
                        stage_directory(paths).join(&manifest.service.file_name),
                    )
                    .await
                    .map_err(crate::logging::error_message)?)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(crate::logging::error_message(error)),
    };
    let repository = repository()?;
    let comparison = format!("v{}...v{}", env!("CARGO_PKG_VERSION"), manifest.version);
    let endpoint = format!("https://api.github.com/repos/{repository}/compare/{comparison}");
    let changelog = tokio::time::timeout(
        Duration::from_secs(60),
        changelog(
            &http.client(&Transport::System, true)?,
            paths,
            repository,
            &endpoint,
        ),
    )
    .await
    .map_err(|_| "获取完整变更超时".to_owned())
    .and_then(|value| value);
    Ok(Check::Managed(Some(Box::new(Release {
        manifest,
        update_service,
        staged,
        changelog,
        comparison_url: format!("https://github.com/{repository}/compare/{comparison}"),
    }))))
}

async fn changelog(
    client: &reqwest::Client,
    paths: &AppPaths,
    repository: &str,
    endpoint: &str,
) -> Result<String, String> {
    #[derive(serde::Deserialize)]
    struct Comparison {
        total_commits: usize,
        commits: Vec<Commit>,
    }
    #[derive(serde::Deserialize)]
    struct Commit {
        sha: String,
        commit: Message,
    }
    #[derive(serde::Deserialize)]
    struct Message {
        message: String,
    }
    let mut markdown = String::new();
    let mut received = 0;
    let mut page = 1;
    loop {
        let url = format!("{endpoint}?per_page=100&page={page}");
        let mut response = client
            .get(crate::network::github_proxy_url(paths, &url, "")?)
            .header("Accept", "application/vnd.github+json")
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(crate::network::error)?
            .error_for_status()
            .map_err(crate::network::error)?;
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(crate::network::error)? {
            if bytes.len().saturating_add(chunk.len()) > 8 * 1024 * 1024 {
                return Err("完整变更响应超过 8 MiB 上限".into());
            }
            bytes.extend(chunk);
        }
        let comparison: Comparison =
            serde_json::from_slice(&bytes).map_err(|error| format!("完整变更响应无效：{error}"))?;
        if comparison.commits.is_empty() && received < comparison.total_commits {
            return Err("GitHub 未返回完整的变更记录".into());
        }
        received += comparison.commits.len();
        for commit in comparison.commits {
            let title = commit.commit.message.lines().next().unwrap_or_default();
            markdown.push_str("- ");
            for character in title.chars() {
                if character.is_ascii_punctuation() {
                    markdown.push('\\');
                }
                markdown.push(character);
            }
            let short = commit.sha.chars().take(7).collect::<String>();
            let sha =
                url::form_urlencoded::byte_serialize(commit.sha.as_bytes()).collect::<String>();
            markdown.push_str(&format!(
                " ([{short}](https://github.com/{repository}/commit/{sha}))\n"
            ));
        }
        if received >= comparison.total_commits {
            return Ok(markdown);
        }
        page += 1;
    }
}

impl Release {
    pub(crate) fn version(&self) -> &str {
        &self.manifest.version
    }
    pub(crate) async fn download(
        &self,
        http: &Http,
        paths: &AppPaths,
        mut cancel: tokio::sync::watch::Receiver<bool>,
        mut progress: impl FnMut(u64, u64),
    ) -> Result<(), String> {
        let directory = stage_directory(paths);
        tokio::fs::create_dir_all(&directory)
            .await
            .map_err(crate::logging::error_message)?;
        let executable = std::env::current_exe().map_err(crate::logging::error_message)?;
        let asset = selected_asset(&self.manifest, &executable)?;
        let transfer = async {
            let assets =
                std::iter::once(asset).chain(self.update_service.then_some(&self.manifest.service));
            let total = asset.size
                + if self.update_service {
                    self.manifest.service.size
                } else {
                    0
                };
            let mut completed = 0;
            for asset in assets {
                download_asset(
                    http,
                    paths,
                    &self.manifest.version,
                    asset,
                    &directory,
                    &mut |received, _| progress(completed + received, total),
                )
                .await?;
                completed += asset.size;
            }
            Ok::<_, String>(())
        };
        tokio::select! {biased; _=cancel.changed()=>return Err("应用退出，更新下载已取消".into()), result=transfer=>result?,}
        let bytes = serde_json::to_vec(&self.manifest).map_err(crate::logging::error_message)?;
        tokio::task::spawn_blocking(move || {
            crate::storage::atomic_write(&directory.join("pending.json"), &bytes)
                .map_err(crate::logging::error_message)
        })
        .await
        .map_err(crate::logging::error_message)??;
        Ok(())
    }
}
async fn download_asset(
    http: &Http,
    paths: &AppPaths,
    version: &str,
    asset: &Asset,
    directory: &Path,
    progress: &mut impl FnMut(u64, u64),
) -> Result<(), String> {
    let url = format!(
        "https://github.com/{}/releases/download/v{version}/{}",
        repository()?,
        asset.file_name
    );
    stage_response(
        request(http, paths, &url).await?,
        asset,
        directory,
        progress,
    )
    .await
}
async fn stage_response(
    mut response: reqwest::Response,
    asset: &Asset,
    directory: &Path,
    progress: &mut impl FnMut(u64, u64),
) -> Result<(), String> {
    if response
        .content_length()
        .is_some_and(|size| size != asset.size)
    {
        return Err(format!("{} 的长度与更新清单不一致", asset.file_name));
    }
    let temporary =
        tempfile::NamedTempFile::new_in(directory).map_err(crate::logging::error_message)?;
    let (file, path) = temporary.into_parts();
    let mut file = tokio::fs::File::from_std(file);
    let mut received = 0;
    let mut digest = Sha256::new();
    while let Some(chunk) = response.chunk().await.map_err(crate::network::error)? {
        received += chunk.len() as u64;
        if received > asset.size {
            return Err(format!("{} 超过更新清单声明的大小", asset.file_name));
        }
        digest.update(&chunk);
        file.write_all(&chunk)
            .await
            .map_err(crate::logging::error_message)?;
        progress(received, asset.size);
    }
    let digest = digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if received != asset.size || !digest.eq_ignore_ascii_case(&asset.sha256) {
        return Err(format!("{} 的大小或 SHA-256 校验失败", asset.file_name));
    }
    file.sync_all()
        .await
        .map_err(crate::logging::error_message)?;
    drop(file);
    path.persist(directory.join(&asset.file_name))
        .map_err(crate::logging::error_message)?;
    Ok::<_, String>(())
}

pub(crate) struct Installer {
    manifest: Manifest,
    package: PathBuf,
    installed: bool,
    service: Option<(PathBuf, PathBuf)>,
}
impl Installer {
    /// Returns whether the application must relaunch after replacing itself.
    pub(crate) fn launch(self) -> Result<bool, String> {
        if self.installed {
            launch_inno(&self.package)?;
            return Ok(false);
        }
        let asset = &self.manifest.portable;
        let source = LocalPackage { path: self.package };
        let mut builder = self_update::backends::custom::Update::configure();
        builder
            .source(StagedRelease {
                version: self.manifest.version,
                name: asset.file_name.clone(),
            })
            .target(channel())
            .bin_name(format!("suki-clash{}", std::env::consts::EXE_SUFFIX))
            .current_version(env!("CARGO_PKG_VERSION"))
            .no_confirm(true)
            .show_output(false)
            .http_client(std::sync::Arc::new(source));
        #[cfg(windows)]
        builder.bin_path_in_archive("suki-clash.exe");
        #[cfg(target_os = "linux")]
        builder
            .bin_install_path(crate::desktop::executable()?)
            .bin_path_in_archive("suki-clash.AppImage")
            .verify_binary(|path| {
                use std::os::unix::fs::PermissionsExt as _;
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
                Ok(())
            });
        #[cfg(target_os = "macos")]
        builder.bundle_path_in_archive("Suki Clash.app");
        builder
            .build()
            .and_then(|updater| updater.update())
            .map_err(|error| format!("应用文件替换失败：{error}"))?;
        if let Some((source, destination)) = self.service {
            replace_service(&source, &destination)
                .map_err(|error| format!("主程序已替换，程序目录中的服务文件替换失败：{error}"))?;
        }
        Ok(true)
    }
}
fn replace_service(source: &Path, destination: &Path) -> Result<(), String> {
    let directory = tempfile::tempdir_in(destination.parent().ok_or("程序目录无效")?)
        .map_err(crate::logging::error_message)?;
    // Portable apps may live on a different drive from the user's download cache.
    let candidate = directory.path().join("suki-clash-service.exe");
    std::fs::copy(source, &candidate).map_err(crate::logging::error_message)?;
    self_update::Move::from_source(&candidate)
        .replace_using_temp(directory.path().join("previous-service"))
        .to_dest(destination)
        .map_err(crate::logging::error_message)
}
pub(crate) fn prepare_installer(expected: &str, paths: &AppPaths) -> Result<Installer, String> {
    let executable = std::env::current_exe().map_err(crate::logging::error_message)?;
    prepare_installer_at(expected, paths, &executable)
}
fn prepare_installer_at(
    expected: &str,
    paths: &AppPaths,
    executable: &Path,
) -> Result<Installer, String> {
    let directory = stage_directory(paths);
    let bytes =
        std::fs::read(directory.join("pending.json")).map_err(crate::logging::error_message)?;
    let manifest = validate_feed(&bytes)?;
    if manifest.version != expected {
        return Err("下载的应用更新已失效，请重新检查".into());
    }
    let asset = selected_asset(&manifest, executable)?;
    let package = directory.join(&asset.file_name);
    verify_file(&package, asset)?;
    let service = if service_update_at(executable, &manifest.service)? {
        let source = directory.join(&manifest.service.file_name);
        verify_file(&source, &manifest.service)?;
        Some((source, executable.with_file_name("suki-clash-service.exe")))
    } else {
        None
    };
    Ok(Installer {
        manifest,
        package,
        installed: installed_edition_at(executable),
        service,
    })
}
fn verify_file(path: &Path, asset: &Asset) -> Result<(), String> {
    if !file_matches(path, asset)? {
        return Err(format!("{} 的大小或 SHA-256 校验失败", path.display()));
    }
    Ok(())
}
fn file_matches(path: &Path, asset: &Asset) -> Result<bool, String> {
    use std::io::Read as _;
    let mut file = std::fs::File::open(path).map_err(crate::logging::error_message)?;
    let mut digest = Sha256::new();
    let mut size = 0;
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(crate::logging::error_message)?;
        if count == 0 {
            break;
        }
        size += count as u64;
        digest.update(&buffer[..count]);
    }
    let hash = digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(size == asset.size && hash.eq_ignore_ascii_case(&asset.sha256))
}
struct StagedRelease {
    version: String,
    name: String,
}
impl self_update::ReleaseSource for StagedRelease {
    fn get_releases(&self) -> self_update::Result<Vec<self_update::Release>> {
        Ok(vec![
            self_update::Release::builder()
                .version(&self.version)
                .asset(self_update::ReleaseAsset::new(
                    &self.name,
                    format!("https://staged.invalid/{}", self.name),
                ))
                .build()?,
        ])
    }
}
// self_update receives only the already verified local artifact. All network traffic stays on Http.
struct LocalPackage {
    path: PathBuf,
}
struct LocalResponse {
    file: std::fs::File,
    headers: self_update::http_client::HeaderMap,
}
impl self_update::http_client::HttpClient for LocalPackage {
    fn get(
        &self,
        _url: &str,
        _headers: &self_update::http_client::HeaderMap,
        _timeout: Option<Duration>,
    ) -> self_update::Result<Box<dyn self_update::http_client::HttpResponse>> {
        Ok(Box::new(LocalResponse {
            file: std::fs::File::open(&self.path)?,
            headers: Default::default(),
        }))
    }
}
impl self_update::http_client::HttpResponse for LocalResponse {
    fn headers(&self) -> &self_update::http_client::HeaderMap {
        &self.headers
    }
    fn body(self: Box<Self>) -> Box<dyn std::io::Read> {
        Box::new(self.file)
    }
}
#[cfg(windows)]
fn launch_inno(package: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt as _;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    let executable = std::env::current_exe().map_err(crate::logging::error_message)?;
    let directory = executable.parent().ok_or("程序目录无效")?;
    let parameters = format!(
        "/SILENT /SUPPRESSMSGBOXES /NORESTART /SUKIWAITPID={} /DIR=\"{}\"",
        std::process::id(),
        directory.display()
    );
    let file: Vec<u16> = package.as_os_str().encode_wide().chain(Some(0)).collect();
    let parameters: Vec<u16> = parameters.encode_utf16().chain(Some(0)).collect();
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            std::ptr::null(),
            file.as_ptr(),
            parameters.as_ptr(),
            std::ptr::null(),
            1,
        )
    } as isize;
    if result <= 32 {
        return Err(format!("无法启动安装程序（{result}）"));
    }
    Ok(())
}
#[cfg(not(windows))]
fn launch_inno(_package: &Path) -> Result<(), String> {
    Err("此平台不支持 Windows 安装程序".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn changelog_paginates_beyond_250_commits_and_preserves_titles_as_text() {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
        let root = tempfile::tempdir().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for page in 1..=3 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = [0; 2048];
                let count = stream.read(&mut request).await.unwrap();
                let request = String::from_utf8_lossy(&request[..count]);
                assert!(request.starts_with(&format!("GET /compare?per_page=100&page={page} ")));
                let start = (page - 1) * 100;
                let commits = (start..(start + 100).min(251)).map(|index| {
                    serde_json::json!({"sha":format!("{index:040x}"), "commit":{"message": if index == 0 { "fix: [文字](https://example.org) *原文*\n提交详情".to_owned() } else { format!("change {index}") }}})
                }).collect::<Vec<_>>();
                let body = serde_json::json!({"total_commits":251,"commits":commits}).to_string();
                stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            }
        });
        let client = Http::default().client(&Transport::Direct, false).unwrap();
        let notes = changelog(
            &client,
            &AppPaths::isolated(root.path()),
            "owner/repo",
            &format!("http://{address}/compare"),
        )
        .await
        .unwrap();
        server.await.unwrap();
        assert_eq!(notes.lines().count(), 251);
        assert!(notes.contains("change 250"));
        assert!(notes.contains(r"\[文字\]\(https\:\/\/example\.org\) \*原文\*"));
        assert!(!notes.contains("提交详情"));
        assert!(notes.contains("https://github.com/owner/repo/commit/"));
    }

    #[tokio::test]
    async fn incomplete_changelog_is_an_error_instead_of_partial_success() {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
        let root = tempfile::tempdir().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 2048];
            assert!(stream.read(&mut request).await.unwrap() > 0);
            let body = r#"{"total_commits":2,"commits":[]}"#;
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        });
        let client = Http::default().client(&Transport::Direct, false).unwrap();
        assert!(
            changelog(
                &client,
                &AppPaths::isolated(root.path()),
                "owner/repo",
                &format!("http://{address}/compare")
            )
            .await
            .unwrap_err()
            .contains("未返回完整")
        );
        server.await.unwrap();
    }
    #[tokio::test]
    async fn bad_update_never_replaces_staged_package_and_valid_bytes_report_progress() {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
        for valid in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let target = root.path().join("update.zip");
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
            let asset = Asset {
                file_name: "update.zip".into(),
                size: 7,
                sha256: if valid {
                    Sha256::digest(b"package")
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect()
                } else {
                    "0".repeat(64)
                },
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

    fn valid_manifest() -> Manifest {
        let prefix = format!("SukiClash-0.9.0-{}", channel());
        let asset = |file_name: String| Asset {
            file_name,
            size: 7,
            sha256: Sha256::digest(b"package")
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        };
        Manifest {
            version: "0.9.0".into(),
            channel: channel(),
            portable: asset(if cfg!(windows) {
                format!("{prefix}-Update.zip")
            } else if cfg!(target_os = "macos") {
                format!("{prefix}-Update.tar.gz")
            } else {
                format!("{prefix}.AppImage")
            }),
            installer: cfg!(windows).then(|| asset(format!("{prefix}-Setup.exe"))),
            service_version: suki_service::VERSION.into(),
            service: asset(format!("{prefix}-Service{}", std::env::consts::EXE_SUFFIX)),
        }
    }
    #[test]
    fn manifest_pins_platform_and_artifact_identity() {
        let valid = valid_manifest();
        assert!(validate_feed(&serde_json::to_vec(&valid).unwrap()).is_ok());
        for mutate in [
            |manifest: &mut Manifest| manifest.channel = "other".into(),
            |manifest: &mut Manifest| manifest.portable.file_name = "../update.zip".into(),
            |manifest: &mut Manifest| manifest.service.file_name = "latest-service.exe".into(),
            |manifest: &mut Manifest| manifest.service.sha256.clear(),
            |manifest: &mut Manifest| manifest.version = "../version".into(),
            |manifest: &mut Manifest| manifest.portable.size = PACKAGE_LIMIT + 1,
        ] {
            let mut invalid = valid.clone();
            mutate(&mut invalid);
            assert!(validate_feed(&serde_json::to_vec(&invalid).unwrap()).is_err());
        }
    }
    #[test]
    fn prepare_requires_expected_release_and_rechecks_downloaded_bytes() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        let directory = stage_directory(&paths);
        std::fs::create_dir_all(&directory).unwrap();
        let manifest = valid_manifest();
        std::fs::write(
            directory.join("pending.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let package = directory.join(&manifest.portable.file_name);
        std::fs::write(&package, b"package").unwrap();
        std::fs::write(directory.join(&manifest.service.file_name), b"package").unwrap();
        let executable = root.path().join("suki-clash.exe");
        assert!(prepare_installer_at("0.9.0", &paths, &executable).is_ok());
        assert!(prepare_installer_at("0.8.0", &paths, &executable).is_err());
        std::fs::write(&package, b"changed").unwrap();
        assert!(prepare_installer_at("0.9.0", &paths, &executable).is_err());
    }
    #[cfg(windows)]
    #[test]
    fn service_plan_tracks_release_bytes_and_excludes_installed_edition() {
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("suki-clash.exe");
        let service = root.path().join("suki-clash-service.exe");
        let mut manifest = valid_manifest();
        assert!(service_update_at(&executable, &manifest.service).unwrap());
        std::fs::write(&service, b"package").unwrap();
        assert!(!service_update_at(&executable, &manifest.service).unwrap());
        // A rebuilt service must be refreshed even if its package version was not bumped.
        std::fs::write(&service, b"changed").unwrap();
        assert!(service_update_at(&executable, &manifest.service).unwrap());
        std::fs::write(root.path().join("inno-install"), b"inno").unwrap();
        assert!(!service_update_at(&executable, &manifest.service).unwrap());
        assert_eq!(
            selected_asset(&manifest, &executable).unwrap(),
            manifest.installer.as_ref().unwrap()
        );
        std::fs::remove_file(root.path().join("inno-install")).unwrap();
        // The downloaded payload belongs to the target application, not this running build.
        manifest.service_version = "999.0.0".into();
        std::fs::write(&service, b"package").unwrap();
        assert!(!service_update_at(&executable, &manifest.service).unwrap());
    }
    #[cfg(windows)]
    #[test]
    fn portable_service_is_required_verified_and_replaced_only_beside_the_app() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        let directory = stage_directory(&paths);
        std::fs::create_dir_all(&directory).unwrap();
        let executable = root.path().join("suki-clash.exe");
        let service = root.path().join("suki-clash-service.exe");
        let system_service = root.path().join("system-service.exe");
        std::fs::write(&executable, b"old application").unwrap();
        std::fs::write(&service, b"old service").unwrap();
        std::fs::write(&system_service, b"installed service").unwrap();
        let mut manifest = valid_manifest();
        manifest.service_version = "999.0.0".into();
        std::fs::write(
            directory.join("pending.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        std::fs::write(directory.join(&manifest.portable.file_name), b"package").unwrap();
        assert!(prepare_installer_at("0.9.0", &paths, &executable).is_err());
        let staged_service = directory.join(&manifest.service.file_name);
        std::fs::write(&staged_service, b"changed").unwrap();
        assert!(prepare_installer_at("0.9.0", &paths, &executable).is_err());
        assert_eq!(std::fs::read(&service).unwrap(), b"old service");
        std::fs::write(&staged_service, b"package").unwrap();
        let installer = prepare_installer_at("0.9.0", &paths, &executable).unwrap();
        let (source, destination) = installer.service.unwrap();
        assert_eq!(destination, service);
        replace_service(&source, &destination).unwrap();
        assert_eq!(std::fs::read(&service).unwrap(), b"package");
        assert_eq!(
            std::fs::read(&system_service).unwrap(),
            b"installed service"
        );
        assert_eq!(std::fs::read(&executable).unwrap(), b"old application");
        // An unchanged service does not need a downloaded payload on the next update.
        std::fs::remove_file(staged_service).unwrap();
        assert!(
            prepare_installer_at("0.9.0", &paths, &executable)
                .unwrap()
                .service
                .is_none()
        );
    }
    #[test]
    fn self_update_installs_local_archive_into_isolated_destination() {
        use std::io::Write as _;
        let root = tempfile::tempdir().unwrap();
        let package = root.path().join("payload.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&package).unwrap());
        zip.start_file("application", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"new application").unwrap();
        zip.finish().unwrap();
        let destination = root.path().join("installed-application");
        std::fs::write(&destination, b"previous application").unwrap();
        self_update::backends::custom::Update::configure()
            .source(StagedRelease {
                version: "0.9.0".into(),
                name: format!("app-{}-Update.zip", channel()),
            })
            .http_client(std::sync::Arc::new(LocalPackage { path: package }))
            .target(channel())
            .bin_name("application")
            .bin_path_in_archive("application")
            .bin_install_path(&destination)
            .current_version("0.8.0")
            .no_confirm(true)
            .show_output(false)
            .build()
            .unwrap()
            .update()
            .unwrap();
        assert_eq!(std::fs::read(destination).unwrap(), b"new application");
    }
    #[cfg(windows)]
    #[test]
    fn inno_marker_is_local_to_the_install_directory() {
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("suki-clash.exe");
        assert!(!installed_edition_at(&executable));
        std::fs::write(root.path().join("unins000.exe"), b"unrelated").unwrap();
        assert!(!installed_edition_at(&executable));
        std::fs::write(root.path().join("inno-install"), b"inno").unwrap();
        assert!(installed_edition_at(&executable));
    }
}
