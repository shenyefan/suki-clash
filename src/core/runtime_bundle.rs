// Adapted from CV 2.5.9 runtime_bundle.rs (GPL-3.0); see platform-service/LICENSE.
use anyhow::{Context as _, Result, bail};

use serde_yaml_ng::{Mapping, Value};
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::path::{Component, Path, PathBuf};
use suki_service::{RemoteProvider, RuntimeAsset, RuntimeBundle};

pub(crate) use suki_service::RUNTIME_GEO_ASSETS as GEO_ASSETS;

/// Provider identity used by the runtime config and core API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemoteProviderRef {
    pub section: &'static str,
    pub name: String,
    pub provider: RemoteProvider,
}

struct RuntimeParts {
    config: Value,
    assets: Vec<RuntimeAsset>,
    remote_providers: Vec<RemoteProviderRef>,
}

#[cfg(test)]
pub(crate) async fn collect_runtime_bundle(
    config_file: &Path,
    core_path: &Path,
) -> Result<RuntimeBundle> {
    collect_runtime_bundle_from(
        config_file,
        core_path,
        config_file
            .parent()
            .ok_or_else(|| anyhow::anyhow!("配置目录无效"))?,
    )
    .await
}
pub(crate) async fn collect_runtime_bundle_from(
    config_file: &Path,
    core_path: &Path,
    config_root: &Path,
) -> Result<RuntimeBundle> {
    let yaml = tokio::fs::read_to_string(config_file)
        .await
        .with_context(|| format!("failed to read runtime config {config_file:?}"))?;
    let config: Value = serde_yaml_ng::from_str(&yaml)
        .with_context(|| format!("failed to parse runtime config {config_file:?}"))?;
    let mut parts = collect_runtime_parts(config, config_root)?;
    // Private keys belong to service publication, not Provider declaration discovery.
    let mut destinations = parts
        .assets
        .iter()
        .map(|asset| asset.destination.clone())
        .chain(
            parts
                .remote_providers
                .iter()
                .map(|declared| declared.provider.destination.clone()),
        )
        .collect();
    collect_ssh_private_keys(
        &mut parts.config,
        &std::fs::canonicalize(config_root)?,
        &mut destinations,
        &mut parts.assets,
    )?;
    Ok(RuntimeBundle {
        yaml: serde_yaml_ng::to_string(&parts.config)
            .context("failed to serialize service runtime config")?,
        assets: parts.assets,
        remote_providers: parts
            .remote_providers
            .into_iter()
            .map(|declared| declared.provider)
            .collect(),
        core_path: core_path.to_string_lossy().into_owned(),
    })
}

pub(crate) fn remote_providers_of(
    config: &Mapping,
    config_root: &Path,
) -> Result<Vec<RemoteProviderRef>> {
    Ok(collect_runtime_parts(Value::Mapping(config.clone()), config_root)?.remote_providers)
}

