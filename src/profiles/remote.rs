//! Decode subscription response metadata independently from transport and UI.
use super::model::{Document, TrafficUsage};
use std::{collections::BTreeMap, io::Read as _};
pub(crate) fn apply(
    document: &mut Document,
    bytes: &[u8],
    headers: &reqwest::header::HeaderMap,
    initial: bool,
) -> Result<(), String> {
    let mut body = Vec::new();
    if !document.age_key.is_empty() {
        let identity = document
            .age_key
            .parse::<age::x25519::Identity>()
            .map_err(|_| "Age 私钥无效")?;
        let decryptor = age::Decryptor::new(age::armor::ArmoredReader::new(bytes))
            .map_err(|_| "Age 订阅格式无效")?;
        let reader = decryptor
            .decrypt(std::iter::once(&identity as &dyn age::Identity))
            .map_err(|_| "Age 订阅解密失败")?;
        reader
            .take(crate::configuration::INPUT_LIMIT + 1)
            .read_to_end(&mut body)
            .map_err(|_| "读取 Age 订阅失败")?;
    } else {
        body.extend_from_slice(bytes);
    }
    if body.len() as u64 > crate::configuration::INPUT_LIMIT {
        return Err("订阅超过大小上限".into());
    }
    let content = String::from_utf8(body).map_err(|_| "订阅不是 UTF-8 文本")?;
    if document.kind == super::model::DocumentKind::JavaScript {
        crate::configuration::script::validate(&content)?;
    } else {
        crate::configuration::compose(
            content.as_bytes(),
            Some(document.id),
            &crate::storage::Values::new(),
        )
        .map_err(|_| "下载内容不是有效 YAML 对象")?;
    }

    let header = headers
        .iter()
        .map(|(name, value)| {
            Ok((
                name.as_str().to_owned(),
                value.to_str().map_err(|_| "订阅响应头编码无效")?.to_owned(),
            ))
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    if initial {
        if document.name == "远程订阅" || document.name == "远程覆写" || document.name.is_empty()
        {
            document.name = header
                .get("content-disposition")
                .and_then(|value| filename(value))
                .or_else(|| name_from_url(&document.url))
                .unwrap_or_else(|| "远程订阅".into());
        }
        if (document.interval.is_empty() || document.interval == "1440")
            && let Some(minutes) = header
                .get("profile-update-interval")
                .and_then(|value| value.parse::<u64>().ok())
                .and_then(|hours| hours.checked_mul(60))
                .filter(|minutes| *minutes > 0 && *minutes <= 525600)
        {
            document.interval = minutes.to_string();
        }
    }
    document.home = header
        .get("profile-web-page-url")
        .filter(|value| super::model::valid_remote_url(value))
        .cloned()
        .unwrap_or_default();
    document.usage = header
        .iter()
        .find(|(key, _)| {
            key.as_str() == "subscription-userinfo" || key.ends_with("-subscription-userinfo")
        })
        .and_then(|(_, value)| usage(value));
    document.content = content;
    document.updated_at = Some(chrono::Utc::now().timestamp());
    Ok(())
}
fn name_from_url(value: &str) -> Option<String> {
    let url = url::Url::parse(value).ok()?;
    let path = url
        .path_segments()
        .and_then(|mut segments| segments.rfind(|segment| !segment.is_empty()));
    let name = path
        .and_then(|path| {
            percent_encoding::percent_decode_str(path)
                .decode_utf8()
                .ok()
        })
        .map(|name| {
            name.trim_end_matches(".yaml")
                .trim_end_matches(".yml")
                .trim_end_matches(".js")
                .to_owned()
        })
        .filter(|name| {
            !name.trim().is_empty() && name.len() <= 256 && !name.chars().any(char::is_control)
        });
    name.or_else(|| url.host_str().map(str::to_owned))
}
fn filename(value: &str) -> Option<String> {
    let pairs = value
        .split(';')
        .filter_map(|part| part.trim().split_once('='))
        .collect::<Vec<_>>();
    let name = if let Some((_, value)) = pairs
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("filename*"))
    {
        let value = value.trim_matches('"');
        let (_, value) = value.split_once("''")?;
        percent_encoding::percent_decode_str(value)
            .decode_utf8()
            .ok()?
            .into_owned()
    } else {
        pairs
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case("filename"))?
            .1
            .trim_matches('"')
            .to_owned()
    };
    let name = name
        .trim()
        .trim_end_matches(".yaml")
        .trim_end_matches(".yml")
        .trim_end_matches(".js")
        .to_owned();
    (!name.is_empty() && name.len() <= 256 && !name.chars().any(char::is_control)).then_some(name)
}
fn usage(value: &str) -> Option<TrafficUsage> {
    let fields = value
        .split(';')
        .filter_map(|part| part.trim().split_once('='))
        .filter_map(|(key, value)| value.parse::<u64>().ok().map(|value| (key, value)))
        .collect::<BTreeMap<_, _>>();
    let total = *fields.get("total")?;
    if total == 0 {
        return None;
    }
    let used = fields
        .get("upload")
        .copied()
        .unwrap_or(0)
        .saturating_add(fields.get("download").copied().unwrap_or(0));
    let expires = fields
        .get("expire")
        .and_then(|value| i64::try_from(*value).ok())
        .and_then(|value| chrono::DateTime::from_timestamp(value, 0))
        .map(|date| date.format("%Y-%m-%d").to_string());
    Some(TrafficUsage {
        used_bytes: used,
        total_bytes: total,
        expires,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn headers(value: &[u8]) -> reqwest::header::HeaderMap {
        let text = std::str::from_utf8(value).unwrap();
        text.lines()
            .filter_map(|line| line.split_once(':'))
            .map(|(name, value)| {
                (
                    reqwest::header::HeaderName::from_bytes(name.trim().as_bytes()).unwrap(),
                    reqwest::header::HeaderValue::from_str(value.trim()).unwrap(),
                )
            })
            .collect()
    }
    use crate::profiles::model::DocumentKind;
    #[test]
    fn automatic_names_use_response_filename_then_url_without_query_secrets() {
        for (url, raw_headers, name) in [
            (
                "https://example.com/link?token=private",
                "Content-Disposition: attachment; filename=server.yaml\r\n",
                "server",
            ),
            (
                "https://example.com/%E6%B5%8B%E8%AF%95.yaml?token=private",
                "",
                "测试",
            ),
            ("https://example.com/?token=private", "", "example.com"),
        ] {
            let mut document = Document::new(1, DocumentKind::Profile);
            document.name = "远程订阅".into();
            document.url = url.into();
            apply(
                &mut document,
                b"rules: ['MATCH,DIRECT']\n",
                &headers(raw_headers.as_bytes()),
                true,
            )
            .unwrap();
            assert_eq!(document.name, name);
            assert!(!document.name.contains("private"));
        }
        let mut document = Document::new(1, DocumentKind::Profile);
        document.name = "自定义名称".into();
        document.url = "https://example.com/file.yaml".into();
        apply(
            &mut document,
            b"rules: ['MATCH,DIRECT']\n",
            &headers(b"Content-Disposition: attachment; filename=server.yaml\r\n"),
            true,
        )
        .unwrap();
        assert_eq!(document.name, "自定义名称");
    }
    #[test]
    fn final_redirect_headers_drive_usage_title_interval_and_home() {
        let mut doc = Document::new(9, DocumentKind::Profile);
        doc.name = "远程订阅".into();
        let raw_headers=b"Content-Disposition: attachment; filename*=UTF-8''%E8%AE%A2%E9%98%85.yaml\r\nSubscription-Userinfo: upload=5; download=10; total=100; expire=1893456000\r\nprofile-update-interval: 6\r\nprofile-web-page-url: https://example.com/home\r\n\r\n";
        apply(
            &mut doc,
            b"proxies: []\nrules: ['MATCH,DIRECT']\n",
            &headers(raw_headers),
            true,
        )
        .unwrap();
        assert_eq!(doc.name, "订阅");
        assert_eq!(doc.interval, "360");
        assert_eq!(doc.home, "https://example.com/home");
        let usage = doc.usage.as_ref().unwrap();
        assert_eq!(usage.used_bytes, 15);
        assert_eq!(usage.total_bytes, 100);
        assert_eq!(usage.expires.as_deref(), Some("2030-01-01"));
        assert!(doc.updated_at.is_some());
        doc.interval = "0".into();
        apply(
            &mut doc,
            b"rules: ['MATCH,DIRECT']\n",
            &headers(raw_headers),
            true,
        )
        .unwrap();
        assert_eq!(doc.interval, "0");
        assert!(!doc.automatic_updates());
    }
    #[test]
    fn rejected_response_never_overwrites_last_content() {
        let mut doc = Document::new(1, DocumentKind::Profile);
        let before = doc.content.clone();
        assert!(apply(&mut doc, b"<html>login page</html>", &headers(b""), false).is_err());
        assert_eq!(doc.content, before);
        assert!(apply(&mut doc, b"rules: [", &headers(b""), false).is_err());
        assert_eq!(doc.content, before);
    }
    #[test]
    fn encrypted_subscription_requires_matching_age_identity() {
        use age::secrecy::ExposeSecret as _;
        use std::io::Write as _;
        let identity = age::x25519::Identity::generate();
        let encryptor = age::Encryptor::with_recipients(std::iter::once(
            &identity.to_public() as &dyn age::Recipient
        ))
        .unwrap();
        let mut bytes = Vec::new();
        let mut writer = encryptor.wrap_output(&mut bytes).unwrap();
        writer.write_all(b"rules: ['MATCH,DIRECT']\n").unwrap();
        writer.finish().unwrap();
        let mut doc = Document::new(1, DocumentKind::Profile);
        doc.age_key = identity.to_string().expose_secret().to_string();
        apply(&mut doc, &bytes, &headers(b""), false).unwrap();
        assert!(doc.content.contains("MATCH,DIRECT"));
        doc.age_key = age::x25519::Identity::generate()
            .to_string()
            .expose_secret()
            .to_string();
        assert!(apply(&mut doc, &bytes, &headers(b""), false).is_err());
    }
}
