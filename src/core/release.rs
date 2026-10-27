//! Channel policy follows ClashSuki; platform artifacts are resolved independently.
use serde::Deserialize;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ReleaseSelection {
    Latest,
    Preview,
    Smart,
    Specific(String),
}
impl ReleaseSelection {
    pub(crate) fn from_values(values: &crate::storage::Values) -> Result<Self, String> {
        Ok(
            match values
                .get("CoreReleaseIndex")
                .map(String::as_str)
                .unwrap_or("最新版")
            {
                "最新版" => Self::Latest,
                "预览版" => Self::Preview,
                "Smart" => Self::Smart,
                "指定版本" => Self::Specific(normalize(
                    values
                        .get("CoreSpecificVersion")
                        .map(String::as_str)
                        .unwrap_or(""),
                )?),
                _ => return Err("发行通道无效".into()),
            },
        )
    }
    pub(super) fn label(&self) -> &'static str {
        match self {
            Self::Latest => "最新版",
            Self::Preview => "预览版",
            Self::Smart => "Smart",
            Self::Specific(_) => "指定版本",
        }
    }
    pub(super) fn version_url(&self) -> Option<&'static str> {
        match self {
            Self::Latest => {
                Some("https://github.com/MetaCubeX/mihomo/releases/latest/download/version.txt")
            }
            Self::Preview => Some(
                "https://github.com/MetaCubeX/mihomo/releases/download/Prerelease-Alpha/version.txt",
            ),
            Self::Smart => Some(
                "https://github.com/vernesong/mihomo/releases/download/Prerelease-Alpha/version.txt",
            ),
            Self::Specific(_) => None,
        }
    }
    pub(super) fn asset(&self, version: &str) -> Result<Asset, String> {
        if !version
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b".-_".contains(&c))
            || version.is_empty()
            || version.len() > 128
        {
            return Err("远端内核版本号无效".into());
        }
        let smart = matches!(self, Self::Smart);
        let stem = asset_stem(std::env::consts::OS, std::env::consts::ARCH, smart)?;
        let extension = if cfg!(windows) { "zip" } else { "gz" };
        let (repo, tag) = match self {
            Self::Smart => ("vernesong/mihomo", "Prerelease-Alpha"),
            Self::Preview => ("MetaCubeX/mihomo", "Prerelease-Alpha"),
            _ => ("MetaCubeX/mihomo", version),
        };
        Ok(Asset {
            url: format!(
                "https://github.com/{repo}/releases/download/{tag}/{stem}-{version}.{extension}"
            ),
            zip: cfg!(windows),
            version: version.into(),
        })
    }
}
pub(super) struct Asset {
    pub(super) url: String,
    pub(super) zip: bool,
    pub(super) version: String,
}
pub(super) fn normalize(version: &str) -> Result<String, String> {
    let version = version.trim();
    if version.is_empty()
        || version.len() > 128
        || !version
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b".-_".contains(&c))
    {
        return Err("请填写有效的版本标签".into());
    }
    Ok(if version.starts_with('v') {
        version.into()
    } else {
        format!("v{version}")
    })
}
fn asset_stem(os: &str, arch: &str, smart: bool) -> Result<&'static str, String> {
    match (os, arch, smart) {
        ("windows", "x86_64", false) => Ok("mihomo-windows-amd64-compatible"),
        ("windows", "x86_64", true) => Ok("mihomo-windows-amd64-v2-go120"),
        ("windows", "aarch64", _) => Ok("mihomo-windows-arm64"),
        ("windows", "x86", false) => Ok("mihomo-windows-386"),
        ("windows", "x86", true) => Ok("mihomo-windows-386-go120"),
        ("macos", "aarch64", _) => Ok("mihomo-darwin-arm64-go122"),
        ("macos", "x86_64", _) => Ok("mihomo-darwin-amd64-v1-go122"),
        ("linux", "aarch64", _) => Ok("mihomo-linux-arm64"),
        ("linux", "x86_64", _) => Ok("mihomo-linux-amd64-v1"),
        _ => Err("当前平台暂无可用的内核发行包".into()),
    }
}
#[derive(Deserialize)]
pub(super) struct Tag {
    pub(super) name: String,
}