#[derive(Default)]
struct ProviderPathOwners {
    remote: BTreeMap<String, Vec<(&'static str, Value)>>,
    local: bool,
}

pub(crate) fn resolve_provider_path_conflicts(
    config: &mut Mapping,
    config_root: &Path,
) -> Result<()> {
    let config_root = std::fs::canonicalize(config_root)?;
    let mut paths = BTreeMap::<String, ProviderPathOwners>::new();
    let mut reserved: HashSet<String> = GEO_ASSETS.iter().map(|name| (*name).to_owned()).collect();
    for section in ["proxy-providers", "rule-providers"] {
        let Some(providers) = config.get(section).and_then(Value::as_mapping) else {
            continue;
        };
        for (name, provider) in providers {
            let Some(raw_path) = provider.get("path").and_then(Value::as_str) else {
                continue;
            };
            let is_remote = provider.get("type").and_then(Value::as_str) == Some("http");
            let destination = if is_remote {
                provider_destination(&config_root, raw_path)
            } else {
                local_provider_source(&config_root, raw_path)
                    .and_then(|source| destination_below_root(&config_root, &source))
                    .or_else(|_| provider_destination(&config_root, raw_path))
            };
            let Ok(destination) = destination else {
                continue;
            };
            reserved.insert(destination.clone());
            let owners = paths.entry(destination).or_default();
            match (is_remote, provider.get("url").and_then(Value::as_str)) {
                (true, Some(url)) => owners
                    .remote
                    .entry(url.to_owned())
                    .or_default()
                    .push((section, name.clone())),
                _ => owners.local = true,
            }
        }
    }

    for (destination, owners) in paths {
        if owners.local || owners.remote.len() < 2 {
            continue;
        }
        // Allocate every source in a conflict group, so declaration order cannot select a cache's owner.
        for (url, providers) in owners.remote {
            let replacement = allocate_provider_destination(&destination, &url, &mut reserved);
            for (section, name) in providers {
                config[section][&name]["path"] = Value::String(replacement.clone());
            }
        }
    }
    Ok(())
}

fn allocate_provider_destination(
    destination: &str,
    url: &str,
    reserved: &mut HashSet<String>,
) -> String {
    let mut digest = Sha256::new();
    digest.update(destination.as_bytes());
    digest.update([0]);
    digest.update(url.as_bytes());
    let digest: String = digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let path = Path::new(destination);
    let extension = path
        .extension()
        .map(|ext| format!(".{}", ext.to_string_lossy()))
        .unwrap_or_default();
    let mut suffix = String::new();
    let mut sequence = 0;
    loop {
        let candidate = path
            .with_file_name(format!("cvr-{digest}{suffix}{extension}"))
            .to_string_lossy()
            .replace('\\', "/");
        if reserved.insert(candidate.clone()) {
            return candidate;
        }
        sequence += 1;
        suffix = format!("-{sequence}");
    }
}

fn collect_runtime_parts(mut config: Value, config_root: &Path) -> Result<RuntimeParts> {
    let config_root = std::fs::canonicalize(config_root)?;
    let mut assets = Vec::new();
    let mut destinations = HashSet::new();
    let mut remote_providers = Vec::new();

    collect_provider_assets(
        &mut config,
        "proxy-providers",
        &config_root,
        &mut destinations,
        &mut assets,
        &mut remote_providers,
    )?;
    collect_provider_assets(
        &mut config,
        "rule-providers",
        &config_root,
        &mut destinations,
        &mut assets,
        &mut remote_providers,
    )?;
    for filename in GEO_ASSETS {
        let source = config_root.join(filename);
        if source.is_file() && destinations.insert((*filename).to_string()) {
            assets.push(RuntimeAsset {
                source: std::fs::canonicalize(&source)?
                    .to_string_lossy()
                    .into_owned(),
                destination: (*filename).to_string(),
            });
        }
    }

    Ok(RuntimeParts {
        config,
        assets,
        remote_providers,
    })
}

/// Rewrites provider paths and records copied assets separately from core-downloaded providers.
/// Remote URLs let the service decide whether a core download cache remains reusable.
fn collect_provider_assets(
    config: &mut Value,
    section: &'static str,
    config_root: &Path,
    destinations: &mut HashSet<String>,
    assets: &mut Vec<RuntimeAsset>,
    remote_providers: &mut Vec<RemoteProviderRef>,
) -> Result<()> {
    let Some(providers) = config
        .as_mapping_mut()
        .and_then(|mapping| mapping.get_mut(section))
        .and_then(Value::as_mapping_mut)
    else {
        return Ok(());
    };

    for (name, provider) in providers.iter_mut() {
        let Some(provider) = provider.as_mapping_mut() else {
            continue;
        };
        let Some(raw_path) = provider.get("path").and_then(Value::as_str) else {
            continue;
        };
        // Match mihomo's `type` classification and leave malformed-but-accepted providers untouched.
        let is_remote = provider.get("type").and_then(Value::as_str) == Some("http");
        let url = provider
            .get("url")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let destination = match (is_remote, url) {
            (true, Some(url)) => {
                let destination = provider_destination(config_root, raw_path)?;
                // Reserve remote destinations too; copied and downloaded files must not collide.
                match remote_providers
                    .iter()
                    .find(|declared| declared.provider.destination == destination)
                {
                    Some(declared) if declared.provider.url == url => {}
                    Some(_) => {
                        bail!(
                            "runtime provider destination {destination:?} is declared for two different sources"
                        )
                    }
                    None => {
                        if !destinations.insert(destination.clone()) {
                            bail!(
                                "runtime provider destination {destination:?} is claimed more than once"
                            );
                        }
                        remote_providers.push(RemoteProviderRef {
                            section,
                            name: name.as_str().unwrap_or_default().to_owned(),
                            provider: RemoteProvider {
                                destination: destination.clone(),
                                url,
                            },
                        });
                    }
                }
                destination
            }
            (true, None) => {
                continue;
            }
            (false, _) => {
                let Ok(source) = local_provider_source(config_root, raw_path) else {
                    continue;
                };
                let destination = destination_below_root(config_root, &source)?;
                if destinations.insert(destination.clone()) {
                    assets.push(RuntimeAsset {
                        source: source.to_string_lossy().into_owned(),
                        destination: destination.clone(),
                    });
                }
                destination
            }
        };
        provider.insert(Value::String("path".to_owned()), Value::String(destination));
    }
    Ok(())
}

/// Mihomo reads SSH file keys relative to its home, including fallback Provider payloads.
fn collect_ssh_private_keys(
    config: &mut Value,
    config_root: &Path,
    destinations: &mut HashSet<String>,
    assets: &mut Vec<RuntimeAsset>,
) -> Result<()> {
    let Some(config) = config.as_mapping_mut() else {
        return Ok(());
    };
    let mut lists = Vec::new();
    for (key, value) in config.iter_mut() {
        match key.as_str() {
            Some("proxies") => lists.extend(value.as_sequence_mut()),
            Some("proxy-providers") => lists.extend(
                value
                    .as_mapping_mut()
                    .into_iter()
                    .flat_map(Mapping::values_mut)
                    .filter_map(|provider| provider.get_mut("payload")?.as_sequence_mut()),
            ),
            _ => {}
        }
    }
    let mut staged = BTreeMap::new();
    for proxy in lists
        .into_iter()
        .flatten()
        .filter_map(Value::as_mapping_mut)
    {
        if proxy.get("type").and_then(Value::as_str) != Some("ssh") {
            continue;
        }
        // This is Mihomo's inline-key classification; do not inspect or rewrite key material.
        let Some(path) = proxy
            .get("private-key")
            .and_then(Value::as_str)
            .filter(|key| !key.is_empty() && !key.contains("PRIVATE KEY"))
        else {
            continue;
        };
        let name = proxy
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        // Resolve only local disk paths: probing UNC/device paths can authenticate to a server.
        if (cfg!(not(windows)) && path.starts_with("\\\\"))
            || matches!(
                Path::new(path).components().next(),
                Some(Component::Prefix(prefix))
                    if !matches!(prefix.kind(), std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_))
            )
        {
            bail!("SSH 节点 {name:?} 的私钥文件需位于本地配置目录");
        }
        let requested = Path::new(path);
        let requested = if requested.is_absolute() {
            requested.to_path_buf()
        } else {
            config_root.join(requested)
        };
        let source = std::fs::canonicalize(&requested)
            .map_err(|error| anyhow::anyhow!("SSH 节点 {name:?} 的私钥文件不可用：{error}"))?;
        if !source.starts_with(config_root) || !source.is_file() {
            bail!("SSH 节点 {name:?} 的私钥需为配置目录内的普通文件");
        }
        let destination = staged.entry(source.clone()).or_insert_with(|| {
            // A root alias avoids service destination restrictions and the inline-key marker.
            let destination = allocate_provider_destination(
                "ssh-private-key",
                &source.to_string_lossy(),
                destinations,
            );
            assets.push(RuntimeAsset {
                source: source.to_string_lossy().into_owned(),
                destination: destination.clone(),
            });
            destination
        });
        proxy.insert("private-key".into(), destination.clone().into());
    }
    Ok(())
}

