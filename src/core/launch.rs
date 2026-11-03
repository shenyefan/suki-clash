use super::client::{Client, Endpoint, random_token};
use crate::{
    configuration,
    storage::{AppPaths, DirectoryKind, Namespace, Repository},
};
use serde_yaml_ng::Value;
#[cfg(windows)]
use std::io;
use std::{path::PathBuf, process::Stdio};
use tempfile::TempDir;
use tokio::process::Command;

const SOURCE_LIMIT: u64 = configuration::INPUT_LIMIT;

pub(super) struct Launch {
    pub(super) force_service: bool,
    executable: PathBuf,
    work_dir: PathBuf,
    config: PathBuf,
    pub(super) source: configuration::Source,
    pub(super) composed: String,
    pub(super) client: Client,
    // The owner outlives the child. Never remove another generation's files.
    _directory: TempDir,
    #[cfg(windows)]
    pipe_sddl: String,
}

#[derive(Default)]
struct Composition {
    preferences: Option<configuration::Preferences>,
    executable: Option<PathBuf>,
    enhancement: Option<crate::storage::Values>,
    chain: Option<crate::proxies::chain_config::Config>,
    overrides: Option<crate::storage::Values>,
    flow: Option<(crate::preferences::SettingsKind, crate::storage::Values)>,
}

impl Launch {
    pub(super) fn tun_enabled(&self) -> Result<bool, String> {
        let value: Value = serde_yaml_ng::from_str(&self.composed).map_err(|_| "运行配置无效")?;
        configuration::tun::enabled(&value)
    }
    pub(super) fn privileged_network(&self) -> Result<bool, String> {
        let config: Value = serde_yaml_ng::from_str(&self.composed).map_err(|_| "运行配置无效")?;
        Ok(self.tun_enabled()?
            || (cfg!(target_os = "linux")
                && ["tproxy-port", "routing-mark"].iter().any(|key| {
                    config
                        .get(*key)
                        .and_then(Value::as_u64)
                        .is_some_and(|value| value > 0)
                })))
    }
    pub(super) async fn service_bundle(&self) -> Result<suki_service::RuntimeBundle, String> {
        let core = suki_service::service_paths()
            .map_err(|error| error.to_string())?
            .core_dir()
            .join(crate::bootstrap::core_name());
        super::runtime_bundle::collect_runtime_bundle_from(&self.config, &core, &self.work_dir)
            .await
            .map_err(|error| error.to_string())
    }
    pub(super) fn service_endpoint(&mut self, identity: &suki_service::OwnerIdentity) {
        let path = suki_service::mihomo_ipc_path(identity);
        #[cfg(unix)]
        let endpoint = Endpoint::Unix(path.into());
        #[cfg(windows)]
        let endpoint = Endpoint::Pipe(path);
        self.client.set_endpoint(endpoint);
    }
    pub(super) fn redactions(&self) -> Vec<String> {
        fn collect(value: &Value, secrets: &mut Vec<String>) {
            match value {
                Value::Mapping(map) => {
                    for (key, value) in map {
                        if key.as_str().is_some_and(|key| {
                            ["secret", "password", "token", "uuid", "private-key"].contains(&key)
                        }) && let Some(value) = value.as_str()
                        {
                            secrets.push(value.into());
                        }
                        if key.as_str() == Some("authentication")
                            && let Some(items) = value.as_sequence()
                        {
                            for item in items {
                                if let Some((_, password)) =
                                    item.as_str().and_then(|s| s.split_once(':'))
                                {
                                    secrets.push(password.into());
                                }
                            }
                        }
                        collect(value, secrets);
                    }
                }
                Value::Sequence(items) => {
                    for item in items {
                        collect(item, secrets);
                    }
                }
                _ => {}
            }
        }
        let mut secrets = Vec::new();
        if let Ok(value) = serde_yaml_ng::from_str::<Value>(&self.composed) {
            collect(&value, &mut secrets);
        }
        secrets
    }
    pub(super) fn use_executable(&mut self, path: PathBuf) {
        self.executable = path;
    }
    pub(super) async fn prepare(paths: &AppPaths) -> Result<Self, String> {
        Self::from_source(paths, configuration::Source::load(paths).await?).await
    }

