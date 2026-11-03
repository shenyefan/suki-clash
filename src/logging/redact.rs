//! Redact values at the diagnostic boundary while retaining the surrounding failure context.
use regex::{Captures, Regex};
use std::sync::LazyLock;

static URLS: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r#"(?i)[a-z][a-z0-9+.-]*://[^\s\"'<>；，。)）]+"#));
static CREDENTIALS: LazyLock<Result<Regex, regex::Error>> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)(\b(?:token|password|passwd|secret|authorization|age[_-]?key)\b[\"']?\s*[:=]\s*)(?:\"[^\"]*\"|'[^']*'|(?:Bearer\s+)?[^\s,;；，}]+)"#,
    )
});
static AGE_KEYS: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"AGE-SECRET-KEY-[A-Za-z0-9]+"));

pub(crate) fn redact(message: &str) -> String {
    let (Ok(urls), Ok(credentials), Ok(age_keys)) = (&*URLS, &*CREDENTIALS, &*AGE_KEYS) else {
        return "日志脱敏不可用".into();
    };
    let text = urls.replace_all(message, |capture: &Captures<'_>| {
        let Ok(url) = url::Url::parse(&capture[0]) else {
            return "<redacted-address>".to_owned();
        };
        let Some(host) = url.host() else {
            return format!("{}://<redacted-address>", url.scheme());
        };
        // Subscription credentials can also be path segments. Keep only the
        // public endpoint identity, not userinfo, path, query or fragment values.
        let port = url
            .port()
            .map(|port| format!(":{port}"))
            .unwrap_or_default();
        let omitted = !url.username().is_empty()
            || url.password().is_some()
            || !matches!(url.path(), "" | "/")
            || url.query().is_some()
            || url.fragment().is_some();
        format!(
            "{}://{host}{port}/{}",
            url.scheme(),
            if omitted { "<redacted>" } else { "" }
        )
    });
    let text = credentials.replace_all(&text, "${1}<redacted>");
    age_keys.replace_all(&text, "<redacted>").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_stage_status_and_recovery_while_redacting_endpoint_secrets() {
        let input = "下载 https://user:pass@example.test:8443/sub/key?token=private#secret 返回 HTTP 503；旧配置恢复 https://backup.test/api/private 失败：permission denied";
        let message = redact(input);
        for secret in ["user", "pass@", "private", "#secret", "/sub/key"] {
            assert!(!message.contains(secret), "{message}");
        }
        for context in [
            "下载",
            "example.test:8443",
            "HTTP 503",
            "旧配置恢复",
            "backup.test",
            "permission denied",
        ] {
            assert!(message.contains(context), "{message}");
        }
        assert_eq!(
            redact("配置已应用；连接清理失败：HTTP 500"),
            "配置已应用；连接清理失败：HTTP 500"
        );
    }
    #[test]
    fn redacts_explicit_credential_fields_and_age_keys() {
        let message = redact(
            r#"token=private password="with spaces" {"secret":"secret-value"} authorization=Bearer bearer-value AGE-SECRET-KEY-123ABC status=500"#,
        );
        assert!(!message.contains("private"));
        assert!(!message.contains("with spaces"));
        assert!(!message.contains("secret-value"));
        assert!(!message.contains("123ABC"));
        assert!(!message.contains("bearer-value"));
        assert!(message.contains("status=500"));
    }
}
