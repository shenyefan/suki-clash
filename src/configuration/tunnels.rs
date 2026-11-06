//! Native tunnel records, independent of the settings form.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Tunnel {
    pub(crate) network: Vec<String>,
    pub(crate) address: String,
    pub(crate) target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) proxy: Option<String>,
}
impl Tunnel {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.network.is_empty()
            || self.network.len() > 2
            || self
                .network
                .iter()
                .any(|n| !matches!(n.as_str(), "tcp" | "udp"))
            || (self.network.len() == 2 && self.network[0] == self.network[1])
        {
            return Err("隧道协议必须为 tcp、udp 或 tcp+udp".into());
        }
        let local = self
            .address
            .parse::<std::net::SocketAddr>()
            .map_err(|error| crate::logging::rejected("本地隧道地址必须为 IP:端口", error))?;
        if local.port() == 0 {
            return Err("本地隧道端口不能为 0".into());
        }
        if let Ok(target) = self.target.parse::<std::net::SocketAddr>() {
            if target.port() == 0 {
                return Err("目标端口不能为 0".into());
            }
        } else {
            let (host, port) = self
                .target
                .rsplit_once(':')
                .ok_or("目标地址必须为主机:端口")?;
            if host.is_empty()
                || host.len() > 253
                || !host.split('.').all(|part| {
                    !part.is_empty()
                        && part.len() <= 63
                        && !part.starts_with('-')
                        && !part.ends_with('-')
                        && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                })
                || port.parse::<u16>().ok().filter(|p| *p != 0).is_none()
            {
                return Err("隧道目标主机或端口无效".into());
            }
        }
        if self
            .proxy
            .as_ref()
            .is_some_and(|p| p.trim().is_empty() || p.len() > 1024)
        {
            return Err("隧道代理名称无效".into());
        }
        Ok(())
    }
}
pub(crate) fn parse(yaml: &str) -> Result<Vec<Tunnel>, String> {
    if yaml.len() > 256 * 1024 {
        return Err("隧道配置超过大小上限".into());
    }
    let entries: Vec<Tunnel> = serde_yaml_ng::from_str(yaml)
        .map_err(|error| crate::logging::rejected("隧道配置格式无效", error))?;
    if entries.len() > 128 {
        return Err("隧道数量不能超过 128".into());
    }
    for (ix, entry) in entries.iter().enumerate() {
        entry.validate()?;
        let address: std::net::SocketAddr = entry
            .address
            .parse()
            .map_err(|error| crate::logging::rejected("隧道监听无效", error))?;
        for other in &entries[..ix] {
            let other_addr: std::net::SocketAddr = other
                .address
                .parse()
                .map_err(|error| crate::logging::rejected("隧道监听无效", error))?;
            if address.port() == other_addr.port()
                && (address.ip() == other_addr.ip()
                    || address.ip().is_unspecified()
                    || other_addr.ip().is_unspecified())
                && entry.network.iter().any(|n| other.network.contains(n))
            {
                return Err("隧道监听地址及协议重复".into());
            }
        }
    }
    Ok(entries)
}
/// Compare the original record when editing/deleting, so a stale editor cannot
/// overwrite a newer record. Native Mihomo YAML remains free of UI identifiers.
#[derive(Clone)]
pub(crate) enum Change {
    Add(Tunnel),
    Replace { previous: Tunnel, entry: Tunnel },
    Remove(Tunnel),
}
pub(crate) fn change(mut entries: Vec<Tunnel>, change: Change) -> Result<Vec<Tunnel>, String> {
    match change {
        Change::Add(entry) => entries.push(entry),
        Change::Replace { previous, entry } => {
            let current = entries
                .iter_mut()
                .find(|entry| **entry == previous)
                .ok_or("隧道已变更，请重新打开编辑")?;
            *current = entry;
        }
        Change::Remove(previous) => {
            let ix = entries
                .iter()
                .position(|entry| *entry == previous)
                .ok_or("隧道已变更，请刷新后重试")?;
            entries.remove(ix);
        }
    }
    let yaml = serde_yaml_ng::to_string(&entries)
        .map_err(|error| crate::logging::diagnostic_error("无法编码隧道配置", error))?;
    parse(&yaml)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_edits_are_rejected_and_other_tunnels_are_preserved() {
        let original = Tunnel {
            network: vec!["tcp".into()],
            address: "127.0.0.1:32123".into(),
            target: "example.com:443".into(),
            proxy: Some("DIRECT".into()),
        };
        let other = Tunnel {
            address: "127.0.0.1:32124".into(),
            ..original.clone()
        };
        let mut updated = original.clone();
        updated.target = "example.org:80".into();
        let entries = change(
            vec![original.clone(), other.clone()],
            Change::Replace {
                previous: original.clone(),
                entry: updated.clone(),
            },
        )
        .unwrap();
        assert_eq!(entries, vec![updated.clone(), other.clone()]);
        assert!(change(entries.clone(), Change::Remove(original)).is_err());
        assert_eq!(
            change(entries, Change::Remove(updated)).unwrap(),
            vec![other]
        );
    }
    #[test]
    fn endpoints_and_transports_are_validated() {
        let mut tunnel = Tunnel {
            network: vec!["tcp".into(), "udp".into()],
            address: "[::1]:32123".into(),
            target: "example.com:443".into(),
            proxy: Some("DIRECT".into()),
        };
        tunnel.validate().unwrap();
        let duplicate = serde_yaml_ng::to_string(&vec![tunnel.clone(), tunnel.clone()]).unwrap();
        assert!(parse(&duplicate).is_err());
        tunnel.target = "https://example.com:443".into();
        assert!(tunnel.validate().is_err());
        tunnel.target = "127.0.0.1:0".into();
        assert!(tunnel.validate().is_err());
    }
}
