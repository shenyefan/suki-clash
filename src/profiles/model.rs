#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum DocumentKind {
    Profile,
    Yaml,
    JavaScript,
}
impl DocumentKind {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Profile => "配置",
            Self::Yaml => "YAML",
            Self::JavaScript => "JavaScript",
        }
    }
    pub(super) fn extension(self) -> &'static str {
        if self == Self::JavaScript {
            "js"
        } else {
            "yaml"
        }
    }
    pub(super) fn template(self) -> &'static str {
        match self {
            Self::JavaScript => "function main(config) {\n  // 在此修改配置\n  return config;\n}\n",
            Self::Yaml => "# 合并到运行配置\nprofile:\n  store-selected: true\n",
            Self::Profile => {
                "mixed-port: 7890\nmode: rule\nproxies: []\nproxy-groups: []\nrules:\n  - MATCH,DIRECT\n"
            }
        }
    }
}
#[derive(Clone)]
pub(super) struct TrafficUsage {
    pub(super) used_bytes: u64,
    pub(super) total_bytes: u64,
    pub(super) expires: Option<String>,
}
impl TrafficUsage {
    pub(super) fn fraction(&self) -> f32 {
        if self.total_bytes == 0 {
            return 0.;
        }
        (self.used_bytes as f64 / self.total_bytes as f64).clamp(0., 1.) as f32
    }
    pub(super) fn amount(bytes: u64) -> String {
        let gib = 1024_f64.powi(3);
        if bytes as f64 >= gib {
            format!("{:.1} GB", bytes as f64 / gib)
        } else {
            format!("{:.1} MB", bytes as f64 / 1024_f64.powi(2))
        }
    }
}

#[derive(Clone)]
pub(super) struct Document {
    pub(super) id: u64,
    pub(super) name: String,
    pub(super) url: String,
    pub(super) content: String,
    pub(super) kind: DocumentKind,
    pub(super) enabled: bool,
    pub(super) scope_profile: Option<u64>,
    pub(super) scope_name: String,
    pub(super) auto_update: bool,
    pub(super) interval: String,
    pub(super) timeout: String,
    pub(super) user_agent: String,
    pub(super) token: String,
    pub(super) age_key: String,
    pub(super) usage: Option<TrafficUsage>,
}
impl Document {
    pub(super) fn new(id: u64, kind: DocumentKind) -> Self {
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
            auto_update: false,
            interval: "1440".into(),
            timeout: "30".into(),
            user_agent: String::new(),
            token: String::new(),
            age_key: String::new(),
            usage: None,
        }
    }
    pub(super) fn stage(&self) -> &'static str {
        match (self.scope_profile.is_some(), self.kind) {
            (false, DocumentKind::JavaScript) => "全局脚本",
            (false, _) => "全局合并",
            (true, DocumentKind::JavaScript) => "订阅脚本",
            (true, _) => "订阅合并",
        }
    }
    pub(super) fn is_remote(&self) -> bool {
        !self.url.is_empty()
    }
}
pub(super) fn valid_remote_url(value: &str) -> bool {
    ["https://", "http://"]
        .iter()
        .find_map(|prefix| value.strip_prefix(prefix))
        .is_some_and(|rest| {
            let host = rest.split('/').next().unwrap_or_default();
            !host.is_empty() && !rest.chars().any(char::is_whitespace)
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