    pub(super) async fn preview(paths: &AppPaths) -> Result<Self, String> {
        Self::compose_source(
            paths,
            configuration::Source::load(paths).await?,
            true,
            Composition {
                executable: Some(
                    paths
                        .directory(DirectoryKind::Core)
                        .join(crate::bootstrap::core_name()),
                ),
                ..Default::default()
            },
        )
        .await
    }
    pub(super) async fn from_source(
        paths: &AppPaths,
        source: configuration::Source,
    ) -> Result<Self, String> {
        Self::compose_source(paths, source, true, Composition::default()).await
    }

    pub(super) async fn with_enhancement(
        paths: &AppPaths,
        source: configuration::Source,
        enhancement: crate::storage::Values,
    ) -> Result<Self, String> {
        Self::compose_source(
            paths,
            source,
            true,
            Composition {
                enhancement: Some(enhancement),
                ..Default::default()
            },
        )
        .await
    }
    pub(super) async fn with_preferences(
        paths: &AppPaths,
        source: configuration::Source,
        preferences: configuration::Preferences,
    ) -> Result<Self, String> {
        Self::compose_source(
            paths,
            source,
            true,
            Composition {
                preferences: Some(preferences),
                ..Default::default()
            },
        )
        .await
    }
    pub(super) async fn with_chain(
        paths: &AppPaths,
        source: configuration::Source,
        chain: crate::proxies::chain_config::Config,
    ) -> Result<Self, String> {
        Self::compose_source(
            paths,
            source,
            true,
            Composition {
                chain: Some(chain),
                ..Default::default()
            },
        )
        .await
    }
    pub(super) async fn with_overrides(
        paths: &AppPaths,
        source: configuration::Source,
        overrides: crate::storage::Values,
    ) -> Result<Self, String> {
        Self::compose_source(
            paths,
            source,
            true,
            Composition {
                overrides: Some(overrides),
                ..Default::default()
            },
        )
        .await
    }
    pub(super) async fn with_flow(
        paths: &AppPaths,
        source: configuration::Source,
        kind: crate::preferences::SettingsKind,
        values: crate::storage::Values,
    ) -> Result<Self, String> {
        Self::compose_source(
            paths,
            source,
            true,
            Composition {
                flow: Some((kind, values)),
                ..Default::default()
            },
        )
        .await
    }
    pub(super) async fn reuse_control(&mut self, previous: &Self) -> Result<(), String> {
        let old: Value = serde_yaml_ng::from_slice(
            &tokio::fs::read(&previous.config)
                .await
                .map_err(|_| "无法读取运行配置")?,
        )
        .map_err(|_| "运行配置无效")?;
        let mut candidate: Value = serde_yaml_ng::from_slice(
            &tokio::fs::read(&self.config)
                .await
                .map_err(|_| "无法读取候选配置")?,
        )
        .map_err(|_| "候选配置无效")?;
        for key in ["external-controller-unix", "external-controller-pipe"] {
            candidate
                .as_mapping_mut()
                .ok_or("候选配置无效")?
                .remove(key);
            if let Some(value) = old.get(key) {
                candidate[key] = value.clone();
            }
        }
        let yaml = serde_yaml_ng::to_string(&candidate).map_err(|_| "无法编码热更新配置")?;
        crate::storage::atomic_write(&self.config, yaml.as_bytes())
            .map_err(|_| "无法写入热更新配置")?;
        self.force_service = previous.force_service;
        self.client = previous.client.clone();
        Ok(())
    }
    pub(super) async fn yaml(&self) -> Result<String, String> {
        tokio::fs::read_to_string(&self.config)
            .await
            .map_err(|_| "无法读取私有运行配置".into())
    }
    pub(super) fn record_mode(&mut self, mode: crate::session::ProxyMode) -> Result<(), String> {
        let mut yaml: Value = serde_yaml_ng::from_slice(
            &std::fs::read(&self.config).map_err(|_| "无法读取私有运行配置")?,
        )
        .map_err(|_| "私有运行配置无效")?;
        yaml["mode"] = mode.label().into();
        let text = serde_yaml_ng::to_string(&yaml).map_err(|_| "无法编码运行模式")?;
        let mut composed: Value =
            serde_yaml_ng::from_str(&self.composed).map_err(|_| "已应用配置无效")?;
        composed["mode"] = mode.label().into();
        let composed = serde_yaml_ng::to_string(&composed).map_err(|_| "无法记录运行模式")?;
        crate::storage::atomic_write(&self.config, text.as_bytes())
            .map_err(|_| "运行模式文件保存失败")?;
        self.composed = composed;
        Ok(())
    }
    pub(super) fn commit_hot(&mut self, candidate: &Self, yaml: &str) -> Result<(), String> {
        crate::storage::atomic_write(&self.config, yaml.as_bytes())
            .map_err(|_| "无法提交私有运行配置")?;
        self.source = candidate.source.clone();
        self.composed = candidate.composed.clone();
        self.client = self.client.clone().with_proxy_config(
            &serde_yaml_ng::from_str(&self.composed).map_err(|_| "已应用配置无效")?,
        );
        Ok(())
    }
    pub(super) async fn for_install(
        paths: &AppPaths,
        source: configuration::Source,
        executable: PathBuf,
    ) -> Result<Self, String> {
        Self::compose_source(
            paths,
            source,
            true,
            Composition {
                executable: Some(executable),
                ..Default::default()
            },
        )
        .await
    }

