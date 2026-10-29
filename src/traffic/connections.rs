//! Session connection history. Domain identity is independent of row position and display order.
use super::model::Record;
use crate::core::Runtime;
use crate::overview::statistics::{Connection, Connections};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::Arc,
    time::Instant,
};
#[derive(Clone, Debug)]
pub(crate) struct Target {
    pub(crate) id: String,
    pub(crate) generation: u64,
    pub(crate) epoch: u64,
}
#[derive(Clone)]
pub(crate) enum Action {
    Close(Target),
    CloseAll { generation: u64, epoch: u64 },
}
#[derive(Clone)]
struct Tracked {
    row: Record,
    upload: u64,
    download: u64,
    at: Instant,
}
#[derive(Clone)]
pub(super) struct Closed {
    entries: Vec<((u64, String), Tracked)>,
}
#[derive(Default)]
pub(super) struct Tracker {
    entries: BTreeMap<(u64, String), Tracked>,
    closed: VecDeque<(u64, String)>,
    next_id: u64,
    previous: Option<Arc<Connections>>,
    scope: Option<(u64, u64)>,
    live: bool,
}
impl Tracker {
    pub(super) fn update(&mut self, runtime: &Runtime) -> bool {
        let scope = (runtime.generation, runtime.epoch);
        if !runtime.connections_live {
            self.live = false;
            return false;
        }
        if self.live
            && self.scope == Some(scope)
            && self
                .previous
                .as_ref()
                .is_some_and(|previous| Arc::ptr_eq(previous, &runtime.connections))
        {
            return false;
        }
        let Some(at) = runtime.connections_at else {
            return false;
        };
        self.scope = Some(scope);
        self.live = true;
        self.previous = Some(runtime.connections.clone());
        let current = runtime
            .connections
            .connections
            .iter()
            .map(|connection| (runtime.generation, connection.id.clone()))
            .collect::<BTreeSet<_>>();
        for (key, entry) in &mut self.entries {
            if entry.row.active && !current.contains(key) {
                entry.row.active = false;
                entry.row.numbers[3] = 0.;
                entry.row.numbers[4] = 0.;
                entry.row.cells[3] = "0 B/s".into();
                entry.row.cells[4] = "0 B/s".into();
                self.closed.push_back(key.clone());
            }
        }
        for connection in &runtime.connections.connections {
            let key = (runtime.generation, connection.id.clone());
            let previous = self.entries.get(&key);
            let (up, down) = previous
                .filter(|previous| previous.row.active)
                .map(|previous| {
                    let seconds = at.saturating_duration_since(previous.at).as_secs_f64();
                    if seconds <= 0. || seconds > 15. {
                        (0., 0.)
                    } else {
                        (
                            connection.upload.saturating_sub(previous.upload) as f64 / seconds,
                            connection.download.saturating_sub(previous.download) as f64 / seconds,
                        )
                    }
                })
                .unwrap_or((0., 0.));
            let id = if let Some(previous) = previous {
                previous.row.id
            } else {
                self.next_id = self.next_id.saturating_add(1);
                self.next_id
            };
            let row = record(connection, id, runtime.generation, runtime.epoch, up, down);
            self.entries.insert(
                key,
                Tracked {
                    row,
                    upload: connection.upload,
                    download: connection.download,
                    at,
                },
            );
        }
        while self.closed.len() > 1000 {
            if let Some(key) = self.closed.pop_front()
                && self
                    .entries
                    .get(&key)
                    .is_some_and(|entry| !entry.row.active)
            {
                self.entries.remove(&key);
            }
        }
        true
    }
    pub(super) fn rows(&self) -> Vec<Record> {
        let mut rows = self
            .entries
            .values()
            .map(|entry| entry.row.clone())
            .collect::<Vec<_>>();
        rows.sort_by_key(|row| row.id);
        rows
    }
    pub(super) fn stopped(&mut self) -> bool {
        let mut changed = false;
        for (key, entry) in &mut self.entries {
            if entry.row.active {
                entry.row.active = false;
                entry.row.numbers[3] = 0.;
                entry.row.numbers[4] = 0.;
                entry.row.cells[3] = "0 B/s".into();
                entry.row.cells[4] = "0 B/s".into();
                self.closed.push_back(key.clone());
                changed = true;
            }
        }
        while self.closed.len() > 1000 {
            if let Some(key) = self.closed.pop_front() {
                self.entries.remove(&key);
            }
        }
        self.live = false;
        changed
    }
    pub(super) fn clear_closed(&mut self) -> Closed {
        let entries = self
            .closed
            .drain(..)
            .filter_map(|key| self.entries.remove(&key).map(|entry| (key, entry)))
            .collect();
        Closed { entries }
    }
    pub(super) fn restore_closed(&mut self, removed: Closed) {
        for (key, entry) in removed.entries {
            if !self.entries.contains_key(&key) {
                self.closed.push_back(key.clone());
                self.entries.insert(key, entry);
            }
        }
        let entries = &self.entries;
        self.closed
            .make_contiguous()
            .sort_by_key(|key| entries[key].row.id);
        while self.closed.len() > 1000 {
            if let Some(key) = self.closed.pop_front() {
                self.entries.remove(&key);
            }
        }
    }
}
fn amount(value: f64) -> String {
    let mut value = value;
    let units = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut unit = 0;
    while value >= 1024. && unit < units.len() - 1 {
        value /= 1024.;
        unit += 1;
    }
    format!("{value:.1} {}", units[unit])
}
fn address(ip: &str, port: &str) -> String {
    if ip.is_empty() {
        return "—".into();
    }
    if port.is_empty() {
        return ip.into();
    }
    if ip.contains(':') {
        format!("[{ip}]:{port}")
    } else {
        format!("{ip}:{port}")
    }
}
fn record(
    connection: &Connection,
    id: u64,
    generation: u64,
    epoch: u64,
    up: f64,
    down: f64,
) -> Record {
    let metadata = &connection.metadata;
    let host = if metadata.host.is_empty() {
        if metadata.destination_ip.is_empty() {
            metadata.remote_destination.clone()
        } else {
            metadata.destination_ip.clone()
        }
    } else {
        metadata.host.clone()
    };
    let process = if metadata.process.is_empty() {
        if metadata.process_path.is_empty() {
            "未知进程".to_owned()
        } else {
            metadata.process_path.clone()
        }
    } else {
        metadata.process.clone()
    };
    let start = chrono::DateTime::parse_from_rfc3339(&connection.start)
        .ok()
        .map(|time| {
            time.with_timezone(&chrono::Local)
                .format("%H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|| "—".into());
    let chain = connection
        .chains
        .iter()
        .rev()
        .cloned()
        .collect::<Vec<_>>()
        .join(" → ");
    let source = address(&metadata.source_ip, &metadata.source_port);
    let destination = address(
        if metadata.destination_ip.is_empty() {
            &metadata.remote_destination
        } else {
            &metadata.destination_ip
        },
        &metadata.destination_port,
    );
    let protocol = format!("{} / {}", metadata.network.to_uppercase(), metadata.kind);
    let rule = if connection.rule_payload.is_empty() {
        connection.rule.clone()
    } else {
        format!("{} ({})", connection.rule, connection.rule_payload)
    };
    let cells = vec![
        host,
        amount(connection.download as f64),
        amount(connection.upload as f64),
        format!("{}/s", amount(down)),
        format!("{}/s", amount(up)),
        chain,
        rule,
        process,
        start,
        source,
        destination,
        protocol,
    ];
    let labels = [
        "主机",
        "下载总量",
        "上传总量",
        "下载速率",
        "上传速率",
        "代理链",
        "规则",
        "进程",
        "开始时间",
        "源地址",
        "目标地址",
        "协议",
    ];
    let details = labels
        .iter()
        .zip(&cells)
        .map(|(label, value)| format!("{label}\t{value}"))
        .chain([
            format!("进程路径\t{}", metadata.process_path),
            format!("远端目标\t{}", metadata.remote_destination),
            format!("目标端口\t{}", metadata.destination_port),
            format!(
                "完整开始时间\t{}",
                chrono::DateTime::parse_from_rfc3339(&connection.start)
                    .map(|time| time
                        .with_timezone(&chrono::Local)
                        .format("%Y-%m-%d %H:%M:%S %:z")
                        .to_string())
                    .unwrap_or_else(|_| connection.start.clone())
            ),
            format!("连接 ID\t{}", connection.id),
        ])
        .collect::<Vec<_>>()
        .join("\n");
    Record {
        id,
        connection: Some(Target {
            id: connection.id.clone(),
            generation,
            epoch,
        }),
        cells,
        numbers: vec![
            0.,
            connection.download as f64,
            connection.upload as f64,
            down / 1024.,
            up / 1024.,
            0.,
            0.,
            0.,
            0.,
            0.,
            0.,
            0.,
        ],
        active: true,
        enabled: true,
        source: 0,
        details,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overview::statistics::{Connection, Metadata};
    use std::time::Duration;
    fn frame(id: &str, generation: u64, at: Instant, upload: u64) -> Runtime {
        Runtime {
            generation,
            epoch: 1,
            connections_live: true,
            connections_at: Some(at),
            connections: Arc::new(Connections {
                connections: vec![Connection {
                    id: id.into(),
                    upload,
                    download: upload * 2,
                    metadata: Metadata {
                        host: "example.test".into(),
                        network: "tcp".into(),
                        source_ip: "::1".into(),
                        source_port: "1234".into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        }
    }
    #[test]
    fn connection_detail_formats_full_time_path_remote_target_and_ipv6() {
        let connection = Connection {
            id: "connection-id".into(),
            start: "2026-10-29T10:21:32+08:00".into(),
            metadata: Metadata {
                remote_destination: "remote.example.test".into(),
                destination_port: "443".into(),
                source_ip: "::1".into(),
                source_port: "50123".into(),
                process_path: "/Applications/Example.app/Contents/MacOS/example".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let row = record(&connection, 1, 2, 3, 1024., 2048.);
        assert_eq!(row.cells[0], "remote.example.test");
        assert_eq!(row.cells[9], "[::1]:50123");
        assert_eq!(row.cells[10], "remote.example.test:443");
        assert_eq!(
            row.cells[7],
            "/Applications/Example.app/Contents/MacOS/example"
        );
        assert!(row.details.contains("2026-10-29"));
        assert!(row.details.contains("完整开始时间\t"));
        assert!(row.details.contains("连接 ID\tconnection-id"));
        assert!(!row.details.contains("2026-10-29T"));
    }
    #[test]
    fn stream_updates_keep_identity_compute_rates_and_preserve_history_across_disconnects() {
        let mut tracker = Tracker::default();
        let at = Instant::now();
        let first = frame("uuid", 1, at, 100);
        assert!(tracker.update(&first));
        let id = tracker.rows()[0].id;
        assert!(!tracker.update(&first));
        assert!(tracker.update(&frame("uuid", 1, at + Duration::from_secs(2), 2148)));
        let rows = tracker.rows();
        assert_eq!(rows[0].id, id);
        assert_eq!(rows[0].numbers[4], 1.);
        assert_eq!(rows[0].cells[9], "[::1]:1234");
        assert!(!tracker.update(&Runtime::default()));
        assert!(tracker.rows()[0].active);
        let empty = Runtime {
            generation: 1,
            epoch: 1,
            connections_live: true,
            connections_at: Some(at + Duration::from_secs(3)),
            ..Default::default()
        };
        assert!(tracker.update(&empty));
        assert!(!tracker.rows()[0].active);
        assert!(tracker.update(&frame("uuid", 2, at + Duration::from_secs(4), 10)));
        assert_eq!(tracker.rows().len(), 2);
        assert_ne!(tracker.rows()[1].id, id);
        let removed = tracker.clear_closed();
        assert_eq!(tracker.rows().len(), 1);
        tracker.update(&frame("new-live", 2, at + Duration::from_secs(5), 20));
        tracker.restore_closed(removed);
        let rows = tracker.rows();
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().any(|row| row.id == id && !row.active));
        assert!(rows.iter().any(|row| row.active));
    }
    #[test]
    fn closed_connection_history_is_bounded() {
        let mut tracker = Tracker::default();
        let at = Instant::now();
        for index in 0..1005 {
            tracker.update(&frame(
                &index.to_string(),
                1,
                at + Duration::from_secs(index),
                0,
            ));
        }
        assert_eq!(tracker.rows().len(), 1001);
        assert_eq!(
            tracker.rows().iter().filter(|row| !row.active).count(),
            1000
        );
    }
}