fn local_provider_source(config_root: &Path, raw_path: &str) -> Result<PathBuf> {
    let path = Path::new(raw_path);
    let source = if path.is_absolute() {
        path.to_path_buf()
    } else {
        config_root.join(path)
    };
    let canonical_source = std::fs::canonicalize(&source)
        .with_context(|| format!("local runtime provider is unavailable: {source:?}"))?;
    canonical_source.strip_prefix(config_root).map_err(|_| {
        anyhow::anyhow!("local runtime provider is outside the config root: {canonical_source:?}")
    })?;
    Ok(canonical_source)
}

fn provider_destination(config_root: &Path, raw_path: &str) -> Result<String> {
    let path = Path::new(raw_path);
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        bail!("runtime provider destination traverses outside the runtime");
    }
    if path.is_absolute() {
        let normalized = canonicalize_with_missing_tail(path)?;
        destination_below_root(config_root, &normalized)
    } else {
        normalized_destination(path)
    }
}

fn canonicalize_with_missing_tail(path: &Path) -> Result<PathBuf> {
    let mut ancestor = path;
    let mut tail = Vec::new();
    while !ancestor.exists() {
        let name = ancestor.file_name().ok_or_else(|| {
            anyhow::anyhow!("runtime provider path has no existing ancestor: {path:?}")
        })?;
        tail.push(name.to_owned());
        ancestor = ancestor.parent().ok_or_else(|| {
            anyhow::anyhow!("runtime provider path has no existing ancestor: {path:?}")
        })?;
    }
    let mut normalized = std::fs::canonicalize(ancestor)?;
    for component in tail.iter().rev() {
        normalized.push(component);
    }
    Ok(normalized)
}

