//! Streams service-owned Geo updates back before local validation or runtime replacement.
use std::{
    future::Future,
    path::Path,
    time::{Duration, UNIX_EPOCH},
};
use suki_service::RuntimeFileOutcome;
use tokio::io::AsyncWriteExt as _;

pub(super) async fn sync<R, F>(root: &Path, mut read: R) -> Result<usize, String>
where
    R: FnMut(String, u64) -> F,
    F: Future<Output = Result<RuntimeFileOutcome, String>>,
{
    let mut changed = 0;
    let mut failures = Vec::new();
    for name in suki_service::RUNTIME_GEO_ASSETS {
        match sync_file(&root.join(name), name, &mut read).await {
            Ok(true) => changed += 1,
            Ok(false) => {}
            Err(error) => failures.push((*name, error)),
        }
    }
    if failures.is_empty() {
        Ok(changed)
    } else {
        let error = crate::failure::scoped(failures);
        Err(if changed == 0 {
            error
        } else {
            format!("已同步 {changed} 项 Geo 数据；{error}")
        })
    }
}

async fn sync_file<R, F>(target: &Path, name: &str, read: &mut R) -> Result<bool, String>
where
    R: FnMut(String, u64) -> F,
    F: Future<Output = Result<RuntimeFileOutcome, String>>,
{
    let local_modified = match tokio::fs::symlink_metadata(target).await {
        Ok(metadata) if metadata.is_file() => {
            Some(metadata.modified().map_err(crate::logging::error_message)?)
        }
        Ok(_) => return Err("本地 Geo 路径不是普通文件".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.to_string()),
    };
    let (len, mtime, hex) = match read(name.into(), u64::MAX).await? {
        RuntimeFileOutcome::Absent => return Ok(false),
        RuntimeFileOutcome::Chunk {
            len,
            mtime_ns: Some(mtime),
            hex,
        } => (len, mtime, hex),
        _ => return Err("Geo 数据修改时间无法确认".into()),
    };
    if !hex.is_empty() || len == 0 || len > super::download::ARCHIVE_LIMIT {
        return Err("Geo 数据文件身份无效或超过上限".into());
    }
    let modified = UNIX_EPOCH + Duration::from_nanos(mtime);
    if local_modified.is_some_and(|local| local >= modified) {
        return Ok(false);
    }
    let temporary = tempfile::Builder::new()
        .prefix(".geo-sync-")
        .tempfile_in(target.parent().ok_or("Geo 数据目录不存在")?)
        .map_err(crate::logging::error_message)?;
    let (file, path) = temporary.into_parts();
    let mut file = tokio::fs::File::from_std(file);
    let mut offset = 0;
    loop {
        let RuntimeFileOutcome::Chunk {
            hex,
            len: current_len,
            mtime_ns,
        } = read(name.into(), offset).await?
        else {
            return Err("Geo 数据读取期间文件发生变化".into());
        };
        if (current_len, mtime_ns) != (len, Some(mtime)) {
            return Err("Geo 数据读取期间文件发生变化".into());
        }
        if offset == len {
            if !hex.is_empty() {
                return Err("Geo 数据结束分块无效".into());
            }
            break;
        }
        let bytes = super::provider_sync::decode_chunk(&hex)?;
        if bytes.is_empty() || bytes.len() as u64 > len - offset {
            return Err("Geo 数据分块长度不一致".into());
        }
        file.write_all(&bytes)
            .await
            .map_err(crate::logging::error_message)?;
        offset += bytes.len() as u64;
    }
    file.flush().await.map_err(crate::logging::error_message)?;
    file.sync_all()
        .await
        .map_err(crate::logging::error_message)?;
    let file = file.into_std().await;
    file.set_modified(modified)
        .map_err(crate::logging::error_message)?;
    drop(file);
    path.persist(target)
        .map_err(crate::logging::error_message)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;
    fn chunk(hex: &str, mtime: SystemTime) -> RuntimeFileOutcome {
        RuntimeFileOutcome::Chunk {
            hex: hex.into(),
            len: 4,
            mtime_ns: Some(mtime.duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64),
        }
    }
    #[tokio::test]
    async fn publishes_only_complete_newer_geo_and_preserves_runtime_timestamp() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("geoip.dat");
        std::fs::write(&path, b"old").unwrap();
        let modified = SystemTime::now() + Duration::from_secs(10);
        let mut read = |_: String, offset| {
            std::future::ready(Ok(chunk(
                match offset {
                    0 => "6162",
                    2 => "6364",
                    _ => "",
                },
                modified,
            )))
        };
        assert!(sync_file(&path, "geoip.dat", &mut read).await.unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), b"abcd");
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            modified
        );
        assert!(!sync_file(&path, "geoip.dat", &mut read).await.unwrap());
    }
    #[tokio::test]
    async fn newer_local_download_is_never_replaced() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("geoip.dat");
        std::fs::write(&path, b"local").unwrap();
        let mut read = |_: String, offset| {
            assert_eq!(offset, u64::MAX);
            std::future::ready(Ok(chunk("", UNIX_EPOCH + Duration::from_secs(10))))
        };
        assert!(!sync_file(&path, "geoip.dat", &mut read).await.unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), b"local");
    }
    #[tokio::test]
    async fn changing_service_file_keeps_local_copy_and_removes_temporary() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("geoip.dat");
        std::fs::write(&path, b"local").unwrap();
        let modified = SystemTime::now() + Duration::from_secs(10);
        let mut read = |_: String, offset| {
            std::future::ready(Ok(chunk(
                if offset == 0 { "6162" } else { "" },
                modified
                    + if offset == 2 {
                        Duration::from_secs(1)
                    } else {
                        Duration::ZERO
                    },
            )))
        };
        assert!(
            sync_file(&path, "geoip.dat", &mut read)
                .await
                .unwrap_err()
                .contains("变化")
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"local");
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    }
}
