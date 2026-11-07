//! Read-only machine and application environment collected on the diagnostics worker.
use crate::storage::{AppPaths, DirectoryKind};
use serde_json::{Value, json};
use std::{collections::BTreeMap, ffi::OsString, net::IpAddr};
use sysinfo::{
    CpuRefreshKind, Disks, Gpus, MemoryRefreshKind, ProcessRefreshKind, RefreshKind, System,
    UpdateKind,
};

pub(super) fn collect(paths: &AppPaths) -> Value {
    let system = System::new_with_specifics(
        RefreshKind::nothing()
            .with_cpu(CpuRefreshKind::everything())
            .with_memory(MemoryRefreshKind::everything())
            .with_processes(
                ProcessRefreshKind::nothing()
                    .without_tasks()
                    .with_memory()
                    .with_exe(UpdateKind::OnlyIfNotSet),
            ),
    );
    let resources = observed(system.map(|system| {
        let mut processes = system.processes().iter().map(|(pid, process)| json!({
            "pid":pid.as_u32(), "parent_pid":process.parent().map(|pid|pid.as_u32()),
            "name":process.name().to_string_lossy(), "executable":process.exe(),
            "status":process.status().to_string(), "memory_bytes":process.memory(),
            "started_at_unix_seconds":process.start_time(),
        })).collect::<Vec<_>>();
        processes.sort_by_key(|process| process["pid"].as_u64());
        json!({
            "logical_cpu_count":system.cpus().len(),
            "physical_core_count":observed(System::physical_core_count()),
            "cpus":system.cpus().iter().map(|cpu|json!({"name":cpu.name(), "brand":cpu.brand(), "vendor":cpu.vendor_id(), "frequency_mhz":cpu.frequency()})).collect::<Vec<_>>(),
            "memory":{"total_bytes":system.total_memory(), "available_bytes":system.available_memory(), "free_bytes":system.free_memory(), "swap_total_bytes":system.total_swap(), "swap_free_bytes":system.free_swap()},
            "processes":processes,
        })
    }));
    let disks = observed(Disks::new_with_refreshed_list().map(|disks| {
        disks.iter().map(|disk|json!({
        "name":disk.name().to_string_lossy(), "mount_point":disk.mount_point(),
        "file_system":disk.file_system().to_string_lossy(), "kind":format!("{:?}",disk.kind()),
        "total_bytes":disk.total_space(), "available_bytes":disk.available_space(),
    })).collect::<Vec<_>>()
    }));
    let gpus = observed(Gpus::new_with_refreshed_list().map(|gpus| {
        gpus.iter().map(|gpu|json!({
        "vendor":gpu.vendor(), "model":gpu.model(), "total_memory_bytes":gpu.total_memory(),
    })).collect::<Vec<_>>()
    }));
    let interfaces = netdev::get_interfaces().into_iter().map(|interface|json!({
        "index":interface.index, "name":interface.name, "friendly_name":interface.friendly_name,
        "description":interface.description, "type":format!("{:?}",interface.if_type),
        "state":format!("{:?}",interface.oper_state), "up":interface.is_up(),
        "loopback":interface.is_loopback(), "default_route":interface.default,
        "mtu_bytes":interface.mtu, "flags":interface.flags, "mac_present":interface.mac_addr.is_some(),
        "ipv4":interface.ipv4.iter().map(|address|format!("{}/{}", network_address(address.addr().into()),address.prefix_len())).collect::<Vec<_>>(),
        "ipv6":interface.ipv6.iter().map(|address|format!("{}/{}", network_address(address.addr().into()),address.prefix_len())).collect::<Vec<_>>(),
        "dns":interface.dns_servers.iter().copied().map(network_address).collect::<Vec<_>>(),
        "gateway":interface.gateway.map(|gateway|json!({"ipv4":gateway.ipv4.into_iter().map(|ip|network_address(ip.into())).collect::<Vec<_>>(),"ipv6":gateway.ipv6.into_iter().map(|ip|network_address(ip.into())).collect::<Vec<_>>()})),
        "transmit_bits_per_second":interface.transmit_speed, "receive_bits_per_second":interface.receive_speed,
        "dhcp_v4":interface.dhcp_v4_enabled, "dhcp_v6":interface.dhcp_v6_enabled,
    })).collect::<Vec<_>>();
    let directories = [
        DirectoryKind::Config,
        DirectoryKind::Data,
        DirectoryKind::Cache,
        DirectoryKind::Runtime,
        DirectoryKind::Logs,
        DirectoryKind::Core,
        DirectoryKind::Backups,
    ]
    .into_iter()
    .map(|kind| (format!("{kind:?}"), paths.directory(kind)))
    .collect::<BTreeMap<_, _>>();
    json!({
        "application":{"name":"Suki Clash","version":env!("CARGO_PKG_VERSION"),"channel":crate::updates::channel(),
            "debug_build":cfg!(debug_assertions),"pid":std::process::id(),"elevated":observed(crate::platform_access::elevated()),
            "executable":observed(std::env::current_exe()),"working_directory":observed(std::env::current_dir()),
            "directories":directories,"core_installed":crate::bootstrap::require_core(paths).is_ok(),
            "service_protocol":suki_service::ProtocolVersion::current().header_value()},
        "system":{"os":std::env::consts::OS,"architecture":System::cpu_arch(),"family":std::env::consts::FAMILY,
            "name":observed(System::name()),"version":observed(System::long_os_version()),"kernel_version":observed(System::kernel_version()),
            "uptime_seconds":observed(System::uptime()),"boot_time_unix_seconds":observed(System::boot_time()),
            "capture_time":chrono::Local::now().to_rfc3339(),"utc_time":chrono::Utc::now().to_rfc3339(),
            "resources":resources,"disks":disks,"graphics":gpus,"network_interfaces":interfaces},
        "environment_variables":variables(std::env::vars_os()),
    })
}

