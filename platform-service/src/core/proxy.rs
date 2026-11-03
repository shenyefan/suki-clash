use crate::{MacosProxyConfig, ProxyApplyOutcome};
use anyhow::{Context, ensure};
use std::net::IpAddr;
use url::{Host, Url};
const MAX_HOST_LEN: usize = 64;
const MAX_BYPASS_LEN: usize = 8192;
const MAX_PAC_URL_LEN: usize = 256;
const PAC_PATH: &str = "/proxy.pac";

pub fn validate_proxy_config(config: &MacosProxyConfig) -> anyhow::Result<()> {
    match config {
        MacosProxyConfig::Disabled => Ok(()),
        MacosProxyConfig::Global { host, port, bypass } => {
            ensure!(
                host.len() <= MAX_HOST_LEN,
                "proxy host exceeds {MAX_HOST_LEN} bytes"
            );
            ensure!(
                bypass.len() <= MAX_BYPASS_LEN,
                "proxy bypass exceeds {MAX_BYPASS_LEN} bytes"
            );
            ensure!(!host.contains('\0'), "proxy host contains NUL");
            ensure!(!bypass.contains('\0'), "proxy bypass contains NUL");
            ensure!(*port != 0, "proxy port must be nonzero");

            let address: IpAddr = host.parse().context("proxy host must be an IP address")?;
            ensure!(address.is_loopback(), "proxy host must be loopback");
            Ok(())
        }
        MacosProxyConfig::Pac { url } => validate_pac_url(url),
    }
}

fn validate_pac_url(raw: &str) -> anyhow::Result<()> {
    ensure!(
        raw.len() <= MAX_PAC_URL_LEN,
        "PAC URL exceeds {MAX_PAC_URL_LEN} bytes"
    );
    ensure!(!raw.contains('\0'), "PAC URL contains NUL");

    let parsed = Url::parse(raw).context("invalid PAC URL")?;
    ensure!(parsed.scheme() == "http", "PAC URL scheme must be http");
    ensure!(
        parsed.username().is_empty(),
        "PAC URL must not contain a username"
    );
    ensure!(
        parsed.password().is_none(),
        "PAC URL must not contain a password"
    );
    ensure!(parsed.query().is_none(), "PAC URL must not contain a query");
    ensure!(
        parsed.fragment().is_none(),
        "PAC URL must not contain a fragment"
    );
    ensure!(parsed.path() == PAC_PATH, "PAC URL path must be {PAC_PATH}");

    let authority = raw
        .split_once("://")
        .map(|(_, remainder)| remainder)
        .and_then(|remainder| remainder.split(['/', '?', '#']).next())
        .context("PAC URL must contain an authority")?;
    ensure!(
        !authority.contains('@'),
        "PAC URL must not contain userinfo"
    );
    ensure!(
        explicit_port(authority)? != 0,
        "PAC URL port must be nonzero"
    );

    let host = parsed.host().context("PAC URL must contain a host")?;
    ensure!(
        parsed
            .host_str()
            .is_some_and(|host| host.len() <= MAX_HOST_LEN),
        "PAC URL host exceeds {MAX_HOST_LEN} bytes"
    );
    let is_loopback = match host {
        Host::Ipv4(address) => address.is_loopback(),
        Host::Ipv6(address) => address.is_loopback(),
        Host::Domain(_) => false,
    };
    ensure!(is_loopback, "PAC URL host must be a loopback IP address");
    Ok(())
}

fn explicit_port(authority: &str) -> anyhow::Result<u16> {
    let port = if authority.starts_with('[') {
        let closing_bracket = authority.find(']').context("invalid PAC URL IPv6 host")?;
        authority
            .get(closing_bracket + 1..)
            .and_then(|remainder| remainder.strip_prefix(':'))
    } else {
        authority.rsplit_once(':').map(|(_, port)| port)
    }
    .context("PAC URL must contain an explicit port")?;

    ensure!(!port.is_empty(), "PAC URL must contain an explicit port");
    port.parse::<u16>().context("PAC URL port must be a u16")
}

pub fn validate_operation(operation: &crate::proxy_owner::Operation) -> anyhow::Result<()> {
    use crate::proxy_owner::Operation;
    if let Operation::Apply(target) = operation {
        ensure!(
            !(target.system.enable && target.auto.enable),
            "manual and PAC proxy are mutually exclusive"
        );
        if target.system.enable {
            validate_proxy_config(&MacosProxyConfig::Global {
                host: target.system.host.clone(),
                port: target.system.port,
                bypass: target.system.bypass.clone(),
            })?;
        } else if target.auto.enable {
            validate_proxy_config(&MacosProxyConfig::Pac {
                url: target.auto.url.clone(),
            })?;
            ensure!(
                target.system.bypass.len() <= MAX_BYPASS_LEN
                    && !target.system.bypass.contains('\0'),
                "invalid proxy bypass"
            );
        }
    }
    Ok(())
}