    pub(super) async fn for_database_validation(
        paths: &AppPaths,
        directory: PathBuf,
        dat: bool,
        rule: String,
    ) -> Result<Self, String> {
        let values = crate::storage::Values::from([
            ("MixedPort".into(), "0".into()),
            ("HttpPort".into(), "0".into()),
            ("SocksPort".into(), "0".into()),
            ("GeoMode".into(), if dat { "dat" } else { "db" }.into()),
        ]);
        let preferences = configuration::Preferences::from_values(&values, None)?;
        let mut launch = Self::compose_source(
            paths,
            configuration::Source::basic(paths)?,
            false,
            Composition {
                preferences: Some(preferences),
                ..Default::default()
            },
        )
        .await?;
        let mut config: Value =
            serde_yaml_ng::from_str(&launch.yaml().await?).map_err(|_| "资源校验配置无效")?;
        config["rules"] = serde_yaml_ng::to_value(vec![rule, "MATCH,DIRECT".into()])
            .map_err(|_| "资源校验规则无效")?;
        let yaml = serde_yaml_ng::to_string(&config).map_err(|_| "资源校验配置无效")?;
        crate::storage::atomic_write(&launch.config, yaml.as_bytes())
            .map_err(|_| "无法写入资源校验配置")?;
        launch.work_dir = directory;
        Ok(launch)
    }
    async fn compose_source(
        paths: &AppPaths,
        source: configuration::Source,
        apply_overrides: bool,
        options: Composition,
    ) -> Result<Self, String> {
        let work_dir = paths.directory(DirectoryKind::Core);
        let executable = if let Some(executable) = options.executable {
            executable
        } else {
            crate::bootstrap::require_core(paths)?;
            work_dir.join(crate::bootstrap::core_name())
        };
        // Metadata supplies order and scope; current body files are the sole
        // content source. All I/O runs on the existing lifecycle worker.
        let repository = Repository::open(paths.clone()).map_err(|_| "无法打开配置来源存储")?;
        let overrides = if let Some(overrides) = options.overrides {
            overrides
        } else if apply_overrides {
            crate::profiles::override_repository::read(paths)?.0
        } else {
            Default::default()
        };
        let enhanced = if let Some(id) = source.profile().map(|profile| profile.id()) {
            let values = match options.enhancement {
                Some(values) => values,
                None => repository
                    .read_limited(
                        &Namespace::parse(&format!("profile-enhance-{id}"))
                            .map_err(|_| "订阅增强身份无效")?,
                        SOURCE_LIMIT,
                    )
                    .map_err(|_| "无法读取订阅增强")?,
            };
            configuration::enhancement::apply(source.content(), &values)?
        } else {
            source.content().to_vec()
        };
        // Global DNS supplies the base; ordered user overrides can specialize it.
        let enhanced: Value = serde_yaml_ng::from_slice(&enhanced).map_err(|_| "订阅配置无效")?;
        let mut base = match enhanced {
            Value::Mapping(mapping) => configuration::lowercase_root(mapping),
            _ => return Err("订阅配置需为对象".into()),
        };
        configuration::flow::apply_dns(paths, options.flow.as_ref(), &mut base)?;
        let enhanced = serde_yaml_ng::to_string(&base)
            .map_err(|_| "无法编码 DNS 基础配置")?
            .into_bytes();
        struct Cancellation(std::sync::Arc<std::sync::atomic::AtomicBool>);
        impl Drop for Cancellation {
            fn drop(&mut self) {
                self.0.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }
        let cancellation = Cancellation(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(
            false,
        )));
        let token = cancellation.0.clone();
        let profile = source.profile().map(|profile| profile.id());
        let name = source
            .profile()
            .map(|profile| profile.name().to_owned())
            .unwrap_or_default();
        let (result, reports) = tokio::task::spawn_blocking(move || {
            let mut reports = vec![];
            let result = configuration::compose_reporting(
                &enhanced,
                profile,
                &name,
                &overrides,
                Some(token),
                &mut |report| reports.push(report),
            );
            (result, reports)
        })
        .await
        .map_err(|_| "配置合成任务异常退出")?;
        let mut log_errors = vec![];
        for report in reports {
            if let Err(error) = super::overrides::save_report(paths, &report) {
                log_errors.push(error);
            }
        }
        let bytes = match result {
            Ok(bytes) if log_errors.is_empty() => bytes,
            Ok(_) => return Err(crate::failure::join(&log_errors)),
            Err(error) => {
                let mut errors = vec![error.to_string()];
                errors.extend(log_errors);
                return Err(crate::failure::join(&errors));
            }
        };
        let preferences = match options.preferences {
            Some(preferences) => preferences,
            None => configuration::Preferences::load(paths)?,
        };
        let mut mapped: Value = serde_yaml_ng::from_slice(&bytes).map_err(|_| "合成结果无效")?;
        preferences.overlay(mapped.as_mapping_mut().ok_or("合成结果必须为对象")?);
        configuration::flow::apply_settings(
            paths,
            options.flow.as_ref(),
            mapped.as_mapping_mut().ok_or("合成结果必须为对象")?,
        )?;
        configuration::tun::prepare(mapped.as_mapping_mut().ok_or("合成结果必须为对象")?)?;
        let chain = match options.chain {
            Some(chain) => chain,
            None => crate::proxies::chain_config::Config::load(paths)?,
        };
        chain.apply_from(
            mapped.as_mapping_mut().ok_or("合成结果必须为对象")?,
            &work_dir,
        )?;
        super::runtime_bundle::resolve_provider_path_conflicts(
            mapped.as_mapping_mut().ok_or("运行配置无效")?,
            &work_dir,
        )
        .map_err(|error| error.to_string())?;
        let composed = serde_yaml_ng::to_string(&mapped).map_err(|_| "无法合成应用内核设置")?;
        let directory = tempfile::Builder::new()
            .prefix("run-")
            .tempdir_in(&work_dir)
            .map_err(|error| format!("无法创建内核运行目录：{}", error.kind()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))
                .map_err(|error| format!("无法保护内核本地通道：{}", error.kind()))?;
        }
        #[cfg(unix)]
        let (endpoint, key, address) = {
            let socket = directory.path().join("api.sock");
            // macOS sockaddr_un has the tightest path bound among supported Unix targets.
            use std::os::unix::ffi::OsStrExt as _;
            if socket.as_os_str().as_bytes().len() >= 104 {
                return Err("应用目录路径过长，无法创建内核 Unix Socket".into());
            }
            let address = socket
                .to_str()
                .ok_or("本地通道路径不是有效 UTF-8")?
                .to_owned();
            (Endpoint::Unix(socket), "external-controller-unix", address)
        };
        #[cfg(windows)]
        let (endpoint, key, address) = {
            let address = format!(
                r"\\.\pipe\suki-clash-{}",
                random_token().map_err(|_| "生成管道身份失败")?
            );
            (
                Endpoint::Pipe(address.clone()),
                "external-controller-pipe",
                address,
            )
        };
        let secret = random_token().map_err(|_| "生成本地控制凭据失败")?;
        let yaml = protected_with_preferences(
            composed.as_bytes(),
            key,
            &address,
            &secret,
            Some(preferences.native()),
        )?;
        let config = directory.path().join("candidate.yaml");
        // This runs on the application's core worker, never on GPUI's UI thread.
        crate::storage::atomic_write(&config, yaml.as_bytes()).map_err(|_| "写入候选配置失败")?;
        Ok(Self {
            force_service: false,
            executable,
            work_dir,
            config,
            source,
            client: Client::new(endpoint, secret).with_proxy_config(&mapped),
            composed,
            _directory: directory,
            #[cfg(windows)]
            pipe_sddl: current_user_pipe_sddl().map_err(|_| "无法限制内核命名管道的访问权限")?,
        })
    }

    pub(super) fn command(&self, validation: bool) -> Command {
        let mut command = Command::new(&self.executable);
        command
            .arg("-d")
            .arg(&self.work_dir)
            .arg("-f")
            .arg(&self.config)
            .current_dir(&self.work_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        // An inherited override must not redirect the controller or config.
        for name in [
            "CLASH_HOME_DIR",
            "CLASH_CONFIG_FILE",
            "CLASH_OVERRIDE_EXTERNAL_CONTROLLER",
            "CLASH_OVERRIDE_EXTERNAL_CONTROLLER_UNIX",
            "CLASH_OVERRIDE_EXTERNAL_CONTROLLER_PIPE",
            "CLASH_OVERRIDE_SECRET",
        ] {
            command.env_remove(name);
        }
        #[cfg(windows)]
        {
            command
                .env("LISTEN_NAMEDPIPE_SDDL", &self.pipe_sddl)
                .creation_flags(0x08000000);
        }
        if validation {
            command.arg("-t");
        }
        command
    }

    #[cfg(test)]
    pub(super) fn candidate_path(&self) -> &std::path::Path {
        &self.config
    }
}

