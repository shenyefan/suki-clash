//! Pure configuration composition. The lifecycle owner supplies an immutable
//! source and one committed override index; this module performs no I/O or GPUI work.
pub(crate) mod tun;
use crate::storage::Values;
use serde_yaml_ng::{Mapping, Value};
use std::collections::BTreeSet;
pub(crate) mod enhancement;
pub(crate) mod flow;
mod preferences;
pub(crate) mod resources;
pub(crate) mod script;
mod source;
pub(crate) mod tunnels;
pub(crate) use preferences::{Preferences, ensure_lan_bind_address};
pub(crate) use source::initialize_selection;
pub(crate) use source::{ProfileIdentity, Source, select_profile, selected_profile};

pub(crate) const INPUT_LIMIT: u64 = 8 * 1024 * 1024;
const OVERRIDE_LIMIT: usize = 128;
const DEPTH_LIMIT: usize = 64;
const NODE_LIMIT: usize = 100_000;

#[derive(Debug, thiserror::Error)]
pub(crate) enum CompositionError {
    #[error("配置来源 {0} 不是有效 YAML 对象")]
    InvalidYaml(String),
    #[error("配置来源 {0} 超过合成预算")]
    Budget(String),
    #[error("覆写索引格式不受支持或已损坏")]
    InvalidIndex,
    #[error("覆写 {0} 的元数据不完整或无效")]
    InvalidOverride(u64),
    #[error("覆写 {0}：{1}")]
    Script(u64, String),
    #[error("无法序列化合成配置")]
    Serialization,
}

#[derive(Clone, Copy)]
enum Scope {
    Global,
    Profile(u64),
}
#[derive(Clone, Copy)]
enum Language {
    Yaml,
    JavaScript,
}
struct Override<'a> {
    id: u64,
    enabled: bool,
    scope: Scope,
    language: Language,
    content: &'a str,
}

/// Order is the persisted `ids` list, never key order or language/scope grouping.
/// Missing index means no overrides; malformed metadata must never become success.
fn ordered_overrides(values: &Values) -> Result<Vec<Override<'_>>, CompositionError> {
    if values.is_empty() {
        return Ok(Vec::new());
    }
    if values.get("format").map(String::as_str) != Some("1") {
        return Err(CompositionError::InvalidIndex);
    }
    let ids = values.get("ids").ok_or(CompositionError::InvalidIndex)?;
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut seen = BTreeSet::new();
    let mut overrides = Vec::new();
    for raw_id in ids.split(',') {
        let id = raw_id.parse::<u64>().map_err(|error| {
            crate::logging::diagnostic_error(CompositionError::InvalidIndex, error)
        })?;
        if seen.len() >= OVERRIDE_LIMIT || !seen.insert(id) {
            return Err(CompositionError::InvalidIndex);
        }
        let field = |key: &str| values.get(&format!("{id}/{key}")).map(String::as_str);
        let invalid = || CompositionError::InvalidOverride(id);
        let enabled = match field("enabled") {
            Some("true") => true,
            Some("false") => false,
            _ => return Err(invalid()),
        };
        let language = match field("kind") {
            Some("YAML") => Language::Yaml,
            Some("JavaScript") => Language::JavaScript,
            _ => return Err(invalid()),
        };
        let scope = match field("scope-profile") {
            None | Some("") => Scope::Global,
            Some(id) => Scope::Profile(
                id.parse()
                    .map_err(|error| crate::logging::diagnostic_error(invalid(), error))?,
            ),
        };
        let content = field("content").ok_or_else(invalid)?;
        overrides.push(Override {
            id,
            enabled,
            scope,
            language,
            content,
        });
    }
    Ok(overrides)
}

fn object(bytes: &[u8], identity: &str) -> Result<Mapping, CompositionError> {
    if bytes.len() as u64 > INPUT_LIMIT {
        return Err(CompositionError::Budget(identity.into()));
    }
    // Parser diagnostics go only to the redacted log; keep the UI error concise.
    let value: Value = serde_yaml_ng::from_slice(bytes).map_err(|error| {
        crate::logging::diagnostic_error(CompositionError::InvalidYaml(identity.into()), error)
    })?;
    let mut remaining = NODE_LIMIT;
    validate_budget(&value, 0, &mut remaining)
        .map_err(|()| CompositionError::Budget(identity.into()))?;
    match value {
        Value::Mapping(mapping) => Ok(mapping),
        _ => Err(CompositionError::InvalidYaml(identity.into())),
    }
}

fn validate_budget(value: &Value, depth: usize, remaining: &mut usize) -> Result<(), ()> {
    if depth > DEPTH_LIMIT || *remaining == 0 {
        return Err(());
    }
    *remaining -= 1;
    match value {
        Value::Mapping(mapping) => validate_mapping(mapping, depth, remaining)?,
        Value::Sequence(sequence) => {
            for value in sequence {
                validate_budget(value, depth + 1, remaining)?;
            }
        }
        Value::Tagged(tagged) => validate_budget(&tagged.value, depth + 1, remaining)?,
        _ => {}
    }
    Ok(())
}

