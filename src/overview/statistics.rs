//! Bounded counter-based traffic accounting. Attribution is explicitly sampled.
use crate::storage::{AppPaths, atomic_write};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub(crate) struct Connections {
    #[serde(rename = "uploadTotal")]
    pub(crate) upload_total: u64,
    #[serde(rename = "downloadTotal")]
    pub(crate) download_total: u64,
    #[serde(default, deserialize_with = "empty_connections")]
    pub(crate) connections: Vec<Connection>,
}
// Mihomo returns null when its active connection slice is empty.
fn empty_connections<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<Connection>, D::Error> {
    Ok(Option::<Vec<Connection>>::deserialize(deserializer)?.unwrap_or_default())
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub(crate) struct Connection {
    pub(crate) id: String,
    pub(crate) upload: u64,
    pub(crate) download: u64,
    #[serde(default)]
    pub(crate) chains: Vec<String>,
    #[serde(default, rename = "rulePayload")]
    pub(crate) rule_payload: String,
    pub(crate) metadata: Metadata,
    #[serde(default)]
    pub(crate) start: String,
    #[serde(default)]
    pub(crate) rule: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub(crate) struct Metadata {
    #[serde(default, rename = "sourceIP")]
    pub(crate) source_ip: String,
    #[serde(default, rename = "sourcePort")]
    pub(crate) source_port: String,
    #[serde(default, rename = "destinationPort")]
    pub(crate) destination_port: String,
    #[serde(default)]
    pub(crate) network: String,
    #[serde(default, rename = "type")]
    pub(crate) kind: String,
    #[serde(default)]
    pub(crate) host: String,
    #[serde(default, rename = "destinationIP")]
    pub(crate) destination_ip: String,
    #[serde(default, rename = "remoteDestination")]
    pub(crate) remote_destination: String,
    #[serde(default)]
    pub(crate) process: String,
    #[serde(default, rename = "processPath")]
    pub(crate) process_path: String,
}
impl Connection {
    fn is_direct(&self) -> Option<bool> {
        self.chains.first().map(|name| name == "DIRECT")
    }
    fn keys(&self) -> [String; 3] {
        let process = if !self.metadata.process.is_empty() {
            self.metadata.process.clone()
        } else {
            self.metadata
                .process_path
                .rsplit(['/', '\\'])
                .next()
                .filter(|name| !name.is_empty())
                .unwrap_or("未知进程")
                .into()
        };
        let domain = if self.metadata.host.is_empty() {
            if self.metadata.destination_ip.is_empty() {
                "未知目标".into()
            } else {
                self.metadata.destination_ip.clone()
            }
        } else {
            self.metadata.host.clone()
        };
        let policy = self
            .chains
            .last()
            .cloned()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| {
                if self.rule_payload.is_empty() {
                    "未知策略".into()
                } else {
                    self.rule_payload.clone()
                }
            });
        [process, domain, policy]
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Totals {
    pub(crate) upload: u64,
    pub(crate) download: u64,
    pub(crate) direct: u64,
    pub(crate) proxy: u64,
    pub(crate) unclassified: u64,
}
impl Totals {
    pub(crate) fn bytes(&self) -> u64 {
        self.upload.saturating_add(self.download)
    }
    fn add(&mut self, other: &Self) {
        self.upload = self.upload.saturating_add(other.upload);
        self.download = self.download.saturating_add(other.download);
        self.direct = self.direct.saturating_add(other.direct);
        self.proxy = self.proxy.saturating_add(other.proxy);
        self.unclassified = self.unclassified.saturating_add(other.unclassified);
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Hour {
    totals: Totals,
    all: [BTreeMap<String, u64>; 3],
    proxy: [BTreeMap<String, u64>; 3],
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct History {
    schema_version: u32,
    hours: BTreeMap<String, Hour>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Point {
    pub(crate) time: String,
    pub(crate) upload: u64,
    pub(crate) download: u64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Summary {
    pub(crate) today: Totals,
    pub(crate) month: Totals,
    pub(crate) hours: Vec<(String, u64, u64)>,
    pub(crate) rankings: Vec<Vec<(String, u64)>>,
    pub(crate) proxy_rankings: Vec<Vec<(String, u64)>>,
    pub(crate) upload_rate: Option<u64>,
    pub(crate) download_rate: Option<u64>,
    pub(crate) connections: Option<usize>,
    pub(crate) processes: usize,
    pub(crate) direct_connections: usize,
    pub(crate) proxy_connections: usize,
    pub(crate) rates: Vec<Point>,
    pub(crate) gaps: u64,
}
pub(crate) struct Recorder {
    history: History,
    baseline: Option<(u64, u64, u64, std::time::Instant)>,
    connections: BTreeMap<String, (u64, u64)>,
    rates: VecDeque<Point>,
    gaps: u64,
    dirty: bool,
}
impl Default for Recorder {
    fn default() -> Self {
        Self {
            history: History {
                schema_version: 1,
                hours: BTreeMap::new(),
            },
            baseline: None,
            connections: BTreeMap::new(),
            rates: VecDeque::new(),
            gaps: 0,
            dirty: false,
        }
    }
}
impl Recorder {
    fn path(paths: &AppPaths) -> std::path::PathBuf {
        paths.data().join("statistics/traffic.json")
    }
    pub(crate) fn load(paths: &AppPaths) -> Result<Self, String> {
        let bytes = match std::fs::read(Self::path(paths)) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(_) => return Err("流量统计文件读取失败".into()),
        };
        if bytes.len() > 8 * 1024 * 1024 {
            return Err("流量统计文件超过上限".into());
        }
        let history: History =
            serde_json::from_slice(&bytes).map_err(|_| "流量统计文件无效".to_owned())?;
        if history.schema_version != 1
            || history.hours.len() > 840
            || history.hours.keys().any(|key| {
                chrono::NaiveDateTime::parse_from_str(&format!("{key}:00:00"), "%Y-%m-%d %H:%M:%S")
                    .is_err()
            })
        {
            return Err("流量统计版本或时间无效".into());
        }
        Ok(Self {
            history,
            ..Self::default()
        })
    }
    pub(crate) fn save(&mut self, paths: &AppPaths) -> Result<(), String> {
        if !self.dirty {
            return Ok(());
        }
        let path = Self::path(paths);
        std::fs::create_dir_all(path.parent().ok_or("统计目录无效")?)
            .map_err(|_| "无法创建统计目录")?;
        let mut bytes = serde_json::to_vec(&self.history).map_err(|_| "无法编码流量统计")?;
        if bytes.len() > 8 * 1024 * 1024 {
            for hour in self.history.hours.values_mut() {
                hour.all = Default::default();
                hour.proxy = Default::default();
            }
            bytes = serde_json::to_vec(&self.history).map_err(|_| "无法编码流量统计")?;
        }
        atomic_write(&path, &bytes).map_err(|_| "流量统计保存失败")?;
        self.dirty = false;
        Ok(())
    }
    pub(crate) fn gap(&mut self) {
        self.baseline = None;
        self.connections.clear();
        self.rates.clear();
        self.gaps = self.gaps.saturating_add(1);
    }
    pub(crate) fn sample(
        &mut self,
        generation: u64,
        current: &Connections,
        time: chrono::DateTime<chrono::Local>,
        now: std::time::Instant,
    ) -> Summary {
        let old = self.baseline.replace((
            generation,
            current.upload_total,
            current.download_total,
            now,
        ));
        let mut delta = Totals::default();
        let mut rate = None;
        if let Some((old_generation, up, down, instant)) = old {
            if generation == old_generation
                && current.upload_total >= up
                && current.download_total >= down
            {
                delta.upload = current.upload_total - up;
                delta.download = current.download_total - down;
                let millis = now.saturating_duration_since(instant).as_millis();
                if millis > 0 && millis <= 5000 {
                    rate = Some((
                        (delta.upload as u128 * 1000 / millis).min(u64::MAX as u128) as u64,
                        (delta.download as u128 * 1000 / millis).min(u64::MAX as u128) as u64,
                    ));
                } else {
                    self.gaps = self.gaps.saturating_add(1);
                }
            } else {
                self.gaps = self.gaps.saturating_add(1);
                self.connections.clear();
            }
        }
        let mut attributed = 0u64;
        let mut entries = Vec::new();
        for connection in &current.connections {
            let previous = self.connections.get(&connection.id).copied();
            let amount = if old.is_none() {
                0
            } else {
                connection
                    .upload
                    .saturating_sub(previous.map_or(0, |value| value.0))
                    .saturating_add(
                        connection
                            .download
                            .saturating_sub(previous.map_or(0, |value| value.1)),
                    )
            };
            let amount = amount.min(delta.bytes().saturating_sub(attributed));
            attributed = attributed.saturating_add(amount);
            match connection.is_direct() {
                Some(true) => delta.direct = delta.direct.saturating_add(amount),
                Some(false) => delta.proxy = delta.proxy.saturating_add(amount),
                None => {}
            }
            entries.push((connection.keys(), connection.is_direct(), amount));
        }
        delta.unclassified = delta
            .bytes()
            .saturating_sub(delta.direct.saturating_add(delta.proxy));
        self.connections = current
            .connections
            .iter()
            .map(|connection| {
                (
                    connection.id.clone(),
                    (connection.upload, connection.download),
                )
            })
            .collect();
        let hour = time.format("%Y-%m-%d %H").to_string();
        let bucket = self.history.hours.entry(hour).or_default();
        bucket.totals.add(&delta);
        for (keys, direct, bytes) in entries {
            if bytes == 0 {
                continue;
            }
            for (index, key) in keys.into_iter().enumerate() {
                add_rank(&mut bucket.all[index], key.clone(), bytes);
                if direct == Some(false) {
                    add_rank(&mut bucket.proxy[index], key, bytes);
                }
            }
        }
        while self.history.hours.len() > 35 * 24 {
            self.history.hours.pop_first();
        }
        self.dirty = true;
        let mut summary = self.summary(time);
        summary.upload_rate = rate.map(|rate| rate.0);
        summary.download_rate = rate.map(|rate| rate.1);
        summary.connections = Some(current.connections.len());
        summary.processes = current
            .connections
            .iter()
            .map(|connection| connection.keys()[0].clone())
            .collect::<BTreeSet<_>>()
            .len();
        summary.direct_connections = current
            .connections
            .iter()
            .filter(|connection| connection.is_direct() == Some(true))
            .count();
        summary.proxy_connections = current
            .connections
            .iter()
            .filter(|connection| connection.is_direct() == Some(false))
            .count();
        summary
    }
    pub(crate) fn record_rate(
        &mut self,
        time: chrono::DateTime<chrono::Local>,
        upload: u64,
        download: u64,
    ) -> Vec<Point> {
        self.rates.push_back(Point {
            time: time.format("%H:%M:%S").to_string(),
            upload,
            download,
        });
        while self.rates.len() > 120 {
            self.rates.pop_front();
        }
        self.rates.iter().cloned().collect()
    }
    pub(crate) fn summary(&self, time: chrono::DateTime<chrono::Local>) -> Summary {
        let day = time.format("%Y-%m-%d").to_string();
        let month = time.format("%Y-%m").to_string();
        let mut result = Summary {
            rates: self.rates.iter().cloned().collect(),
            gaps: self.gaps,
            ..Summary::default()
        };
        let mut all: [BTreeMap<String, u64>; 3] = Default::default();
        let mut proxy = all.clone();
        for (key, hour) in &self.history.hours {
            if key.starts_with(&month) {
                result.month.add(&hour.totals);
            }
            if key.starts_with(&day) {
                result.today.add(&hour.totals);
                for index in 0..3 {
                    for (name, bytes) in &hour.all[index] {
                        add_rank(&mut all[index], name.clone(), *bytes);
                    }
                    for (name, bytes) in &hour.proxy[index] {
                        add_rank(&mut proxy[index], name.clone(), *bytes);
                    }
                }
            }
        }
        result.hours = (0..24)
            .map(|hour| {
                let key = format!("{day} {hour:02}");
                let totals = self
                    .history
                    .hours
                    .get(&key)
                    .map(|hour| hour.totals.clone())
                    .unwrap_or_default();
                (format!("{hour:02}:00"), totals.bytes(), totals.proxy)
            })
            .collect();
        result.rankings = all.into_iter().map(ranking).collect();
        result.proxy_rankings = proxy.into_iter().map(ranking).collect();
        result
    }
}
fn add_rank(map: &mut BTreeMap<String, u64>, key: String, bytes: u64) {
    let key = if key.len() > 256 || (map.len() >= 32 && !map.contains_key(&key)) {
        "其他".into()
    } else {
        key
    };
    let value = map.entry(key).or_default();
    *value = value.saturating_add(bytes);
}
fn ranking(map: BTreeMap<String, u64>) -> Vec<(String, u64)> {
    let mut values = map.into_iter().collect::<Vec<_>>();
    values.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    values.truncate(8);
    values
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    #[test]
    fn counts_real_deltas_and_keeps_unobserved_connections_unclassified() {
        let mut recorder = Recorder::default();
        let time = chrono::Local::now();
        let instant = Instant::now();
        let mut sample = Connections {
            upload_total: 100,
            download_total: 200,
            ..Default::default()
        };
        assert_eq!(recorder.sample(1, &sample, time, instant).upload_rate, None);
        sample.upload_total += 100;
        sample.download_total += 300;
        sample.connections.push(Connection {
            id: "new".into(),
            upload: 20,
            download: 80,
            chains: vec!["DIRECT".into()],
            metadata: Metadata {
                host: "example.test".into(),
                process: "browser".into(),
                ..Default::default()
            },
            ..Default::default()
        });
        let result = recorder.sample(1, &sample, time, instant + Duration::from_secs(2));
        assert_eq!(result.upload_rate, Some(50));
        assert_eq!(result.download_rate, Some(150));
        assert_eq!(result.today.bytes(), 400);
        assert_eq!(result.today.direct, 100);
        assert_eq!(result.today.unclassified, 300);
        assert_eq!(result.rankings[1], vec![("example.test".into(), 100)]);
        let duplicate = recorder.sample(1, &sample, time, instant + Duration::from_secs(3));
        assert_eq!(duplicate.today.bytes(), 400);
        assert_eq!(duplicate.upload_rate, Some(0));
        sample.upload_total = 0;
        sample.download_total = 0;
        assert_eq!(
            recorder
                .sample(1, &sample, time, instant + Duration::from_secs(4))
                .upload_rate,
            None
        );
        sample.upload_total = 2000;
        assert_eq!(
            recorder
                .sample(2, &sample, time, instant + Duration::from_secs(5))
                .today
                .bytes(),
            400
        );
        recorder.gap();
        sample.upload_total = 9000;
        let resumed = recorder.sample(2, &sample, time, instant + Duration::from_secs(6));
        assert_eq!(resumed.today.bytes(), 400);
        assert_eq!(resumed.upload_rate, None);
        assert!(resumed.gaps >= 3);
    }
    #[test]
    fn persists_history_without_reusing_previous_process_counters() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        let mut recorder = Recorder::default();
        let time = chrono::Local::now();
        let now = Instant::now();
        let mut sample = Connections::default();
        recorder.sample(1, &sample, time, now);
        sample.download_total = 1024;
        recorder.sample(1, &sample, time, now + Duration::from_secs(1));
        recorder.save(&paths).unwrap();
        let mut restored = Recorder::load(&paths).unwrap();
        assert_eq!(restored.summary(time).today.bytes(), 1024);
        sample.download_total = 4096;
        assert_eq!(
            restored
                .sample(1, &sample, time, now + Duration::from_secs(2))
                .today
                .bytes(),
            1024
        );
    }
}