#[cfg(test)]
fn protected_config(
    bytes: &[u8],
    endpoint_key: &str,
    endpoint: &str,
    secret: &str,
) -> Result<String, String> {
    protected_with_preferences(bytes, endpoint_key, endpoint, secret, None)
}

fn protected_with_preferences(
    bytes: &[u8],
    endpoint_key: &str,
    endpoint: &str,
    secret: &str,
    owned: Option<&serde_yaml_ng::Mapping>,
) -> Result<String, String> {
    let value: Value = serde_yaml_ng::from_slice(bytes).map_err(|_| "运行配置不是有效 YAML")?;
    let Value::Mapping(mut config) = value else {
        return Err("运行配置顶层必须是对象".into());
    };
    for (key, label) in [("tun", "TUN"), ("dns", "DNS")] {
        if let Some(value) = config.get(key) {
            let value = value
                .as_mapping()
                .ok_or_else(|| format!("{label} 配置必须是对象"))?;
            if let Some(enabled) = value.get("enable") {
                match enabled.as_bool() {
                    Some(false) => {}
                    Some(true) => {}
                    None => return Err(format!("{label} enable 必须是布尔值")),
                }
            }
        }
    }
    // Fail closed for platform integrations that this owner cannot restore.
    for key in ["redir-port", "tproxy-port", "routing-mark"] {
        if owned.is_some()
            && ((key == "redir-port" && cfg!(unix))
                || (matches!(key, "tproxy-port" | "routing-mark") && cfg!(target_os = "linux")))
        {
            continue;
        }
        if config
            .get(key)
            .is_some_and(|value| value.as_u64() != Some(0))
        {
            return Err(format!("{key} 尚未接入平台恢复流程"));
        }
    }
    if config.contains_key("iptables") {
        return Err("iptables 不是内核配置字段；自动路由请使用虚拟网卡自动路由".into());
    }
    for key in [
        "external-controller",
        "external-controller-tls",
        "external-controller-unix",
        "external-controller-pipe",
        "external-controller-cors",
        "external-ui",
        "external-ui-url",
        "external-ui-name",
        "external-doh-server",
        "secret",
    ] {
        config.remove(Value::String(key.into()));
    }
    config.insert(
        Value::String(endpoint_key.into()),
        Value::String(endpoint.into()),
    );
    let external = owned
        .and_then(|settings| settings.get("external-controller"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    let secret = if external.is_some() {
        owned
            .and_then(|settings| settings.get("secret"))
            .and_then(Value::as_str)
            .ok_or("外部控制缺少密钥")?
    } else {
        secret
    };
    config.insert(Value::String("secret".into()), Value::String(secret.into()));
    if let Some(address) = external {
        config.insert("external-controller".into(), address.into());
        if let Some(cors) = owned.and_then(|settings| settings.get("external-controller-cors")) {
            config.insert("external-controller-cors".into(), cors.clone());
        }
    }
    let lan = owned
        .and_then(|settings| settings.get("allow-lan"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    config.insert(Value::String("allow-lan".into()), Value::Bool(lan));
    configuration::ensure_lan_bind_address(&mut config);
    serde_yaml_ng::to_string(&config).map_err(|_| "无法生成候选配置".into())
}

#[cfg(windows)]
fn current_user_pipe_sddl() -> io::Result<String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, LocalFree},
        Security::{
            Authorization::ConvertSidToStringSidW, GetTokenInformation, TOKEN_QUERY, TOKEN_USER,
            TokenUser,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut length = 0;
        GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut length);
        // usize storage keeps TOKEN_USER aligned.
        let mut buffer = vec![0usize; (length as usize).div_ceil(std::mem::size_of::<usize>())];
        let success = GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            length,
            &mut length,
        );
        let error = io::Error::last_os_error();
        CloseHandle(token);
        if success == 0 {
            return Err(error);
        }
        let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        let mut text = std::ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut text) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut length = 0;
        while *text.add(length) != 0 {
            length += 1;
        }
        let sid = String::from_utf16(std::slice::from_raw_parts(text, length));
        LocalFree(text.cast());
        let sid = sid.map_err(io::Error::other)?;
        Ok(format!("D:P(A;;GA;;;{sid})"))
    }
}

