#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DocumentKind {
    Profile,
    Yaml,
    JavaScript,
}
impl DocumentKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Profile => "配置",
            Self::Yaml => "YAML",
            Self::JavaScript => "JavaScript",
        }
    }
    pub(crate) fn extension(self) -> &'static str {
        if self == Self::JavaScript {
            "js"
        } else {
            "yaml"
        }
    }
    pub(crate) fn template(self) -> &'static str {
        match self {
            Self::JavaScript => "function main(config) {\n  // 在此修改配置\n  return config;\n}\n",
            Self::Yaml => "# 合并到运行配置\nprofile:\n  store-selected: true\n",
            Self::Profile => {
                "mixed-port: 7890\nmode: rule\nproxies: []\nproxy-groups: []\nrules:\n  - MATCH,DIRECT\n"
            }
        }
    }
}
#[derive(Clone, Debug)]
pub(crate) struct TrafficUsage {
    pub(crate) used_bytes: u64,
    pub(crate) total_bytes: u64,
    pub(crate) expires: Option<String>,
}
impl TrafficUsage {
    pub(crate) fn fraction(&self) -> f32 {
        if self.total_bytes == 0 {
            return 0.;
        }
        (self.used_bytes as f64 / self.total_bytes as f64).clamp(0., 1.) as f32
    }
    pub(crate) fn amount(bytes: u64) -> String {
        let gib = 1024_f64.powi(3);
        if bytes as f64 >= gib {
            format!("{:.1} GB", bytes as f64 / gib)
        } else {
            format!("{:.1} MB", bytes as f64 / 1024_f64.powi(2))
        }
    }
}

#[derive(Clone)]
pub(crate) struct Document {
    pub(crate) id: u64,
    pub(crate) name: String,
    pub(crate) url: String,
    pub(crate) content: String,
    pub(crate) kind: DocumentKind,
    pub(crate) enabled: bool,
    pub(crate) scope_profile: Option<u64>,
    pub(crate) scope_name: String,
    pub(crate) interval: String,
    pub(crate) timeout: String,
    pub(crate) user_agent: String,
    pub(crate) with_proxy: bool,
    pub(crate) self_proxy: bool,
    pub(crate) accept_invalid_certs: bool,
    pub(crate) token: String,
    pub(crate) age_key: String,
    pub(crate) credentials: super::credentials::CredentialEdits,
    pub(crate) usage: Option<TrafficUsage>,
    pub(crate) updated_at: Option<i64>,
    pub(crate) home: String,
}
impl std::fmt::Debug for Document {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Document")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}
impl Document {
    pub(crate) fn new(id: u64, kind: DocumentKind) -> Self {
        Self {
            id,
            name: if kind == DocumentKind::Profile {
                "本地配置".into()
            } else {
                format!("新建 {}", kind.label())
            },
            url: String::new(),
            content: kind.template().into(),
            kind,
            enabled: true,
            scope_profile: None,
            scope_name: String::new(),
            interval: "1440".into(),
            timeout: "30".into(),
            user_agent: String::new(),
            with_proxy: false,
            self_proxy: false,
            accept_invalid_certs: false,
            token: String::new(),
            age_key: String::new(),
            credentials: Default::default(),
            usage: None,
            updated_at: None,
            home: String::new(),
        }
    }
    pub(crate) fn stage(&self) -> &'static str {
        match (self.scope_profile.is_some(), self.kind) {
            (false, DocumentKind::JavaScript) => "全局脚本",
            (false, _) => "全局合并",
            (true, DocumentKind::JavaScript) => "订阅脚本",
            (true, _) => "订阅合并",
        }
    }
    pub(crate) fn automatic_updates(&self) -> bool {
        self.is_remote()
            && self
                .interval
                .parse::<u32>()
                .is_ok_and(|minutes| minutes > 0)
    }
    pub(crate) fn is_remote(&self) -> bool {
        !self.url.is_empty()
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SubscriptionImport {
    pub(crate) url: String,
    pub(crate) name: Option<String>,
}

pub(crate) fn valid_remote_url(value: &str) -> bool {
    value.len() <= 8192
        && url::Url::parse(value).ok().is_some_and(|url| {
            matches!(url.scheme(), "http" | "https")
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && !value.chars().any(char::is_whitespace)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn override_stages_follow_scope_and_language() {
        let mut doc = Document::new(42, DocumentKind::Yaml);
        assert_eq!(doc.stage(), "全局合并");
        doc.kind = DocumentKind::JavaScript;
        assert_eq!(doc.stage(), "全局脚本");
        doc.scope_profile = Some(7);
        assert_eq!(doc.stage(), "订阅脚本");
        doc.kind = DocumentKind::Yaml;
        assert_eq!(doc.stage(), "订阅合并");
    }
    #[test]
    fn usage_progress_handles_empty_and_exceeded_quotas() {
        let mut usage = TrafficUsage {
            used_bytes: 120,
            total_bytes: 100,
            expires: None,
        };
        assert_eq!(usage.fraction(), 1.);
        usage.used_bytes = 25;
        assert_eq!(usage.fraction(), 0.25);
        usage.total_bytes = 0;
        assert_eq!(usage.fraction(), 0.);
    }
    #[test]
    fn remote_urls_require_network_scheme_and_host() {
        for url in [
            "https://example.com/sub?token=123",
            "http://127.0.0.1:8000/config",
        ] {
            assert!(valid_remote_url(url));
        }
        for url in [
            "file:///etc/passwd",
            "https://",
            "ftp://host/file",
            "https://exam ple.com",
        ] {
            assert!(!valid_remote_url(url));
        }
    }
}