#[cfg(all(target_os = "macos", not(any(test, feature = "test"))))]
struct ProxyWork {
    operation: crate::proxy_owner::Operation,
    reply: tokio::sync::oneshot::Sender<Result<crate::proxy_owner::Observation, String>>,
}
#[cfg(all(target_os = "macos", not(any(test, feature = "test"))))]
struct ProxyWorker {
    sender: tokio::sync::mpsc::Sender<ProxyWork>,
    _thread: std::thread::JoinHandle<()>,
}
#[cfg(all(target_os = "macos", not(any(test, feature = "test"))))]
impl ProxyWorker {
    fn new() -> Result<Self, String> {
        let journal = crate::service_paths()
            .map_err(|error| error.to_string())?
            .persistent_state_dir()
            .join("system-proxy-owner.json");
        let (sender, mut receiver) = tokio::sync::mpsc::channel::<ProxyWork>(2);
        let thread = std::thread::Builder::new()
            .name("suki-service-proxy".into())
            .spawn(move || {
                use crate::proxy_owner::{Operation, Owner};
                let mut owner = None;
                while let Some(ProxyWork { operation, reply }) = receiver.blocking_recv() {
                    if owner.is_none()
                        || (owner.as_ref().is_some_and(Result::is_err)
                            && matches!(&operation, Operation::Apply(_) | Operation::Observe))
                    {
                        owner = Some(
                            crate::macos_proxy::Adapter::new(Some(journal.clone()), false)
                                .map(Owner::new),
                        );
                    }
                    let result = match owner.as_mut() {
                        Some(Ok(owner)) => match operation {
                            Operation::Apply(target) => owner.apply(target),
                            Operation::Observe => owner.observe(),
                            Operation::Rollback => owner.rollback(),
                            Operation::Clear => owner.clear(),
                        },
                        Some(Err(error)) => Err(error.clone()),
                        None => Err("system proxy owner is not initialized".into()),
                    };
                    let _ = reply.send(result);
                }
                if let Some(Ok(owner)) = &mut owner {
                    let _ = owner.clear();
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            sender,
            _thread: thread,
        })
    }
}

pub async fn execute_operation(
    operation: crate::proxy_owner::Operation,
) -> anyhow::Result<crate::proxy_owner::Observation> {
    #[cfg(all(target_os = "macos", not(any(test, feature = "test"))))]
    {
        static WORKER: once_cell::sync::Lazy<Result<ProxyWorker, String>> =
            once_cell::sync::Lazy::new(ProxyWorker::new);
        let worker = WORKER
            .as_ref()
            .map_err(|error| anyhow::anyhow!(error.clone()))?;
        let (reply, receiver) = tokio::sync::oneshot::channel();
        worker
            .sender
            .send(ProxyWork { operation, reply })
            .await
            .map_err(|_| anyhow::anyhow!("system proxy owner stopped"))?;
        receiver
            .await
            .context("system proxy owner exited")?
            .map_err(anyhow::Error::msg)
    }
    #[cfg(any(test, feature = "test"))]
    {
        // Explicit service integration tests must never alter host network settings.
        let target = match operation {
            crate::proxy_owner::Operation::Apply(target) => Some(target),
            _ => None,
        };
        Ok(crate::proxy_owner::Observation::read(
            &Default::default(),
            target.as_ref(),
        ))
    }
    #[cfg(all(not(target_os = "macos"), not(any(test, feature = "test"))))]
    {
        let _ = operation;
        anyhow::bail!("macOS proxy configuration is unsupported on this platform")
    }
}

pub async fn apply_proxy(config: &MacosProxyConfig) -> anyhow::Result<()> {
    validate_proxy_config(config)?;
    use crate::proxy_owner::{
        Operation, Target,
        types::{ManualProxy, PacProxy},
    };
    let target = match config {
        MacosProxyConfig::Disabled => Target::default(),
        MacosProxyConfig::Global { host, port, bypass } => Target {
            system: ManualProxy {
                enable: true,
                host: host.clone(),
                port: *port,
                bypass: bypass.clone(),
            },
            auto: PacProxy::default(),
        },
        MacosProxyConfig::Pac { url } => Target {
            system: ManualProxy::default(),
            auto: PacProxy {
                enable: true,
                url: url.clone(),
            },
        },
    };
    execute_operation(Operation::Apply(target)).await?;
    Ok(())
}
pub async fn clear_proxy() -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    execute_operation(crate::proxy_owner::Operation::Clear).await?;
    Ok(())
}
pub async fn clear_proxy_waiting_for_network() -> anyhow::Result<()> {
    clear_proxy().await
}
pub async fn apply_proxy_or_direct(
    config: Option<&MacosProxyConfig>,
) -> anyhow::Result<ProxyApplyOutcome> {
    match config {
        None => Ok(ProxyApplyOutcome::NotRequested),
        Some(config) => {
            apply_proxy(config).await?;
            Ok(ProxyApplyOutcome::Applied)
        }
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn service_proxy_boundary_rejects_external_targets_and_mixed_modes() {
        use crate::proxy_owner::{
            Operation, Target,
            types::{ManualProxy, PacProxy},
        };
        let mut target = Target {
            system: ManualProxy {
                enable: true,
                host: "203.0.113.1".into(),
                port: 8080,
                bypass: String::new(),
            },
            auto: PacProxy::default(),
        };
        assert!(super::validate_operation(&Operation::Apply(target.clone())).is_err());
        target.system.host = "127.0.0.1".into();
        assert!(super::validate_operation(&Operation::Apply(target.clone())).is_ok());
        target.auto = PacProxy {
            enable: true,
            url: "http://127.0.0.1:53111/proxy.pac".into(),
        };
        assert!(super::validate_operation(&Operation::Apply(target.clone())).is_err());
        target.system.enable = false;
        assert!(super::validate_operation(&Operation::Apply(target)).is_ok());
    }
}
