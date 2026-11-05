//! Native, read-only network inventory through one cross-platform library.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Interface {
    pub(crate) name: String,
    pub(crate) mac: String,
    pub(crate) addresses: Vec<String>,
}
pub(super) async fn list() -> Result<Vec<Interface>, String> {
    tokio::task::spawn_blocking(|| {
        let mut interfaces = netdev::get_interfaces()
            .into_iter()
            .map(|interface| {
                let mut addresses = interface
                    .ipv4
                    .iter()
                    .map(|network| network.addr().to_string())
                    .chain(
                        interface
                            .ipv6
                            .iter()
                            .map(|network| network.addr().to_string()),
                    )
                    .collect::<Vec<_>>();
                addresses.sort();
                addresses.dedup();
                Interface {
                    name: interface.name,
                    mac: interface
                        .mac_addr
                        .map(|mac| mac.to_string())
                        .unwrap_or_default(),
                    addresses,
                }
            })
            .collect::<Vec<_>>();
        interfaces.sort_by(|a, b| a.name.cmp(&b.name));
        interfaces
    })
    .await
    .map_err(|error| crate::logging::diagnostic_error("系统网卡查询任务异常退出".into(), error))
}
