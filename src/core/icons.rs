//! Application-owned icon cache: bounded downloads, content validation and scoped publication.
use super::{download::Transfer, monitor::Target};
use crate::{
    network::Http,
    storage::{AppPaths, DirectoryKind},
};
use base64::Engine as _;
use sha2::{Digest as _, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::Arc,
};
use tokio::sync::watch;
const LIMIT: usize = 2 * 1024 * 1024;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct State {
    pub(crate) generation: u64,
    pub(crate) epoch: u64,
    pub(crate) files: BTreeMap<String, PathBuf>,
    pub(crate) pending: bool,
    pub(crate) completed: u64,
    pub(crate) total: u64,
    pub(crate) error: Option<String>,
}
pub(super) struct Request {
    pub(super) target: Target,
    pub(super) sources: BTreeSet<String>,
}
pub(super) async fn run(
    paths: AppPaths,
    mut requests: watch::Receiver<Option<Arc<Request>>>,
    updates: watch::Sender<State>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut cache = BTreeMap::<String, Result<PathBuf, String>>::new();
    'requests: loop {
        if *shutdown.borrow() {
            break;
        }
        let request = requests.borrow_and_update().clone();
        if let Some(request) = request {
            let mut state = State {
                generation: request.target.generation,
                epoch: request.target.epoch,
                pending: !request.sources.is_empty(),
                total: request.sources.len() as u64,
                ..Default::default()
            };
            updates.send_replace(state.clone());
            for source in &request.sources {
                if !cache.contains_key(source) {
                    let work = load(&request.target.http, &paths, source);
                    let result = tokio::select! {biased;_=shutdown.changed()=>return,changed=requests.changed()=>{if changed.is_err(){return;}continue 'requests;},result=work=>result};
                    // Bound failed-source retention as well as downloaded entries.
                    if cache.len() >= 512 {
                        cache.clear();
                    }
                    cache.insert(source.clone(), result);
                }
                if let Some(result) = cache.get(source) {
                    match result {
                        Ok(path) => {
                            state.files.insert(source.clone(), path.clone());
                        }
                        Err(error) => {
                            state.error = Some(error.clone());
                        }
                    }
                }
                state.completed += 1;
                updates.send_replace(state.clone());
            }
            state.pending = false;
            updates.send_replace(state);
        }
        tokio::select! {biased;_=shutdown.changed()=>break,changed=requests.changed()=>{if changed.is_err(){break;}}}
    }
}
async fn load(http: &Http, paths: &AppPaths, source: &str) -> Result<PathBuf, String> {
    let key: String = Sha256::digest(source.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let directory = paths.directory(DirectoryKind::Cache).join("proxy-icons");
    tokio::fs::create_dir_all(&directory)
        .await
        .map_err(|error| crate::logging::diagnostic_error("无法创建代理组图标缓存", error))?;
    let path = directory.join(key);
    let bytes = if let Ok(bytes) = tokio::fs::read(&path).await {
        bytes
    } else if source.starts_with("<svg") {
        source.as_bytes().to_vec()
    } else if let Some(data) = source.strip_prefix("data:") {
        let (metadata, data) = data.split_once(',').ok_or("图标 data 地址无效")?;
        if !metadata.starts_with("image/") {
            return Err("图标类型无效".into());
        }
        if metadata.ends_with(";base64") {
            base64::engine::general_purpose::STANDARD
                .decode(data)
                .map_err(|error| crate::logging::rejected("图标 Base64 无效", error))?
        } else {
            percent_encoding::percent_decode_str(data).collect()
        }
    } else {
        let mut transfer = Transfer::icon(http, paths, source)?;
        transfer.finish(|_, _| {}).await?
    };
    let destination = path.clone();
    tokio::task::spawn_blocking(move || {
        let bytes = validate(&bytes)?;
        crate::storage::atomic_write(&destination, &bytes).map_err(|error| {
            crate::logging::diagnostic_error("代理组图标缓存写入失败".to_owned(), error)
        })
    })
    .await
    .map_err(|error| crate::logging::diagnostic_error("图标缓存任务异常", error))??;
    Ok(path)
}
fn validate(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.len() > LIMIT {
        return Err("代理组图标超过 2 MiB".into());
    }
    let text = std::str::from_utf8(bytes)
        .unwrap_or("")
        .trim_start_matches('\u{feff}')
        .trim_start();
    if text.starts_with("<svg") || text.starts_with("<?xml") {
        let tree = usvg::Tree::from_data(bytes, &usvg::Options::default())
            .map_err(|error| crate::logging::rejected("SVG 图标无效", error))?;
        if tree.size().width() > 4096. || tree.size().height() > 4096. {
            return Err("代理组图标尺寸超过上限".into());
        }
        let bytes = tree.to_string(&usvg::WriteOptions::default()).into_bytes();
        if bytes.len() > LIMIT {
            return Err("代理组图标超过 2 MiB".into());
        }
        return Ok(bytes);
    }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| crate::logging::rejected("图标格式无效", error))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let decoded = reader
        .decode()
        .map_err(|error| crate::logging::rejected("代理组图标内容无效", error))?;
    let mut output = std::io::Cursor::new(Vec::new());
    decoded
        .write_to(&mut output, image::ImageFormat::Png)
        .map_err(|error| crate::logging::diagnostic_error("图标编码失败", error))?;
    let bytes = output.into_inner();
    if bytes.len() > LIMIT {
        return Err("代理组图标超过 2 MiB".into());
    }
    Ok(bytes)
}
#[cfg(test)]
mod tests {
    #[test]
    fn rejects_html_and_invalid_or_oversized_images() {
        assert!(super::validate(b"<html>not an icon</html>").is_err());
        assert!(super::validate(b"<svg xmlns='http://www.w3.org/2000/svg' width='32' height='32'><rect width='32' height='32'/></svg>").is_ok());
        assert!(
            super::validate(b"<svg xmlns='http://www.w3.org/2000/svg' width='10000' height='32'/>")
                .is_err()
        );
        assert!(super::validate(&vec![0; super::LIMIT + 1]).is_err());
    }
}

#[cfg(test)]
mod cache_tests {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    #[tokio::test]
    async fn downloads_once_and_reuses_validated_cache_without_network() {
        let root = tempfile::tempdir().unwrap();
        let paths = crate::storage::AppPaths::isolated(root.path());
        paths.create().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/icon", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buffer = [0; 4096];
            assert!(stream.read(&mut buffer).await.unwrap() > 0);
            let body=b"<svg xmlns='http://www.w3.org/2000/svg' width='32' height='32'><path d='M0 0L32 32'/></svg>";
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            stream.write_all(body).await.unwrap();
        });
        let http = crate::network::Http::default();
        let first = super::load(&http, &paths, &url).await.unwrap();
        server.await.unwrap();
        let second = super::load(&http, &paths, &url).await.unwrap();
        assert_eq!(first, second);
        assert!(std::fs::metadata(first).unwrap().len() > 0);
        let bytes=super::validate(b"<svg xmlns='http://www.w3.org/2000/svg' width='32' height='32'><image href='file:///not-an-app-resource.png' width='32' height='32'/></svg>").unwrap();
        assert!(!String::from_utf8(bytes).unwrap().contains("file://"));
    }
}
