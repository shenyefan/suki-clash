//! Provider cache passes belong to the lifecycle actor and are cancelled before commands.
use super::client::Client;
use crate::storage::AppPaths;
use std::{
    collections::BTreeMap,
    future::Future,
    path::Path,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use suki_service::RuntimeFileOutcome;

const SETTLE: Duration = Duration::from_secs(2);
const CHUNK: u64 = 2 * 1024 * 1024;
type LoadTimes = BTreeMap<(&'static str, String), Option<u64>>;

pub(super) async fn sync_local(
    client: &Client,
    composed: &str,
    paths: &AppPaths,
) -> Result<usize, String> {
    let root = paths.directory(crate::storage::DirectoryKind::Core);
    sync(
        client,
        composed,
        paths,
        |destination, offset| {
            let path = root.join(destination);
            async move { local_chunk(&path, offset).await }
        },
        || async { Ok(()) },
        true,
    )
    .await
}

pub(super) async fn sync<R, F, C, CF>(
    client: &Client,
    composed: &str,
    paths: &AppPaths,
    mut read: R,
    mut confirm: C,
    local: bool,
) -> Result<usize, String>
where
    R: FnMut(String, u64) -> F,
    F: Future<Output = Result<RuntimeFileOutcome, String>>,
    C: FnMut() -> CF,
    CF: Future<Output = Result<(), String>>,
{
    let root = paths.directory(crate::storage::DirectoryKind::Core);
    use tokio::io::{AsyncReadExt as _, AsyncSeekExt as _, AsyncWriteExt as _};
    let config: serde_yaml_ng::Mapping = serde_yaml_ng::from_str(composed)
        .map_err(|error| crate::logging::rejected("运行配置无效", error))?;
    let declarations = super::runtime_bundle::remote_providers_of(&config, &root)
        .map_err(crate::logging::error_message)?;
    if declarations.is_empty() {
        return Ok(0);
    }
    log::debug!(
        "开始同步 Provider 缓存，共 {} 项，本地进程 {}",
        declarations.len(),
        local
    );
    let started = Instant::now();
    let before = load_times(client).await?;
    let mut pending = 0;
    let mut fetched = Vec::new();
    for declared in declarations {
        let key = (declared.section, declared.name.clone());
        if !before.contains_key(&key) {
            pending += 1;
            continue;
        }
        let read_started = Instant::now();
        let content = fetch(&mut read, &declared.provider.destination, now_ns()).await?;
        log::debug!(
            "Provider {} {} 缓存读取结束，耗时 {} ms，可用 {}，字节数 {}",
            declared.section,
            declared.name,
            read_started.elapsed().as_millis(),
            content.is_some(),
            content.as_ref().map_or(0, |(bytes, _)| bytes.len())
        );
        let Some((bytes, identity)) = content else {
            pending += 1;
            continue;
        };
        // Keep the batch on disk so large rule providers do not accumulate in memory.
        // Dropping a cancelled pass closes and removes every anonymous temporary file.
        let mut file =
            tokio::fs::File::from_std(tempfile::tempfile().map_err(crate::logging::error_message)?);
        file.write_all(&bytes)
            .await
            .map_err(crate::logging::error_message)?;
        fetched.push((declared, file, identity));
    }
    let after = load_times(client).await?;
    for (declared, mut file, identity) in fetched {
        let verification_started = Instant::now();
        let key = (declared.section, declared.name.clone());
        if before.get(&key) != after.get(&key)
            || !finished_loading(identity.1, after.get(&key).copied().flatten())
        {
            pending += 1;
            continue;
        }
        file.rewind().await.map_err(crate::logging::error_message)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .await
            .map_err(crate::logging::error_message)?;
        let final_chunk = read(declared.provider.destination.clone(), identity.0).await?;
        if !eof_matches(final_chunk, identity) {
            pending += 1;
            continue;
        }
        // Actor serialization protects config identity. The service additionally
        // authenticates the same owner generation immediately before publication.
        confirm().await?;
        let path = root.join(&declared.provider.destination);
        if local {
            if !crate::proxies::chain_sources::confirm_local_cache(
                &path,
                &declared.provider.url,
                &bytes,
            )? {
                pending += 1;
            }
        } else {
            crate::proxies::chain_sources::publish_cache(&path, &declared.provider.url, &bytes)?;
        }
        log::debug!(
            "Provider {} {} 缓存确认及发布结束，耗时 {} ms",
            declared.section,
            declared.name,
            verification_started.elapsed().as_millis()
        );
    }
    log::debug!(
        "Provider 缓存同步本轮结束，耗时 {} ms，待重试 {pending} 项",
        started.elapsed().as_millis()
    );
    Ok(pending)
}

async fn load_times(client: &Client) -> Result<LoadTimes, String> {
    let (proxies, rules) = tokio::join!(
        client.json("/providers/proxies"),
        client.json("/providers/rules")
    );
    let mut result = BTreeMap::new();
    for (section, response) in [("proxy-providers", proxies?), ("rule-providers", rules?)] {
        let providers = response
            .get("providers")
            .and_then(serde_json::Value::as_object)
            .ok_or("Provider 状态响应无效")?;
        for (name, provider) in providers {
            let time = provider
                .get("updatedAt")
                .and_then(serde_json::Value::as_str)
                .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
                .and_then(|time| time.timestamp_nanos_opt())
                .and_then(|time| u64::try_from(time).ok());
            result.insert((section, name.clone()), time);
        }
    }
    Ok(result)
}
fn now_ns() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_nanos())
}
fn settled(mtime: Option<u64>, now: u128) -> bool {
    mtime.is_some_and(|mtime| now.saturating_sub(u128::from(mtime)) >= SETTLE.as_nanos())
}
fn finished_loading(mtime: Option<u64>, loaded: Option<u64>) -> bool {
    match (mtime, loaded) {
        (Some(mtime), Some(loaded)) => u128::from(mtime) <= u128::from(loaded) + SETTLE.as_nanos(),
        _ => false,
    }
}
fn eof_matches(outcome: RuntimeFileOutcome, identity: (u64, Option<u64>)) -> bool {
    matches!(outcome, RuntimeFileOutcome::Chunk { hex, len, mtime_ns } if hex.is_empty() && (len,mtime_ns)==identity)
}
async fn fetch<R, F>(
    read: &mut R,
    destination: &str,
    now: u128,
) -> Result<Option<(Vec<u8>, (u64, Option<u64>))>, String>
where
    R: FnMut(String, u64) -> F,
    F: Future<Output = Result<RuntimeFileOutcome, String>>,
{
    let mut bytes = Vec::new();
    let mut identity = None;
    loop {
        let RuntimeFileOutcome::Chunk { hex, len, mtime_ns } =
            read(destination.to_owned(), bytes.len() as u64).await?
        else {
            return Ok(None);
        };
        if len > crate::proxies::chain_sources::PROVIDER_CACHE_LIMIT {
            return Err("Provider 缓存超过读取上限".into());
        }
        if len == 0 || !settled(mtime_ns, now) {
            return Ok(None);
        }
        let current = (len, mtime_ns);
        if *identity.get_or_insert(current) != current {
            return Ok(None);
        }
        if bytes.len() as u64 == len {
            return Ok(if hex.is_empty() {
                Some((bytes, current))
            } else {
                None
            });
        }
        let chunk = decode_chunk(&hex)?;
        if chunk.is_empty() {
            return Err("后台服务返回空 Provider 分块".into());
        }
        bytes.extend(chunk);
        if bytes.len() as u64 > len {
            return Err("Provider 分块长度不一致".into());
        }
    }
}
pub(super) fn decode_chunk(hex: &str) -> Result<Vec<u8>, String> {
    if !hex.len().is_multiple_of(2) {
        return Err("运行文件分块编码无效".into());
    }
    hex.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let digit = |b| match b {
                b'0'..=b'9' => Some(b - b'0'),
                b'a'..=b'f' => Some(b - b'a' + 10),
                _ => None,
            };
            Ok(digit(pair[0]).ok_or("运行文件分块编码无效")? * 16
                + digit(pair[1]).ok_or("运行文件分块编码无效")?)
        })
        .collect()
}
async fn local_chunk(path: &Path, offset: u64) -> Result<RuntimeFileOutcome, String> {
    use tokio::io::{AsyncReadExt as _, AsyncSeekExt as _};
    let mut file = match tokio::fs::File::open(path).await {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(RuntimeFileOutcome::Absent);
        }
        Err(error) => return Err(format!("无法读取 Provider 缓存：{error}")),
    };
    let before = file
        .metadata()
        .await
        .map_err(crate::logging::error_message)?;
    if !before.is_file() {
        return Err("Provider 缓存不是普通文件".into());
    }
    let mtime = modified_ns(&before);
    let mut bytes = Vec::new();
    if offset < before.len() {
        file.seek(std::io::SeekFrom::Start(offset))
            .await
            .map_err(crate::logging::error_message)?;
        (&mut file)
            .take(CHUNK)
            .read_to_end(&mut bytes)
            .await
            .map_err(crate::logging::error_message)?;
    }
    let after = tokio::fs::metadata(path)
        .await
        .map_err(crate::logging::error_message)?;
    if before.len() != after.len() || mtime != modified_ns(&after) {
        return Ok(RuntimeFileOutcome::Absent);
    }
    Ok(RuntimeFileOutcome::Chunk {
        hex: bytes.iter().map(|byte| format!("{byte:02x}")).collect(),
        len: before.len(),
        mtime_ns: mtime,
    })
}
fn modified_ns(metadata: &std::fs::Metadata) -> Option<u64> {
    metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos()
        .try_into()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::launch::Launch;
    fn chunk(hex: &str, len: u64, time: u64) -> RuntimeFileOutcome {
        RuntimeFileOutcome::Chunk {
            hex: hex.into(),
            len,
            mtime_ns: Some(time),
        }
    }
    #[tokio::test]
    async fn single_chunk_requires_stability_and_an_eof_identity() {
        let mut calls = 0;
        let result = fetch(
            &mut |_, offset| {
                calls += 1;
                std::future::ready(Ok(if offset == 0 {
                    chunk("6162", 2, 1)
                } else {
                    chunk("", 2, 1)
                }))
            },
            "provider",
            3_000_000_001,
        )
        .await
        .unwrap();
        assert_eq!(result.unwrap().0, b"ab");
        assert_eq!(calls, 2);
        let mut calls = 0;
        assert!(
            fetch(
                &mut |_, _| {
                    calls += 1;
                    std::future::ready(Ok(chunk("6162", 2, 2_000_000_001)))
                },
                "provider",
                3_000_000_001
            )
            .await
            .unwrap()
            .is_none()
        );
        assert_eq!(calls, 1);
    }
    #[tokio::test]
    async fn changed_multichunk_or_final_identity_never_publishes() {
        let result = fetch(
            &mut |_, offset| {
                std::future::ready(Ok(if offset == 0 {
                    chunk("61", 2, 1)
                } else {
                    chunk("62", 2, 2)
                }))
            },
            "provider",
            3_000_000_001,
        )
        .await
        .unwrap();
        assert!(result.is_none());
        assert!(!eof_matches(chunk("", 2, 2), (2, Some(1))));
        assert!(!finished_loading(Some(5_000_000_000), Some(1)));
        assert!(!finished_loading(Some(1), None));
    }
    #[tokio::test]
    async fn provider_load_revision_changes_keep_old_cache_until_a_confirmed_read() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        };
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
        let directory = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(directory.path());
        crate::storage::Repository::open(paths.clone()).unwrap();
        let repository = crate::storage::Repository::open(paths.clone()).unwrap();
        crate::preferences::initialize(&repository).unwrap();
        let mut launch = Launch::preview(&paths).await.unwrap();
        launch.composed = "proxy-providers: {p: {type: http, url: 'https://example.test/current', path: cache.yaml}, q: {type: http, url: 'https://example.test/other', path: other.yaml}}".into();
        #[cfg(unix)]
        let socket = directory.path().join("provider.sock");
        #[cfg(unix)]
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        #[cfg(unix)]
        {
            launch.client =
                Client::new(super::super::client::Endpoint::Unix(socket), String::new());
        }
        #[cfg(windows)]
        let socket = format!(
            r"\\.\pipe\suki-provider-sync-{}",
            super::super::client::random_token().unwrap()
        );
        #[cfg(windows)]
        let mut listener = tokio::net::windows::named_pipe::ServerOptions::new()
            .first_pipe_instance(true)
            .create(&socket)
            .unwrap();
        #[cfg(windows)]
        {
            launch.client = Client::new(
                super::super::client::Endpoint::Pipe(socket.clone()),
                String::new(),
            );
        }
        let changing = Arc::new(AtomicBool::new(true));
        let calls = Arc::new(AtomicUsize::new(0));
        let server = tokio::spawn({
            let changing = changing.clone();
            let calls = calls.clone();
            async move {
                loop {
                    #[cfg(unix)]
                    let (mut stream, _) = listener.accept().await.unwrap();
                    #[cfg(windows)]
                    let mut stream = {
                        listener.connect().await.unwrap();
                        std::mem::replace(
                            &mut listener,
                            tokio::net::windows::named_pipe::ServerOptions::new()
                                .create(&socket)
                                .unwrap(),
                        )
                    };
                    let mut request = [0; 4096];
                    let count = stream.read(&mut request).await.unwrap();
                    let body = if request[..count].starts_with(b"GET /providers/proxies ") {
                        let call = calls.fetch_add(1, Ordering::SeqCst);
                        let nano = if changing.load(Ordering::SeqCst) {
                            call + 1
                        } else {
                            10
                        };
                        format!(
                            r#"{{"providers":{{"p":{{"updatedAt":"1970-01-01T00:00:00.{nano:09}Z"}},"q":{{"updatedAt":"1970-01-01T00:00:00.{nano:09}Z"}}}}}}"#
                        )
                    } else {
                        r#"{"providers":{}}"#.into()
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    stream.write_all(response.as_bytes()).await.unwrap();
                }
            }
        });
        let path = paths
            .directory(crate::storage::DirectoryKind::Core)
            .join("cache.yaml");
        std::fs::write(&path, b"old cache").unwrap();
        let bytes = b"proxies: []";
        let read = |_: String, offset: u64| {
            std::future::ready(Ok(RuntimeFileOutcome::Chunk {
                hex: if offset == 0 {
                    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
                } else {
                    String::new()
                },
                len: bytes.len() as u64,
                mtime_ns: Some(1),
            }))
        };
        let confirmed = std::sync::atomic::AtomicBool::new(false);
        let confirm = || {
            confirmed.store(true, Ordering::SeqCst);
            std::future::ready(Ok(()))
        };
        assert_eq!(
            sync(
                &launch.client,
                &launch.composed,
                &paths,
                read,
                confirm,
                false
            )
            .await
            .unwrap(),
            2
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(std::fs::read(&path).unwrap(), b"old cache");
        assert!(!path.with_file_name("other.yaml").exists());
        assert!(!confirmed.load(Ordering::SeqCst));
        changing.store(false, Ordering::SeqCst);
        assert_eq!(
            sync(
                &launch.client,
                &launch.composed,
                &paths,
                read,
                confirm,
                false
            )
            .await
            .unwrap(),
            0
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(
            std::fs::read(path.with_file_name("other.yaml")).unwrap(),
            bytes
        );
        assert_eq!(calls.load(Ordering::SeqCst), 4);
        assert!(confirmed.load(Ordering::SeqCst));
        server.abort();
    }
}