fn destination_below_root(config_root: &Path, path: &Path) -> Result<String> {
    let relative = path.strip_prefix(config_root).map_err(|_| {
        anyhow::anyhow!("runtime provider path is outside the config root: {path:?}")
    })?;
    normalized_destination(relative)
}

fn normalized_destination(relative: &Path) -> Result<String> {
    let mut destination = PathBuf::new();
    for component in relative.components() {
        match component {
            Component::Normal(component) => destination.push(component),
            Component::CurDir => {}
            _ => bail!("local runtime provider destination traverses outside the runtime"),
        }
    }
    if destination.as_os_str().is_empty() {
        bail!("local runtime provider destination is empty");
    }
    Ok(destination.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use super::{collect_runtime_bundle, remote_providers_of, resolve_provider_path_conflicts};

    #[tokio::test]
    async fn ssh_keys_are_staged_once_across_nodes_and_provider_payloads() -> anyhow::Result<()> {
        let temporary = tempfile::tempdir()?;
        let root = std::fs::canonicalize(temporary.path())?;
        std::fs::create_dir(root.join("keys"))?;
        let key = root.join("keys/client.pem");
        std::fs::write(&key, "private test material")?;
        let config = root.join("config.yaml");
        let source = format!(
            "proxies:\n  - {{ name: relative, type: ssh, private-key: keys/client.pem }}\n  - {{ name: absolute, type: ssh, private-key: {} }}\n  - {{ name: inline, type: ssh, private-key: '-----BEGIN OPENSSH PRIVATE KEY-----' }}\n  - {{ name: password, type: ssh, password: unchanged }}\n  - {{ name: other, type: ss, private-key: ignored }}\nproxy-providers:\n  inline:\n    type: inline\n    payload:\n      - {{ name: inline-payload, type: ssh, private-key: ./keys/client.pem }}\n  remote:\n    type: http\n    url: https://example.test/nodes.yaml\n    path: providers/nodes.yaml\n    payload:\n      - {{ name: fallback, type: ssh, private-key: keys/client.pem }}\n  local:\n    type: file\n    path: providers/local.yaml\n    payload:\n      - {{ name: local-payload, type: ssh, private-key: keys/client.pem }}\n",
            key.display()
        );
        std::fs::write(&config, &source)?;
        let bundle = collect_runtime_bundle(&config, &root.join("mihomo")).await?;
        assert_eq!(bundle.assets.len(), 1, "shared keys use one staged asset");
        let asset = &bundle.assets[0];
        assert_eq!(asset.source, key.to_string_lossy());
        assert!(!asset.destination.contains('/'));
        assert!(!asset.destination.contains("PRIVATE KEY"));
        let yaml: serde_yaml_ng::Value = serde_yaml_ng::from_str(&bundle.yaml)?;
        for value in [
            &yaml["proxies"][0],
            &yaml["proxies"][1],
            &yaml["proxy-providers"]["inline"]["payload"][0],
            &yaml["proxy-providers"]["remote"]["payload"][0],
            &yaml["proxy-providers"]["local"]["payload"][0],
        ] {
            assert_eq!(value["private-key"], asset.destination);
        }
        assert_eq!(
            yaml["proxies"][2]["private-key"],
            "-----BEGIN OPENSSH PRIVATE KEY-----"
        );
        assert_eq!(yaml["proxies"][3]["password"], "unchanged");
        assert_eq!(yaml["proxies"][4]["private-key"], "ignored");
        assert_eq!(
            std::fs::read_to_string(config)?,
            source,
            "source stays intact"
        );
        Ok(())
    }

    #[tokio::test]
    async fn ssh_file_errors_stop_publication_but_not_provider_discovery() -> anyhow::Result<()> {
        let temporary = tempfile::tempdir()?;
        let root = std::fs::canonicalize(temporary.path())?;
        let outside = tempfile::tempdir()?;
        let outside_key = outside.path().join("private.pem");
        std::fs::write(&outside_key, "never include this private content in errors")?;
        let config = root.join("config.yaml");
        for path in [
            "missing.pem".to_owned(),
            ".".to_owned(),
            outside_key.to_string_lossy().into_owned(),
            "\\\\server\\private.pem".to_owned(),
        ] {
            let source = format!(
                "proxies:\n  - name: ssh\n    type: ssh\n    private-key: {}\nproxy-providers:\n  remote:\n    type: http\n    url: https://example.test/nodes.yaml\n    path: providers/nodes.yaml\n",
                serde_json::to_string(&path)?
            );
            std::fs::write(&config, &source)?;
            let error = collect_runtime_bundle(&config, &root.join("mihomo"))
                .await
                .err()
                .ok_or_else(|| anyhow::anyhow!("invalid private key file was accepted"))?;
            assert!(error.to_string().contains("私钥"));
            assert!(
                !error
                    .to_string()
                    .contains("never include this private content")
            );
            let mapping = serde_yaml_ng::from_str(&source)?;
            assert_eq!(remote_providers_of(&mapping, &root)?.len(), 1);
            assert_eq!(std::fs::read_to_string(&config)?, source);
        }
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn ssh_symlink_cannot_escape_the_config_root() -> anyhow::Result<()> {
        let temporary = tempfile::tempdir()?;
        let outside = tempfile::tempdir()?;
        let key = outside.path().join("private.pem");
        std::fs::write(&key, "private")?;
        std::os::unix::fs::symlink(key, temporary.path().join("private.pem"))?;
        let config = temporary.path().join("config.yaml");
        std::fs::write(
            &config,
            "proxies: [{name: ssh, type: ssh, private-key: private.pem}]\n",
        )?;
        let error = collect_runtime_bundle(&config, &temporary.path().join("mihomo"))
            .await
            .err()
            .ok_or_else(|| anyhow::anyhow!("escaping private-key symlink was accepted"))?;
        assert!(error.to_string().contains("配置目录内"));
        Ok(())
    }

    #[tokio::test]
    async fn conflicting_sources_get_separate_paths_in_config_bundle_and_readback()
    -> anyhow::Result<()> {
        let root =
            std::env::temp_dir().join(format!("clash-verge-provider-paths-{}", std::process::id()));
        std::fs::create_dir_all(&root)?;
        let mut config: serde_yaml_ng::Mapping = serde_yaml_ng::from_str(
            "rule-providers:\n  AppleDev:\n    type: http\n    url: https://one.example/AppleDev.yaml\n    path: ./providers/rule/AppleDev.yaml\n  mirror:\n    type: http\n    url: https://two.example/AppleDev.yaml\n    path: providers/rule/AppleDev.yaml\n",
        )?;
        let original = config.clone();
        let root = std::fs::canonicalize(root)?;
        let mut reordered = config.clone();
        reordered["rule-providers"] = serde_yaml_ng::from_str(&format!(
            "mirror:\n  type: http\n  url: https://two.example/AppleDev.yaml\n  path: {}\nAppleDev:\n  type: http\n  url: https://one.example/AppleDev.yaml\n  path: providers/rule/AppleDev.yaml\n",
            root.join("providers/rule/AppleDev.yaml").display(),
        ))?;

        resolve_provider_path_conflicts(&mut config, &root)?;
        let config_file = root.join("config.yaml");
        std::fs::write(&config_file, serde_yaml_ng::to_string(&config)?)?;
        let bundle = collect_runtime_bundle(&config_file, &root.join("mihomo")).await?;
        let paths = &config["rule-providers"];
        assert_ne!(paths["AppleDev"]["path"], paths["mirror"]["path"]);
        resolve_provider_path_conflicts(&mut reordered, &root)?;
        assert_eq!(
            config, reordered,
            "path allocation must not depend on declaration order or path spelling"
        );
        let resolved = config.clone();
        resolve_provider_path_conflicts(&mut config, &root)?;
        assert_eq!(
            config, resolved,
            "resolving an already generated config must be idempotent"
        );

        assert_eq!(
            serde_yaml_ng::from_str::<serde_yaml_ng::Mapping>(&bundle.yaml)?,
            config
        );
        let readback = remote_providers_of(&config, &root)?;
        assert_eq!(bundle.remote_providers.len(), 2);
        for declared in readback {
            assert!(bundle.remote_providers.contains(&declared.provider));
            assert_eq!(
                config[declared.section][&declared.name]["path"],
                declared.provider.destination
            );
            assert_eq!(
                config[declared.section][&declared.name]["url"],
                original[declared.section][&declared.name]["url"]
            );
        }
        std::fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn allocated_provider_paths_do_not_claim_another_declared_path() -> anyhow::Result<()> {
        let root = std::env::temp_dir();
        let original: serde_yaml_ng::Mapping = serde_yaml_ng::from_str(
            "proxy-providers:\n  a:\n    type: http\n    url: https://one.example/p.yaml\n    path: ./providers/shared.yaml\nrule-providers:\n  b:\n    type: http\n    url: https://two.example/r.yaml\n    path: ./providers/shared.yaml\n",
        )?;
        let mut config = original.clone();
        resolve_provider_path_conflicts(&mut config, &root)?;
        let allocated = config["proxy-providers"]["a"]["path"].clone();
        let mut config = original;
        config["rule-providers"]["reserved"] =
            serde_yaml_ng::from_str("type: file\npath: placeholder")?;
        config["rule-providers"]["reserved"]["path"] = allocated.clone();
        resolve_provider_path_conflicts(&mut config, &root)?;
        assert_ne!(config["proxy-providers"]["a"]["path"], allocated);
        assert_ne!(
            config["proxy-providers"]["a"]["path"],
            config["rule-providers"]["b"]["path"]
        );
        assert_eq!(config["rule-providers"]["reserved"]["path"], allocated);
        Ok(())
    }

    #[test]
    fn provider_path_resolution_leaves_shared_sources_and_local_files_unchanged()
    -> anyhow::Result<()> {
        let mut config: serde_yaml_ng::Mapping = serde_yaml_ng::from_str(
            "rule-providers:\n  a:\n    type: http\n    url: https://one.example/r.yaml\n    path: ./rules/shared.yaml\n  b:\n    type: http\n    url: https://one.example/r.yaml\n    path: ./rules/shared.yaml\n  local:\n    type: file\n    path: ./rules/local.yaml\n  remote:\n    type: http\n    url: https://two.example/r.yaml\n    path: ./rules/local.yaml\n  mirror:\n    type: http\n    url: https://three.example/r.yaml\n    path: ./rules/local.yaml\n",
        )?;
        let original = config.clone();
        resolve_provider_path_conflicts(&mut config, &std::env::temp_dir())?;
        assert_eq!(config, original);
        Ok(())
    }

    #[tokio::test]
    async fn collects_only_local_providers_and_existing_geo_assets() -> anyhow::Result<()> {
        let root =
            std::env::temp_dir().join(format!("clash-verge-runtime-bundle-{}", std::process::id()));
        std::fs::create_dir_all(root.join("providers"))?;
        std::fs::write(root.join("providers/local.yaml"), b"proxies: []\n")?;
        std::fs::write(root.join("Country.mmdb"), b"geo")?;
        let config = root.join("config.yaml");
        let absolute_local = root.join("providers/local.yaml");
        let absolute_remote = root.join("providers/remote.yaml");
        std::fs::write(
            &config,
            format!(
                "proxy-providers:\n  local:\n    type: file\n    path: {}\n  remote:\n    type: http\n    url: https://example.com/p.yaml\n    path: {}\nrule-providers: {{}}\n",
                absolute_local.display(),
                absolute_remote.display()
            ),
        )?;
        let core = root.join("mihomo");
        std::fs::write(&core, b"core")?;

        let bundle = collect_runtime_bundle(&config, &core).await?;

        let bundled_config: serde_yaml_ng::Value = serde_yaml_ng::from_str(&bundle.yaml)?;
        assert_eq!(
            bundled_config["proxy-providers"]["local"]["path"].as_str(),
            Some("providers/local.yaml")
        );
        assert_eq!(
            bundled_config["proxy-providers"]["remote"]["path"].as_str(),
            Some("providers/remote.yaml")
        );
        assert!(!bundle.yaml.contains(&root.to_string_lossy().into_owned()));
        assert!(
            bundle
                .assets
                .iter()
                .any(|asset| asset.destination == "providers/local.yaml")
        );
        assert!(
            bundle
                .assets
                .iter()
                .any(|asset| asset.destination == "Country.mmdb")
        );
        assert!(
            !bundle
                .assets
                .iter()
                .any(|asset| asset.destination.contains("remote"))
        );
        // Remote declarations preserve service-side cache reuse.
        assert_eq!(
            bundle
                .remote_providers
                .iter()
                .map(|provider| (provider.destination.as_str(), provider.url.as_str()))
                .collect::<Vec<_>>(),
            [("providers/remote.yaml", "https://example.com/p.yaml")],
            "the declaration must pair the rewritten destination with the url, not the original path"
        );
        assert!(
            !bundle
                .assets
                .iter()
                .any(|asset| asset.destination.contains("geoip.dat"))
        );
        std::fs::remove_dir_all(root)?;
        Ok(())
    }

    #[tokio::test]
    async fn two_names_for_one_remote_file_are_folded_rather_than_refused() -> anyhow::Result<()> {
        // Identical repeats are one service-side file and are safe to fold.
        let root =
            std::env::temp_dir().join(format!("clash-verge-bundle-dup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root)?;
        let config = root.join("config.yaml");
        std::fs::write(
            &config,
            "rule-providers:\n  a:\n    type: http\n    url: https://one.example/x.yaml\n    path: ./rules/x.yaml\n  b:\n    type: http\n    url: https://one.example/x.yaml\n    path: ./rules/x.yaml\n",
        )?;
        let core = root.join("mihomo");
        std::fs::write(&core, b"core")?;

        let bundle = collect_runtime_bundle(&config, &core).await?;

        assert_eq!(bundle.remote_providers.len(), 1, "one file, declared twice");
        assert_eq!(bundle.remote_providers[0].destination, "rules/x.yaml");
        std::fs::remove_dir_all(root)?;
        Ok(())
    }

    #[tokio::test]
    async fn two_sources_for_one_remote_file_are_refused() -> anyhow::Result<()> {
        let root = std::env::temp_dir().join(format!(
            "clash-verge-bundle-conflict-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root)?;
        let config = root.join("config.yaml");
        std::fs::write(
            &config,
            "rule-providers:\n  a:\n    type: http\n    url: https://one.example/x.yaml\n    path: ./rules/x.yaml\n  b:\n    type: http\n    url: https://two.example/x.yaml\n    path: ./rules/x.yaml\n",
        )?;
        let core = root.join("mihomo");
        std::fs::write(&core, b"core")?;

        let Err(error) = collect_runtime_bundle(&config, &core).await else {
            anyhow::bail!("two sources cannot own one file");
        };

        assert!(
            error.to_string().contains("two different sources"),
            "{error}"
        );
        std::fs::remove_dir_all(root)?;
        Ok(())
    }

    #[tokio::test]
    async fn an_odd_provider_is_left_alone_instead_of_failing_the_whole_bundle()
    -> anyhow::Result<()> {
        // Odd providers accepted by mihomo must not block the rest of the bundle.
        let root =
            std::env::temp_dir().join(format!("clash-verge-bundle-odd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root)?;
        let config = root.join("config.yaml");
        std::fs::write(
            &config,
            "rule-providers:\n  nourl:\n    type: http\n    path: ./rules/nourl.yaml\n  missing:\n    type: inline\n    url: https://one.example/i.yaml\n    path: ./rules/missing.yaml\n  good:\n    type: http\n    url: https://one.example/g.yaml\n    path: ./rules/g.yaml\n",
        )?;
        let core = root.join("mihomo");
        std::fs::write(&core, b"core")?;

        let bundle = collect_runtime_bundle(&config, &core).await?;

        assert_eq!(
            bundle.remote_providers.len(),
            1,
            "only the provider that could be classified is declared"
        );
        assert_eq!(bundle.remote_providers[0].destination, "rules/g.yaml");
        let reserialized: serde_yaml_ng::Value = serde_yaml_ng::from_str(&bundle.yaml)?;
        assert_eq!(
            reserialized["rule-providers"]["nourl"]["path"].as_str(),
            Some("./rules/nourl.yaml"),
            "an unclassifiable provider keeps the path the core was given"
        );
        std::fs::remove_dir_all(root)?;
        Ok(())
    }

    #[tokio::test]
    async fn rejects_provider_paths_outside_the_config_root() -> anyhow::Result<()> {
        let root = std::env::temp_dir().join(format!(
            "clash-verge-runtime-bundle-outside-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root)?;
        let config = root.join("config.yaml");
        let outside = root
            .parent()
            .ok_or_else(|| anyhow::anyhow!("test root has no parent"))?
            .join(format!("outside-{}.yaml", std::process::id()));
        std::fs::write(
            &config,
            format!(
                "proxy-providers:\n  remote:\n    type: http\n    url: https://example.com/p.yaml\n    path: {}\n",
                outside.display()
            ),
        )?;
        let core = root.join("mihomo");
        std::fs::write(&core, b"core")?;

        let Err(error) = collect_runtime_bundle(&config, &core).await else {
            anyhow::bail!("outside provider path must be rejected");
        };

        assert!(error.to_string().contains("outside the config root"));
        std::fs::remove_dir_all(root)?;
        Ok(())
    }
}
