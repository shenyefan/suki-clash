//! Listener probes fail on conflicts; configured ports are never substituted.
/// Probe newly requested tunnel listeners before asking Mihomo to recreate them.
/// Existing owned endpoints are skipped because they remain bound during hot reload.
pub(super) fn probe_tunnels(
    previous: &crate::core::Preferences,
    desired: &crate::core::Preferences,
) -> Result<(), String> {
    use crate::configuration::tunnels;
    let old = tunnels::parse(&previous.values()["Tunnels"])?;
    let new = tunnels::parse(&desired.values()["Tunnels"])?;
    for entry in &new {
        for network in &entry.network {
            if old
                .iter()
                .any(|old| old.address == entry.address && old.network.contains(network))
            {
                continue;
            }
            let available = if network == "tcp" {
                std::net::TcpListener::bind(&entry.address).map(|_| ())
            } else {
                std::net::UdpSocket::bind(&entry.address).map(|_| ())
            };
            if let Err(error) = available {
                return Err(format!(
                    "隧道 {} {network} 无法监听：{}",
                    entry.address,
                    error.kind()
                ));
            }
        }
    }
    Ok(())
}
