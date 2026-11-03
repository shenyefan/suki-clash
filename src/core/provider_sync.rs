//! Provider cache publication is serialized by the lifecycle actor, for both core owners.
use super::{client::Client, launch::Launch};
use crate::storage::AppPaths;
use std::{
    collections::BTreeMap,
    future::Future,
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use suki_service::RuntimeFileOutcome;

const SETTLE: Duration = Duration::from_secs(2);
const CHUNK: u64 = 2 * 1024 * 1024;
type LoadTimes = BTreeMap<(&'static str, String), Option<u64>>;

pub(super) async fn sync_local(
    client: &Client,
    launch: &Launch,
    paths: &AppPaths,
) -> Result<usize, String> {
    let root = paths.directory(crate::storage::DirectoryKind::Core);
    sync(
        client,
        launch,
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
    launch: &Launch,
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
    let config: serde_yaml_ng::Mapping =
        serde_yaml_ng::from_str(&launch.composed).map_err(|_| "运行配置无效")?;
    let declarations = super::runtime_bundle::remote_providers_of(&config, &root)
        .map_err(|error| error.to_string())?;
    if declarations.is_empty() {
        return Ok(0);
    }
    let mut pending = 0;
    for declared in declarations {
        let key = (declared.section, declared.name.clone());
        let before = load_times(client).await?;
        let Some(before_time) = before.get(&key) else {
            pending += 1;
            continue;
        };
        let Some((bytes, identity)) =
            fetch(&mut read, &declared.provider.destination, now_ns()).await?
        else {
            pending += 1;
            continue;
        };
        let after = load_times(client).await?;
        if before.get(&key) != after.get(&key) || !finished_loading(identity.1, *before_time) {
            pending += 1;
            continue;
        }
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
    }
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
        if len > crate::configuration::INPUT_LIMIT {
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
    let before = file.metadata().await.map_err(|error| error.to_string())?;
    if !before.is_file() {
        return Err("Provider 缓存不是普通文件".into());
    }
    let mtime = modified_ns(&before);
    let mut bytes = Vec::new();
    if offset < before.len() {
        file.seek(std::io::SeekFrom::Start(offset))
            .await
            .map_err(|error| error.to_string())?;
        (&mut file)
            .take(CHUNK)
            .read_to_end(&mut bytes)
            .await
            .map_err(|error| error.to_string())?;
    }
    let after = tokio::fs::metadata(path)
        .await
        .map_err(|error| error.to_string())?;
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
    #[cfg(unix)]
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
        launch.composed = "proxy-providers: {p: {type: http, url: 'https://example.test/current', path: cache.yaml}}".into();
        let socket = directory.path().join("provider.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        launch.client = Client::new(super::super::client::Endpoint::Unix(socket), String::new());
        let changing = Arc::new(AtomicBool::new(true));
        let calls = Arc::new(AtomicUsize::new(0));
        let server = tokio::spawn({
            let changing = changing.clone();
            let calls = calls.clone();
            async move {
                loop {
                    let (mut stream, _) = listener.accept().await.unwrap();
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
                            r#"{{"providers":{{"p":{{"updatedAt":"1970-01-01T00:00:00.{nano:09}Z"}}}}}}"#
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
            sync(&launch.client, &launch, &paths, read, confirm, false)
                .await
                .unwrap(),
            1
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"old cache");
        assert!(!confirmed.load(Ordering::SeqCst));
        changing.store(false, Ordering::SeqCst);
        assert_eq!(
            sync(&launch.client, &launch, &paths, read, confirm, false)
                .await
                .unwrap(),
            0
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert!(confirmed.load(Ordering::SeqCst));
        server.abort();
    }
}