fn validate_mapping(mapping: &Mapping, depth: usize, remaining: &mut usize) -> Result<(), ()> {
    for (key, value) in mapping {
        validate_budget(key, depth + 1, remaining)?;
        validate_budget(value, depth + 1, remaining)?;
    }
    Ok(())
}

/// Match CV: ordinary objects recurse, arrays/scalars/null replace, DNS merges
/// one level, and Hosts replaces entirely. Only root keys are normalized.
fn merge(target: &mut Mapping, patch: Mapping) {
    for (key, value) in patch {
        match (target.get_mut(&key), value) {
            (Some(Value::Mapping(existing)), Value::Mapping(patch))
                if key.as_str() == Some("dns") =>
            {
                existing.extend(patch)
            }
            (Some(existing), value) if key.as_str() != Some("hosts") => {
                merge_value(existing, value)
            }
            (_, value) => {
                target.insert(key, value);
            }
        }
    }
}

fn merge_value(target: &mut Value, patch: Value) {
    match (target, patch) {
        (Value::Mapping(target), Value::Mapping(patch)) => {
            for (key, value) in patch {
                if let Some(existing) = target.get_mut(&key) {
                    merge_value(existing, value);
                } else {
                    target.insert(key, value);
                }
            }
        }
        (target, value) => *target = value,
    }
}

pub(crate) fn lowercase_root(mapping: Mapping) -> Mapping {
    mapping
        .into_iter()
        .filter_map(|(key, value)| {
            key.as_str()
                .map(|key| (Value::String(key.to_ascii_lowercase()), value))
        })
        .collect()
}

/// `profile` is the identity of the supplied source, not a UI selection.
/// The development runtime-source has no profile identity, so only globals apply.
/// Control-channel/platform protection runs afterwards in the lifecycle owner.
pub(crate) fn compose(
    source: &[u8],
    profile: Option<u64>,
    values: &Values,
) -> Result<Vec<u8>, CompositionError> {
    compose_reporting(source, profile, "", values, None, &mut |_| {})
}

pub(crate) fn validate_override_index(values: &Values) -> Result<(), CompositionError> {
    for item in ordered_overrides(values)? {
        match item.language {
            Language::Yaml => {
                object(item.content.as_bytes(), &format!("覆写 {}", item.id))?;
            }
            Language::JavaScript => script::validate(item.content)
                .map_err(|error| CompositionError::Script(item.id, error))?,
        }
    }
    Ok(())
}

pub(crate) fn compose_reporting(
    source: &[u8],
    profile: Option<u64>,
    name: &str,
    values: &Values,
    cancellation: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    report: &mut impl FnMut(script::Report),
) -> Result<Vec<u8>, CompositionError> {
    let mut config = lowercase_root(object(source, "原始来源")?);
    let mut input_bytes = source.len() as u64;
    for item in ordered_overrides(values)? {
        if !item.enabled || matches!(item.scope, Scope::Profile(id) if profile != Some(id)) {
            continue;
        }
        input_bytes = input_bytes.saturating_add(item.content.len() as u64);
        if input_bytes > INPUT_LIMIT {
            return Err(CompositionError::Budget(format!("覆写 {}", item.id)));
        }
        match item.language {
            Language::Yaml => {
                let patch = lowercase_root(object(
                    item.content.as_bytes(),
                    &format!("覆写 {}", item.id),
                )?);
                merge(&mut config, patch);
                let mut remaining = NODE_LIMIT - 1;
                validate_mapping(&config, 0, &mut remaining)
                    .map_err(|()| CompositionError::Budget(format!("覆写 {}", item.id)))?;
            }
            Language::JavaScript => {
                let (output, execution) = script::run_cancellable(
                    item.id,
                    item.content,
                    &config,
                    name,
                    cancellation.clone(),
                );
                report(execution);
                config = lowercase_root(
                    output.map_err(|error| CompositionError::Script(item.id, error))?,
                );
                let mut remaining = NODE_LIMIT - 1;
                validate_mapping(&config, 0, &mut remaining)
                    .map_err(|()| CompositionError::Budget(format!("覆写 {}", item.id)))?;
            }
        }
    }
    let yaml = serde_yaml_ng::to_string(&config).map_err(|error| {
        crate::logging::diagnostic_error(CompositionError::Serialization, error)
    })?;
    if yaml.len() as u64 > INPUT_LIMIT {
        return Err(CompositionError::Budget("合成结果".into()));
    }
    Ok(yaml.into_bytes())
}

#[cfg(test)]
mod tests;
