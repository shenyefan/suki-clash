//! Optional first-run installation of verified runtime assets shipped with the app.
//! Invoked only by the core worker; never overwrites a user-managed runtime file.
use crate::storage::{AppPaths, DirectoryKind};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const LIMIT: u64 = 256 * 1024 * 1024;
const GEO: [&str; 4] = ["Country.mmdb", "ASN.mmdb", "geoip.dat", "geosite.dat"];

#[derive(Deserialize, Serialize)]
struct Manifest {
    version: u32,
    target: String,
    assets: Vec<Asset>,
}
#[derive(Deserialize, Serialize)]
struct Asset {
    name: String,
    sha256: String,
    size: u64,
}
pub(crate) fn core_name() -> &'static str {
    if cfg!(windows) {
        "mihomo.exe"
    } else {
        "mihomo"
    }
}
pub(crate) fn target() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

pub(crate) fn bundle_directory() -> Result<PathBuf, String> {
    let executable = std::env::current_exe().map_err(|_| "无法定位应用安装目录")?;
    bundle_beside(&executable)
}
fn bundle_beside(executable: &Path) -> Result<PathBuf, String> {
    let parent = executable.parent().ok_or("应用路径没有父目录")?;
    // The same relative contract is used by CI and the macOS app packager.
    if cfg!(target_os = "macos") && parent.file_name().is_some_and(|name| name == "MacOS") {
        Ok(parent
            .parent()
            .ok_or("应用包目录无效")?
            .join("Resources/runtime"))
    } else {
        Ok(parent.join("resources/runtime"))
    }
}

fn plain_file(path: &Path) -> Result<Option<fs::Metadata>, String> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => Ok(Some(meta)),
        Ok(_) => Err("资源位置包含符号链接、目录或特殊文件".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("无法检查资源文件".into()),
    }
}

/// Install available bundled files. Missing bundles/assets are allowed: the user
/// downloads the core, and Mihomo downloads geodata on demand. Partial progress is safe: every published file is
/// complete, each retry checks every destination, and no one-shot marker hides gaps.
/// Cancellation is checked between chunks and before the no-clobber commit.
pub(crate) fn prepare(
    paths: &AppPaths,
    bundle: &Path,
    cancelled: impl Fn() -> bool,
) -> Result<(), String> {
    paths.create().map_err(|_| "无法创建应用资源目录")?;
    let root = paths.directory(DirectoryKind::Core);
    let names = [core_name(), GEO[0], GEO[1], GEO[2], GEO[3]];
    // Existing files can be newer/user supplied. Do not replace them from the
    // installation package or use package timestamps as a version policy.
    let mut missing = Vec::new();
    for name in names {
        if cancelled() {
            return Err("资源准备已取消".into());
        }
        match plain_file(&root.join(name))? {
            Some(meta) if meta.len() > 0 && meta.len() <= LIMIT => {}
            Some(_) => {
                return Err(format!(
                    "{name} 文件为空或超过上限；请检查文件或重新安装完整应用包"
                ));
            }
            None => missing.push(name),
        }
    }
    if missing.is_empty() {
        return Ok(());
    }
    let manifest_path = bundle.join("manifest.json");
    let Some(meta) = plain_file(&manifest_path)? else {
        return Ok(());
    };
    if meta.len() > 64 * 1024 {
        return Err("运行资源清单超过上限".into());
    }
    let bytes = read_bounded(&manifest_path, 64 * 1024)?;
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|_| "运行资源清单格式无效")?;
    if manifest.version != 1 || manifest.target != target() || manifest.assets.len() != names.len()
    {
        return Err("运行资源清单版本、平台或文件数量不匹配".into());
    }
    for name in names {
        let matching = manifest
            .assets
            .iter()
            .filter(|asset| asset.name == name)
            .collect::<Vec<_>>();
        if matching.len() != 1 {
            return Err("运行资源清单缺少文件或存在重复文件".into());
        }
        let asset = matching[0];
        if asset.size == 0
            || asset.size > LIMIT
            || asset.sha256.len() != 64
            || !asset.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(format!("{name} 清单校验信息无效"));
        }
    }
    for name in missing {
        if plain_file(&bundle.join(name))?.is_none() {
            continue;
        }
        let asset = manifest
            .assets
            .iter()
            .find(|asset| asset.name == name)
            .ok_or("资源清单已失效")?;
        install(&bundle.join(name), &root.join(name), asset, &cancelled)?;
    }
    Ok(())
}