#[cfg(test)]
mod tests {
    use super::protected_config;
    #[test]
    fn final_control_protection_keeps_custom_bindings() {
        for allow_lan in [false, true] {
            for binding in ["192.168.1.2", "2001:db8::10"] {
                let owned =
                    serde_yaml_ng::Mapping::from_iter([("allow-lan".into(), allow_lan.into())]);
                let output = super::protected_with_preferences(
                    format!("bind-address: '{binding}'").as_bytes(),
                    "external-controller-unix",
                    "/api.sock",
                    "secret",
                    Some(&owned),
                )
                .unwrap();
                let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(&output).unwrap();
                assert_eq!(value["bind-address"], binding);
            }
        }
    }

    #[test]
    fn private_controller_is_final_and_unknown_fields_are_preserved() {
        let source = b"external-controller: 0.0.0.0:9090\nsecret: source-secret\ncustom-key:\n  MixedCase: preserved\nproxies: []\nrules: ['MATCH,DIRECT']\n";
        let output = protected_config(
            source,
            "external-controller-unix",
            "/private/api.sock",
            "new-secret",
        )
        .unwrap();
        let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(&output).unwrap();
        assert!(value.get("external-controller").is_none());
        assert_eq!(value["external-controller-unix"], "/private/api.sock");
        assert_eq!(value["custom-key"]["MixedCase"], "preserved");
        assert_eq!(value["rules"][0], "MATCH,DIRECT");
        assert!(!output.contains("source-secret"));
        assert_eq!(value["allow-lan"], false);
    }
    #[test]
    fn unsupported_network_changes_fail_instead_of_being_silently_disabled() {
        for source in ["redir-port: 1234", "[]"] {
            assert!(
                protected_config(
                    source.as_bytes(),
                    "external-controller-unix",
                    "/api.sock",
                    "secret"
                )
                .is_err()
            );
        }
        let output = protected_config(
            b"listeners: []",
            "external-controller-unix",
            "/api.sock",
            "secret",
        )
        .unwrap();
        let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(&output).unwrap();
        assert_eq!(
            value["listeners"],
            serde_yaml_ng::Value::Sequence(Vec::new())
        );
    }
}
