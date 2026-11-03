//! GitHub Gist REST operations. No implicit upload, retry, or credential persistence.
use crate::network::{Http, Transport};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, time::Duration};
const FILE: &str = "suki-clash-backup.txt";
const MAX_RESPONSE: usize = 9 * 1024 * 1024;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Remote {
    pub(crate) id: String,
    pub(crate) description: String,
    pub(crate) updated_at: String,
    pub(crate) html_url: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Page {
    pub(crate) entries: Vec<Remote>,
    pub(crate) has_more: bool,
}
#[derive(Deserialize)]
struct GistFile {
    content: Option<String>,
    raw_url: Option<String>,
    truncated: Option<bool>,
}
#[derive(Deserialize)]
struct Response {
    id: String,
    description: Option<String>,
    updated_at: String,
    html_url: String,
    files: BTreeMap<String, GistFile>,
}
impl Response {
    fn remote(&self) -> Remote {
        Remote {
            id: self.id.clone(),
            description: self.description.clone().unwrap_or_default(),
            updated_at: self.updated_at.clone(),
            html_url: self.html_url.clone(),
        }
    }
}
pub(crate) struct Gist {
    client: reqwest::Client,
    token: String,
    endpoint: String,
}
impl Gist {
    pub(crate) fn new(http: &Http, token: &str) -> Result<Self, String> {
        if token.trim().is_empty() {
            return Err("请先保存GitHub令牌".into());
        }
        Ok(Self {
            client: http.client(&Transport::System, true)?,
            token: token.trim().to_owned(),
            endpoint: "https://api.github.com".into(),
        })
    }
    fn request(&self, method: reqwest::Method, suffix: &str) -> reqwest::RequestBuilder {
        self.client
            .request(method, format!("{}{suffix}", self.endpoint))
            .bearer_auth(&self.token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2026-03-10")
            .header("User-Agent", crate::network::DEFAULT_USER_AGENT)
            .timeout(Duration::from_secs(60))
    }
    async fn response(&self, request: reqwest::RequestBuilder) -> Result<Vec<u8>, String> {
        let mut response = request.send().await.map_err(crate::network::error)?;
        if !response.status().is_success() {
            return Err(format!(
                "GitHub Gist返回HTTP {}",
                response.status().as_u16()
            ));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE as u64)
        {
            return Err("Gist响应超过大小上限".into());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(crate::network::error)? {
            if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE {
                return Err("Gist响应超过大小上限".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }
    pub(crate) async fn list(&self, page: u32) -> Result<Page, String> {
        if page == 0 {
            return Err("Gist页码无效".into());
        }
        let bytes = self
            .response(self.request(
                reqwest::Method::GET,
                &format!("/gists?per_page=100&page={page}"),
            ))
            .await?;
        let responses: Vec<Response> =
            serde_json::from_slice(&bytes).map_err(|_| "Gist列表响应无效")?;
        let has_more = responses.len() == 100;
        Ok(Page {
            has_more,
            entries: responses
                .into_iter()
                .filter(|gist| gist.files.contains_key(FILE))
                .map(|gist| gist.remote())
                .collect(),
        })
    }
    pub(crate) async fn create(&self, content: &str) -> Result<Remote, String> {
        validate_content(content)?;
        let body = serde_json::json!({ "description": format!("Suki Clash backup {}", chrono::Utc::now().to_rfc3339()), "public": false, "files": { FILE: { "content": content } } });
        let bytes = self
            .response(
                self.request(reqwest::Method::POST, "/gists")
                    .header("Content-Type", "application/json")
                    .body(serde_json::to_vec(&body).map_err(|_| "无法编码Gist备份")?),
            )
            .await?;
        let gist: Response = serde_json::from_slice(&bytes)
            .map_err(|_| "Gist创建响应无效；请刷新列表确认是否已创建")?;
        validate_id(&gist.id)?;
        Ok(gist.remote())
    }
    pub(crate) async fn download(&self, id: &str) -> Result<String, String> {
        validate_id(id)?;
        let bytes = self
            .response(self.request(reqwest::Method::GET, &format!("/gists/{id}")))
            .await?;
        let gist: Response = serde_json::from_slice(&bytes).map_err(|_| "Gist备份响应无效")?;
        let file = gist.files.get(FILE).ok_or("Gist中没有Suki Clash备份文件")?;
        if file.truncated != Some(true) {
            let content = file.content.clone().ok_or("Gist备份正文缺失")?;
            validate_content(&content)?;
            return Ok(content);
        }
        let url = file.raw_url.as_deref().ok_or("Gist未提供完整备份地址")?;
        let parsed = url::Url::parse(url).map_err(|_| "Gist原始备份地址无效")?;
        if parsed.scheme() != "https"
            || parsed.host_str() != Some("gist.githubusercontent.com")
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err("Gist原始备份地址无效".into());
        }
        // Raw files deliberately receive no API bearer token.
        let bytes = self
            .response(
                self.client
                    .get(parsed)
                    .header("User-Agent", crate::network::DEFAULT_USER_AGENT)
                    .timeout(Duration::from_secs(60)),
            )
            .await?;
        let content = String::from_utf8(bytes).map_err(|_| "Gist备份不是UTF-8文本")?;
        validate_content(&content)?;
        Ok(content)
    }
    pub(crate) async fn delete(&self, id: &str) -> Result<(), String> {
        validate_id(id)?;
        self.response(self.request(reqwest::Method::DELETE, &format!("/gists/{id}")))
            .await
            .map(|_| ())
    }
}
fn validate_content(content: &str) -> Result<(), String> {
    if content.len() > 8 * 1024 * 1024 {
        return Err("Gist备份超过8MiB上限".into());
    }
    if !content.starts_with(super::HEADER)
        && !content.starts_with("-----BEGIN AGE ENCRYPTED FILE-----")
    {
        return Err("Gist中不是当前Suki Clash备份".into());
    }
    Ok(())
}
fn validate_id(id: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > 64 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("Gist ID无效".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn secret_gist_history_create_list_download_delete_use_current_api_without_retries() {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            for (index, method) in [
                "POST /gists ",
                "GET /gists?per_page=100&page=1 ",
                "GET /gists/abc123 ",
                "DELETE /gists/abc123 ",
            ]
            .into_iter()
            .enumerate()
            {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                let mut buffer = [0; 4096];
                loop {
                    let n = socket.read(&mut buffer).await.unwrap();
                    if n == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&buffer[..n]);
                    if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&bytes[..end]);
                        let length = headers
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(str::trim)
                                    .and_then(|n| n.parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if bytes.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                let request = String::from_utf8(bytes).unwrap();
                assert!(request.starts_with(method));
                assert!(
                    request
                        .to_ascii_lowercase()
                        .contains("authorization: bearer test-token")
                );
                if index == 0 {
                    let body: serde_json::Value =
                        serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
                    assert_eq!(body["public"], false);
                    assert_eq!(
                        body["files"][FILE]["content"],
                        "SUKI-CLASH-BACKUP-1\nbackup"
                    );
                }
                let object = serde_json::json!({"id":"abc123","description":"Suki backup","updated_at":"2026-11-03T00:00:00Z","html_url":"https://gist.github.com/abc123","files":{FILE:{"content":"SUKI-CLASH-BACKUP-1\nbackup","truncated":false}}});
                let body = if index == 1 {
                    serde_json::to_string(&vec![object]).unwrap()
                } else if index == 3 {
                    String::new()
                } else {
                    serde_json::to_string(&object).unwrap()
                };
                socket
                    .write_all(
                        format!(
                            "HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            if index == 3 {
                                "204 No Content"
                            } else {
                                "200 OK"
                            },
                            body.len()
                        )
                        .as_bytes(),
                    )
                    .await
                    .unwrap();
            }
        });
        let gist = Gist {
            client: Http::default().client(&Transport::Direct, false).unwrap(),
            token: "test-token".into(),
            endpoint: format!("http://{address}"),
        };
        assert_eq!(
            gist.create("SUKI-CLASH-BACKUP-1\nbackup").await.unwrap().id,
            "abc123"
        );
        assert_eq!(gist.list(1).await.unwrap().entries.len(), 1);
        assert_eq!(
            gist.download("abc123").await.unwrap(),
            "SUKI-CLASH-BACKUP-1\nbackup"
        );
        gist.delete("abc123").await.unwrap();
        task.await.unwrap();
        assert!(validate_id("../other").is_err());
        assert!(validate_id("").is_err());
    }
    #[tokio::test]
    async fn filtered_page_still_has_next_and_rejected_create_is_not_retried() {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            let count = socket.read(&mut request).await.unwrap();
            assert!(String::from_utf8_lossy(&request[..count]).starts_with("GET /gists?"));
            let object = serde_json::json!({"id":"abc123","description":"other","updated_at":"now","html_url":"https://gist.github.com/abc123","files":{"other.txt":{}}});
            let body = serde_json::to_string(&vec![object; 100]).unwrap();
            socket
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            drop(socket);
            let (mut socket, _) = listener.accept().await.unwrap();
            let count = socket.read(&mut request).await.unwrap();
            assert!(String::from_utf8_lossy(&request[..count]).starts_with("POST /gists "));
            socket.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
            drop(socket);
            assert!(
                tokio::time::timeout(Duration::from_millis(200), listener.accept())
                    .await
                    .is_err()
            );
        });
        let gist = Gist {
            client: Http::default().client(&Transport::Direct, false).unwrap(),
            token: "test-token".into(),
            endpoint,
        };
        let page = gist.list(1).await.unwrap();
        assert!(page.entries.is_empty());
        assert!(page.has_more);
        assert_eq!(
            gist.create("SUKI-CLASH-BACKUP-1\nbackup")
                .await
                .unwrap_err(),
            "GitHub Gist返回HTTP 503"
        );
        server.await.unwrap();
    }
}
