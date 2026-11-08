//! Mihomo connection snapshots and bounded, session-only traffic samples.
use serde::Deserialize;
use std::collections::{BTreeMap, VecDeque};

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
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Point {
    pub(crate) time: String,
    pub(crate) upload: u64,
    pub(crate) download: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Summary {
    pub(crate) upload_rate: Option<u64>,
    pub(crate) download_rate: Option<u64>,
    pub(crate) connections: Option<usize>,
    pub(crate) rates: Vec<Point>,
    pub(crate) rankings: [Vec<(String, u64)>; 3],
}

/// Recent rates are observational data, not a cross-session traffic ledger.
#[derive(Default)]
pub(crate) struct Recorder {
    rates: VecDeque<(chrono::DateTime<chrono::Local>, Point)>,
    scope: Option<(u64, u64)>,
    baseline: Option<BTreeMap<String, (u64, u64)>>,
    rankings: [BTreeMap<String, u64>; 3],
}
impl Recorder {
    pub(crate) fn set_scope(&mut self, scope: Option<(u64, u64)>) {
        if self.scope == scope {
            return;
        }
        if self.scope.map(|scope| scope.0) != scope.map(|scope| scope.0) {
            self.rankings = Default::default();
        }
        self.scope = scope;
        self.baseline = None;
        self.rates.clear();
    }

    pub(crate) fn previous_connections(&self) -> Option<&BTreeMap<String, (u64, u64)>> {
        self.baseline.as_ref()
    }

    pub(crate) fn connection_gap(&mut self) {
        self.baseline = None;
    }

    /// Only observed active-connection differences are attributed. The first
    /// snapshot after a discontinuity establishes a baseline instead of backfill.
    pub(crate) fn sample_connections(&mut self, current: &Connections) -> [Vec<(String, u64)>; 3] {
        if let Some(previous) = &self.baseline {
            for connection in &current.connections {
                let (upload, download) = previous.get(&connection.id).copied().unwrap_or_default();
                let bytes = connection
                    .upload
                    .saturating_sub(upload)
                    .saturating_add(connection.download.saturating_sub(download));
                if bytes == 0 {
                    continue;
                }
                let process = if connection.metadata.process.is_empty() {
                    connection
                        .metadata
                        .process_path
                        .rsplit(['/', '\\'])
                        .next()
                        .filter(|name| !name.is_empty())
                        .unwrap_or("未知进程")
                } else {
                    &connection.metadata.process
                };
                let domain = [
                    &connection.metadata.host,
                    &connection.metadata.destination_ip,
                    &connection.metadata.remote_destination,
                ]
                .into_iter()
                .find(|value| !value.is_empty())
                .map(String::as_str)
                .unwrap_or("未知目标");
                let proxy = connection
                    .chains
                    .first()
                    .map(String::as_str)
                    .unwrap_or("未知代理");
                for (index, name) in [domain, process, proxy].into_iter().enumerate() {
                    let values = &mut self.rankings[index];
                    let name = if values.len() >= 1000 && !values.contains_key(name) {
                        "其他"
                    } else {
                        name
                    };
                    let total = values.entry(name.to_owned()).or_default();
                    *total = total.saturating_add(bytes);
                }
            }
        }
        self.baseline = Some(
            current
                .connections
                .iter()
                .map(|connection| {
                    (
                        connection.id.clone(),
                        (connection.upload, connection.download),
                    )
                })
                .collect(),
        );
        self.rankings()
    }

    pub(crate) fn rankings(&self) -> [Vec<(String, u64)>; 3] {
        self.rankings.each_ref().map(|values| {
            let mut values = values
                .iter()
                .map(|(name, bytes)| (name.clone(), *bytes))
                .collect::<Vec<_>>();
            values.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            values.truncate(8);
            values
        })
    }

    pub(crate) fn gap(&mut self) {
        self.rates.clear();
    }

    pub(crate) fn record_rate(
        &mut self,
        time: chrono::DateTime<chrono::Local>,
        upload: u64,
        download: u64,
    ) -> Vec<Point> {
        self.rates.push_back((
            time,
            Point {
                time: time.format("%H:%M:%S").to_string(),
                upload,
                download,
            },
        ));
        let cutoff = time - chrono::Duration::seconds(30);
        while self.rates.len() > 600 || self.rates.front().is_some_and(|(time, _)| *time <= cutoff)
        {
            self.rates.pop_front();
        }
        self.rates.iter().map(|(_, point)| point.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connections(bytes: u64) -> Connections {
        Connections {
            connections: vec![Connection {
                id: "connection".into(),
                upload: bytes,
                chains: vec!["出口节点".into(), "策略组".into()],
                metadata: Metadata {
                    host: "example.test".into(),
                    process: "browser".into(),
                    ..Default::default()
                },
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn session_rankings_count_differences_once_and_attribute_real_exit() {
        let mut recorder = Recorder::default();
        recorder.set_scope(Some((1, 1)));
        assert!(recorder.sample_connections(&connections(100))[0].is_empty());
        let rankings = recorder.sample_connections(&connections(150));
        assert_eq!(rankings[0], vec![("example.test".into(), 50)]);
        assert_eq!(rankings[1], vec![("browser".into(), 50)]);
        assert_eq!(rankings[2], vec![("出口节点".into(), 50)]);
        assert_eq!(recorder.sample_connections(&connections(150)), rankings);
        assert_eq!(recorder.sample_connections(&connections(0)), rankings);
        assert_eq!(recorder.sample_connections(&connections(10))[0][0].1, 60);
    }

    #[test]
    fn ranking_discontinuities_do_not_backfill_and_core_restart_clears_session() {
        let mut recorder = Recorder::default();
        recorder.set_scope(Some((1, 1)));
        recorder.sample_connections(&connections(0));
        recorder.sample_connections(&connections(50));
        recorder.connection_gap();
        assert_eq!(recorder.sample_connections(&connections(1000))[0][0].1, 50);
        assert_eq!(recorder.sample_connections(&connections(1020))[0][0].1, 70);
        recorder.set_scope(Some((1, 2)));
        assert_eq!(recorder.sample_connections(&connections(2000))[0][0].1, 70);
        assert_eq!(recorder.sample_connections(&connections(2010))[0][0].1, 80);
        recorder.set_scope(Some((2, 1)));
        assert!(recorder.rankings()[0].is_empty());
        assert!(recorder.sample_connections(&connections(3000))[0].is_empty());
        recorder.sample_connections(&connections(3010));
        recorder.set_scope(None);
        assert!(recorder.rankings()[0].is_empty());
    }

    #[test]
    fn ranking_keys_are_bounded_and_overflow_is_explicitly_grouped() {
        let mut recorder = Recorder::default();
        recorder.set_scope(Some((1, 1)));
        recorder.sample_connections(&Connections::default());
        let mut current = Connections::default();
        for index in 0..1100 {
            current.connections.push(Connection {
                id: index.to_string(),
                upload: 1,
                metadata: Metadata {
                    host: format!("{index}.test"),
                    ..Default::default()
                },
                ..Default::default()
            });
        }
        recorder.sample_connections(&current);
        assert_eq!(recorder.rankings[0].len(), 1001);
        assert_eq!(recorder.rankings[0]["其他"], 100);
    }

    #[test]
    fn traffic_window_expires_old_samples_and_gap_does_not_fabricate_zeroes() {
        let mut recorder = Recorder::default();
        let start = chrono::Local::now();
        recorder.record_rate(start, 10, 20);
        let points = recorder.record_rate(start + chrono::Duration::seconds(30), 30, 40);
        assert_eq!(points.len(), 1);
        assert_eq!((points[0].upload, points[0].download), (30, 40));
        recorder.gap();
        let points = recorder.record_rate(start + chrono::Duration::seconds(31), 50, 60);
        assert_eq!(points.len(), 1);
        assert_eq!((points[0].upload, points[0].download), (50, 60));
    }

    #[test]
    fn traffic_window_rolls_forward_with_both_rates_for_the_last_thirty_seconds() {
        let mut recorder = Recorder::default();
        let start = chrono::Local::now();
        for second in 0..65 {
            let points = recorder.record_rate(
                start + chrono::Duration::seconds(second),
                second as u64,
                second as u64 * 2,
            );
            let first = (second - 29).max(0) as u64;
            assert_eq!(points.len(), (second + 1).min(30) as usize);
            assert_eq!((points[0].upload, points[0].download), (first, first * 2));
            assert_eq!(points.last().unwrap().upload, second as u64);
            assert_eq!(points.last().unwrap().download, second as u64 * 2);
        }
    }

    #[test]
    fn traffic_window_stays_bounded_when_core_sends_faster_samples() {
        let mut recorder = Recorder::default();
        let start = chrono::Local::now();
        for index in 0..700 {
            recorder.record_rate(
                start + chrono::Duration::milliseconds(index),
                index as u64,
                0,
            );
        }
        assert_eq!(recorder.rates.len(), 600);
        assert_eq!(recorder.rates.front().unwrap().1.upload, 100);
        assert_eq!(recorder.rates.back().unwrap().1.upload, 699);
    }
}