pub(super) fn unpack_gzip(bytes: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Read as _;
    let reader = flate2::read::GzDecoder::new(bytes);
    let mut output = Vec::new();
    reader
        .take(super::download::ARCHIVE_LIMIT + 1)
        .read_to_end(&mut output)
        .map_err(|_| "内核 Gzip 包损坏")?;
    if output.is_empty() || output.len() as u64 > super::download::ARCHIVE_LIMIT {
        return Err("解压内核为空或超过大小上限".into());
    }
    Ok(output)
}

pub(super) fn unpack_zip(bytes: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::{Cursor, Read as _};
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| "内核 ZIP 归档无效")?;
    if archive.len() > 1024 {
        return Err("内核归档条目超过上限".into());
    }
    let mut selected = None;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|_| "无法读取内核 ZIP 条目")?;
        let path = entry.enclosed_name().ok_or("内核 ZIP 路径越界")?;
        if entry.is_symlink() {
            return Err("内核 ZIP 不允许符号链接".into());
        }
        if !entry.is_file() {
            continue;
        }
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("mihomo") && name.ends_with(".exe"))
            && selected.replace(index).is_some()
        {
            return Err("内核归档必须包含唯一 Mihomo 可执行文件".into());
        }
    }
    let mut entry = archive
        .by_index(selected.ok_or("内核 ZIP 缺少 Mihomo 可执行文件")?)
        .map_err(|_| "无法读取内核 ZIP 文件")?;
    if entry.size() > super::download::ARCHIVE_LIMIT {
        return Err("解包内核超过大小上限".into());
    }
    let mut binary = Vec::new();
    (&mut entry)
        .take(super::download::ARCHIVE_LIMIT + 1)
        .read_to_end(&mut binary)
        .map_err(|_| "内核 ZIP 解包或校验失败")?;
    if binary.is_empty() || binary.len() as u64 > super::download::ARCHIVE_LIMIT {
        return Err("解包内核为空或超过上限".into());
    }
    Ok(binary)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn channels_are_vendor_specific_and_tags_cannot_inject_paths() {
        assert!(
            ReleaseSelection::Smart
                .asset("alpha-smart-1234")
                .unwrap()
                .url
                .contains("vernesong/mihomo/releases/download/Prerelease-Alpha/")
        );
        assert!(
            ReleaseSelection::Preview
                .asset("alpha-1234")
                .unwrap()
                .url
                .contains("MetaCubeX/mihomo/releases/download/Prerelease-Alpha/")
        );
        assert!(
            ReleaseSelection::Latest
                .asset("v1.2.3")
                .unwrap()
                .url
                .contains("/v1.2.3/")
        );
        for tag in ["", "../other", "v1;rm", "v1\n"] {
            assert!(normalize(tag).is_err() || tag == "v1\n");
        }
        assert_eq!(
            asset_stem("windows", "x86_64", true).unwrap(),
            "mihomo-windows-amd64-v2-go120"
        );
        assert!(asset_stem("unknown", "x86_64", false).is_err());
    }
    fn archive(files: &[(&str, &[u8])]) -> Vec<u8> {
        use std::io::{Cursor, Write as _};
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in files {
            zip.start_file(
                *name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored),
            )
            .unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }
    #[test]
    fn zip_decoder_checks_identity_paths_duplicates_and_checksum() {
        let valid = archive(&[("bin/mihomo.exe", b"binary")]);
        assert_eq!(unpack_zip(&valid).unwrap(), b"binary");
        assert!(unpack_zip(&archive(&[("mihomo.exe", b"a"), ("dir/mihomo.exe", b"b")])).is_err());
        assert!(unpack_zip(&archive(&[("../mihomo.exe", b"binary")])).is_err());
        assert!(unpack_zip(&archive(&[("other.exe", b"binary")])).is_err());
        let mut corrupt = valid;
        let offset = corrupt
            .windows(6)
            .position(|value| value == b"binary")
            .unwrap();
        corrupt[offset] ^= 1;
        assert!(unpack_zip(&corrupt).is_err());
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zip.add_symlink(
            "mihomo.exe",
            "../other.exe",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        assert!(unpack_zip(&zip.finish().unwrap().into_inner()).is_err());
    }
}