/// Called on the core worker, never during rendering.
pub(crate) fn geodata_notice(paths: &AppPaths) -> Option<String> {
    let root = paths.directory(DirectoryKind::Core);
    let missing: Vec<_> = GEO
        .into_iter()
        .filter(|name| {
            !fs::metadata(root.join(name)).is_ok_and(|meta| meta.is_file() && meta.len() > 0)
        })
        .collect();
    (!missing.is_empty()).then(|| format!(
        "缺少 Geo 数据文件：{}。内核启动时会按配置需要自动下载；网络异常可能导致下载或启动失败，请检查网络后重试，或前往外部资源更新。",
        missing.join("、")
    ))
}

pub(crate) fn require_core(paths: &AppPaths) -> Result<(), String> {
    let path = paths.directory(DirectoryKind::Core).join(core_name());
    if fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.len() > 0) {
        Ok(())
    } else {
        Err("未安装 Mihomo 内核，请前往「内核」界面，选择发行通道并下载内核。".into())
    }
}

fn install(
    source: &Path,
    destination: &Path,
    asset: &Asset,
    cancelled: &impl Fn() -> bool,
) -> Result<(), String> {
    let metadata = plain_file(source)?.ok_or_else(|| format!("安装包缺少 {}", asset.name))?;
    if metadata.len() != asset.size {
        return Err(format!("{} 文件长度校验失败", asset.name));
    }
    let mut input = fs::File::open(source).map_err(|_| format!("无法读取 {}", asset.name))?;
    let directory = destination.parent().ok_or("资源目标没有父目录")?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(directory).map_err(|_| "无法创建资源临时文件")?;
    let mut digest = Sha256::new();
    let mut size = 0u64;
    let mut chunk = [0u8; 64 * 1024];
    loop {
        if cancelled() {
            return Err("资源准备已取消".into());
        }
        let read = input
            .read(&mut chunk)
            .map_err(|_| format!("读取 {} 失败", asset.name))?;
        if read == 0 {
            break;
        }
        size = size.saturating_add(read as u64);
        if size > asset.size {
            return Err(format!("{} 文件长度校验失败", asset.name));
        }
        digest.update(&chunk[..read]);
        temporary
            .write_all(&chunk[..read])
            .map_err(|_| "写入资源临时文件失败")?;
    }
    if size != asset.size
        || digest
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
            != asset.sha256.to_ascii_lowercase()
    {
        return Err(format!("{} SHA-256 校验失败，未安装该文件", asset.name));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = if asset.name == core_name() {
            0o700
        } else {
            0o600
        };
        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(mode))
            .map_err(|_| "无法设置资源文件权限")?;
    }
    temporary
        .as_file()
        .sync_all()
        .map_err(|_| "无法同步资源文件")?;
    if cancelled() {
        return Err("资源准备已取消".into());
    }
    // Another instance may have installed a file while this instance copied it.
    // Never overwrite it, and never interpret an arbitrary special file as success.
    match temporary.persist_noclobber(destination) {
        Ok(_) => Ok(()),
        Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
            match plain_file(destination)? {
                Some(meta) if meta.len() > 0 && meta.len() <= LIMIT => Ok(()),
                _ => Err("资源文件在安装时发生变化，请关闭应用并检查安装文件".into()),
            }
        }
        Err(_) => Err(format!("无法安装 {}", asset.name)),
    }
}

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let file = fs::File::open(path).map_err(|_| "无法读取资源清单")?;
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "读取资源清单失败")?;
    if bytes.len() as u64 > limit {
        return Err("资源清单超过上限".into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests;