fn observed<T: serde::Serialize, E: std::fmt::Display>(result: Result<T, E>) -> Value {
    match result {
        Ok(value) => json!(value),
        Err(error) => json!({"unavailable":error.to_string()}),
    }
}
fn network_address(address: IpAddr) -> String {
    let local = match address {
        IpAddr::V4(ip) => {
            ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_unspecified()
        }
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_unique_local()
                || ip.is_unicast_link_local()
                || ip.is_unspecified()
        }
    };
    if local {
        address.to_string()
    } else {
        "<redacted-public-ip>".into()
    }
}
fn variables(entries: impl Iterator<Item = (OsString, OsString)>) -> BTreeMap<String, String> {
    entries
        .map(|(name, value)| {
            let name = name.to_string_lossy().into_owned();
            let normalized = name
                .chars()
                .filter(|ch| ch.is_ascii_alphanumeric())
                .collect::<String>()
                .to_ascii_lowercase();
            let hidden = [
                "token",
                "secret",
                "password",
                "passwd",
                "passphrase",
                "credential",
                "authorization",
                "cookie",
                "private",
                "connectionstring",
                "key",
                "session",
            ]
            .iter()
            .any(|part| normalized.contains(part))
                || matches!(
                    normalized.as_str(),
                    "user"
                        | "username"
                        | "logname"
                        | "hostname"
                        | "computername"
                        | "userdomain"
                        | "userdnsdomain"
                );
            (
                name,
                if hidden && !value.is_empty() {
                    "<redacted>".into()
                } else {
                    value.to_string_lossy().into_owned()
                },
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn environment_keeps_operating_values_and_masks_credentials_and_identity() {
        let entries = [
            ("PATH", "/opt/tools"),
            ("LANG", "zh_CN.UTF-8"),
            ("HTTP_PROXY", "http://localhost:7890"),
            ("OPENAI_API_KEY", "api-private"),
            ("AZURE_STORAGE_CONNECTION_STRING", "connection-private"),
            ("USERNAME", "user-private"),
            ("SESSION_ID", "session-private"),
        ];
        let variables = super::variables(entries.into_iter().map(|(k, v)| (k.into(), v.into())));
        assert_eq!(variables["PATH"], "/opt/tools");
        assert_eq!(variables["LANG"], "zh_CN.UTF-8");
        assert_eq!(variables["HTTP_PROXY"], "http://localhost:7890");
        for key in [
            "OPENAI_API_KEY",
            "AZURE_STORAGE_CONNECTION_STRING",
            "USERNAME",
            "SESSION_ID",
        ] {
            assert_eq!(variables[key], "<redacted>");
        }
    }
}
