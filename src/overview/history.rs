//! Application-owned traffic ledger. The monitor is its single sampling and disk owner.
use super::statistics::Connections;
use crate::{proxies::runtime::Proxies, storage::atomic_write};
use chrono::{DateTime, Datelike, Local, NaiveDate, Timelike};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Instant,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Bytes {
    pub(crate) upload: u64,
    pub(crate) download: u64,
}
impl Bytes {
    pub(crate) fn total(self) -> u64 {
        self.upload.saturating_add(self.download)
    }
    fn add(&mut self, other: Self) {
        self.upload = self.upload.saturating_add(other.upload);
        self.download = self.download.saturating_add(other.download);
    }
    fn difference(self, previous: Self) -> Self {
        Self {
            upload: self.upload.saturating_sub(previous.upload),
            download: self.download.saturating_sub(previous.download),
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Hour {
    pub(crate) all: Bytes,
    pub(crate) proxy: Bytes,
    pub(crate) samples: u64,
}
impl Hour {
    pub(crate) fn bytes(self, proxy_only: bool) -> Bytes {
        if proxy_only { self.proxy } else { self.all }
    }
    fn add(&mut self, other: Self) {
        self.all.add(other.all);
        self.proxy.add(other.proxy);
        self.samples = self.samples.saturating_add(other.samples);
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub(crate) available: bool,
    pub(crate) hours: [Hour; 24],
    pub(crate) today: Hour,
    pub(crate) month: Hour,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    days: BTreeMap<String, [Hour; 24]>,
}
impl Default for Document {
    fn default() -> Self {
        Self {
            version: 1,
            days: BTreeMap::new(),
        }
    }
}
impl Document {
    fn read(path: &Path) -> Result<Self, String> {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(format!("{}：{error}", path.display())),
        };
        let document: Self = serde_json::from_slice(&bytes)
            .map_err(|error| format!("{}：{error}", path.display()))?;
        if document.version != 1
            || document.days.keys().any(|date| {
                !NaiveDate::parse_from_str(date, "%Y-%m-%d")
                    .is_ok_and(|parsed| parsed.to_string() == *date)
            })
        {
            return Err(format!("{}：流量记录版本或日期无效", path.display()));
        }
        Ok(document)
    }
    fn merge(&mut self, other: &Self) {
        for (date, hours) in &other.days {
            let target = self
                .days
                .entry(date.clone())
                .or_insert([Hour::default(); 24]);
            for (target, source) in target.iter_mut().zip(hours) {
                target.add(*source);
            }
        }
    }
    fn prune(&mut self, today: NaiveDate) {
        let month = today.format("%Y-%m-").to_string();
        self.days.retain(|date, _| date.starts_with(&month));
    }
}

pub(crate) struct Ledger {
    path: PathBuf,
    saved: Option<Document>,
    pending: Document,
    error: Option<String>,
    baseline: Option<(Instant, Bytes)>,
    proxy_names: BTreeMap<String, bool>,
}
impl Ledger {
    pub(crate) async fn open(path: PathBuf) -> Self {
        let source = path.clone();
        let result = tokio::task::spawn_blocking(move || Document::read(&source))
            .await
            .map_err(|error| format!("{}：{error}", path.display()))
            .and_then(|result| result);
        let (saved, error) = match result {
            Ok(mut document) => {
                document.prune(Local::now().date_naive());
                (Some(document), None)
            }
            Err(error) => (None, Some(error)),
        };
        Self {
            path,
            saved,
            pending: Document::default(),
            error,
            baseline: None,
            proxy_names: BTreeMap::new(),
        }
    }
    pub(crate) fn error(&self) -> Option<String> {
        self.error.clone()
    }
    pub(crate) fn gap(&mut self) {
        self.baseline = None;
    }
    pub(crate) fn set_proxies(&mut self, proxies: &Proxies) {
        self.proxy_names = proxies
            .entries
            .values()
            .chain(proxies.records.values())
            .map(|proxy| {
                (
                    proxy.name.clone(),
                    !matches!(
                        proxy.kind.as_str(),
                        "Direct" | "Reject" | "RejectDrop" | "Pass" | "Compatible"
                    ) && proxy.all.is_none(),
                )
            })
            .collect();
    }
    pub(crate) fn sample(
        &mut self,
        time: DateTime<Local>,
        at: Instant,
        current: &Connections,
        previous: Option<&BTreeMap<String, (u64, u64)>>,
    ) {
        let total = Bytes {
            upload: current.upload_total,
            download: current.download_total,
        };
        if let Some((previous_at, previous_total)) = &self.baseline
            && at > *previous_at
            && at.duration_since(*previous_at).as_secs() <= 15
            && total.upload >= previous_total.upload
            && total.download >= previous_total.download
        {
            let mut proxy = Bytes::default();
            for connection in &current.connections {
                let Some(exit) = connection.chains.first() else {
                    continue;
                };
                if !self.proxy_names.get(exit).copied().unwrap_or(false) {
                    continue;
                }
                let bytes = Bytes {
                    upload: connection.upload,
                    download: connection.download,
                };
                if let Some((upload, download)) =
                    previous.and_then(|values| values.get(&connection.id))
                {
                    proxy.add(bytes.difference(Bytes {
                        upload: *upload,
                        download: *download,
                    }));
                } else {
                    // A new connection is attributed only when created in the observed interval.
                    let initial = DateTime::parse_from_rfc3339(&connection.start)
                        .ok()
                        .is_some_and(|start| {
                            start.with_timezone(&Local)
                                >= time
                                    - chrono::Duration::milliseconds(
                                        at.duration_since(*previous_at).as_millis() as i64,
                                    )
                        });
                    if initial {
                        proxy.add(bytes);
                    }
                }
            }
            let day = self
                .pending
                .days
                .entry(time.date_naive().to_string())
                .or_insert([Hour::default(); 24]);
            day[time.hour() as usize].add(Hour {
                all: total.difference(*previous_total),
                proxy,
                samples: 1,
            });
            self.pending.prune(time.date_naive());
        }
        // Replayed packets cannot move the counter baseline backwards.
        if self.baseline.as_ref().is_none_or(|(last, _)| at > *last) {
            self.baseline = Some((at, total));
        }
    }
    pub(crate) fn snapshot(&self, now: DateTime<Local>) -> Snapshot {
        let date = now.date_naive().to_string();
        let mut snapshot = Snapshot {
            available: self.saved.is_some(),
            ..Default::default()
        };
        let month = format!("{:04}-{:02}-", now.year(), now.month());
        for document in self.saved.iter().chain(std::iter::once(&self.pending)) {
            for (day, hours) in &document.days {
                if !day.starts_with(&month) {
                    continue;
                }
                for (index, hour) in hours.iter().enumerate() {
                    snapshot.month.add(*hour);
                    if day == &date {
                        snapshot.hours[index].add(*hour);
                        snapshot.today.add(*hour);
                    }
                }
            }
        }
        snapshot
    }
    pub(crate) async fn flush(&mut self, now: DateTime<Local>) {
        if self.pending.days.is_empty() && self.saved.is_some() && self.error.is_none() {
            return;
        }
        let path = self.path.clone();
        let saved = self.saved.clone();
        let pending = self.pending.clone();
        let result = tokio::task::spawn_blocking(move || {
            let mut document = match saved {
                Some(document) => document,
                None => Document::read(&path)?,
            };
            document.merge(&pending);
            document.prune(now.date_naive());
            let bytes = serde_json::to_vec(&document)
                .map_err(|error| format!("{}：{error}", path.display()))?;
            atomic_write(&path, &bytes).map_err(|error| format!("{}：{error}", path.display()))?;
            Ok::<_, String>(document)
        })
        .await
        .map_err(|error| format!("{}：{error}", self.path.display()))
        .and_then(|result| result);
        match result {
            Ok(document) => {
                self.saved = Some(document);
                self.pending = Document::default();
                self.error = None;
            }
            Err(error) => {
                self.error = Some(error);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overview::statistics::Connection;
    use chrono::TimeZone;
    use std::time::Duration;
    struct TestLedger {
        ledger: Ledger,
        recorder: super::super::statistics::Recorder,
    }
    impl std::ops::Deref for TestLedger {
        type Target = Ledger;
        fn deref(&self) -> &Ledger {
            &self.ledger
        }
    }
    impl std::ops::DerefMut for TestLedger {
        fn deref_mut(&mut self) -> &mut Ledger {
            &mut self.ledger
        }
    }
    impl TestLedger {
        fn sample(&mut self, time: DateTime<Local>, at: Instant, current: &Connections) {
            self.ledger
                .sample(time, at, current, self.recorder.previous_connections());
            self.recorder.sample_connections(current);
        }
        fn gap(&mut self) {
            self.ledger.gap();
            self.recorder.connection_gap();
        }
    }
    async fn open(path: PathBuf) -> TestLedger {
        TestLedger {
            ledger: Ledger::open(path).await,
            recorder: Default::default(),
        }
    }
    fn time(day: u32, hour: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(2026, 11, day, hour, 0, 0)
            .single()
            .unwrap()
    }
    fn fixture(upload: u64, download: u64) -> Connections {
        Connections {
            upload_total: upload,
            download_total: download,
            connections: vec![
                Connection {
                    id: "proxy".into(),
                    upload: upload / 2,
                    download: download / 2,
                    chains: vec!["exit".into(), "group".into()],
                    ..Default::default()
                },
                Connection {
                    id: "direct".into(),
                    upload: upload / 2,
                    download: download / 2,
                    chains: vec!["custom-direct".into(), "group".into()],
                    ..Default::default()
                },
            ],
        }
    }
    fn proxies() -> Proxies {
        use crate::proxies::runtime::Proxy;
        let mut proxies = Proxies::default();
        proxies.entries = [
            (
                "exit".into(),
                Proxy {
                    name: "exit".into(),
                    kind: "Shadowsocks".into(),
                    ..Default::default()
                },
            ),
            (
                "custom-direct".into(),
                Proxy {
                    name: "custom-direct".into(),
                    kind: "Direct".into(),
                    ..Default::default()
                },
            ),
        ]
        .into();
        proxies
    }
    #[tokio::test]
    async fn durable_deltas_restore_without_recounting_core_totals_and_exclude_direct() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("cache/traffic.json");
        let mut ledger = open(path.clone()).await;
        ledger.set_proxies(&proxies());
        let at = Instant::now();
        let now = time(8, 11);
        ledger.sample(now, at, &fixture(100, 200));
        ledger.sample(now, at + Duration::from_secs(1), &fixture(140, 280));
        ledger.sample(now, at + Duration::from_secs(1), &fixture(140, 280));
        let snapshot = ledger.snapshot(now);
        assert_eq!(
            snapshot.today.all,
            Bytes {
                upload: 40,
                download: 80
            }
        );
        assert_eq!(
            snapshot.today.proxy,
            Bytes {
                upload: 20,
                download: 40
            }
        );
        assert_eq!(snapshot.hours[11].samples, 1);
        assert_eq!(snapshot.hours[12].samples, 0);
        ledger.flush(now).await;
        assert!(ledger.error().is_none());
        let mut restored = open(path).await;
        restored.set_proxies(&proxies());
        assert_eq!(restored.snapshot(now), snapshot);
        restored.sample(now, at + Duration::from_secs(2), &fixture(300, 600));
        assert_eq!(restored.snapshot(now), snapshot);
        restored.sample(now, at + Duration::from_secs(3), &fixture(320, 640));
        assert_eq!(
            restored.snapshot(now).today.all,
            Bytes {
                upload: 60,
                download: 120
            }
        );
        restored.flush(now).await;
        restored.flush(now).await;
        assert_eq!(
            restored.snapshot(now).today.all,
            Bytes {
                upload: 60,
                download: 120
            }
        );
    }
    #[tokio::test]
    async fn gaps_resets_and_replayed_old_packets_do_not_backfill_or_double_count() {
        let root = tempfile::tempdir().unwrap();
        let mut ledger = open(root.path().join("traffic.json")).await;
        let at = Instant::now();
        let now = time(8, 10);
        ledger.sample(now, at, &fixture(100, 200));
        ledger.sample(now, at + Duration::from_secs(1), &fixture(120, 240));
        ledger.sample(now, at, &fixture(10, 20));
        ledger.sample(now, at + Duration::from_secs(2), &fixture(130, 260));
        ledger.gap();
        ledger.sample(now, at + Duration::from_secs(3), &fixture(500, 1000));
        ledger.sample(now, at + Duration::from_secs(4), &fixture(510, 1020));
        ledger.sample(now, at + Duration::from_secs(5), &fixture(0, 0));
        ledger.sample(now, at + Duration::from_secs(6), &fixture(10, 20));
        ledger.sample(now, at + Duration::from_secs(40), &fixture(900, 1800));
        ledger.sample(now, at + Duration::from_secs(41), &fixture(910, 1820));
        assert_eq!(
            ledger.snapshot(now).today.all,
            Bytes {
                upload: 60,
                download: 120
            }
        );
    }
    #[tokio::test]
    async fn local_hour_day_and_month_rollover_keep_independent_totals() {
        let root = tempfile::tempdir().unwrap();
        let mut ledger = open(root.path().join("traffic.json")).await;
        let at = Instant::now();
        let oct = Local
            .with_ymd_and_hms(2026, 10, 31, 23, 59, 59)
            .single()
            .unwrap();
        ledger.sample(oct - chrono::Duration::seconds(1), at, &fixture(0, 0));
        ledger.sample(oct, at + Duration::from_secs(1), &fixture(10, 20));
        ledger.sample(time(1, 0), at + Duration::from_secs(2), &fixture(30, 60));
        ledger.gap();
        ledger.sample(time(2, 0), at + Duration::from_secs(3), &fixture(30, 60));
        ledger.sample(time(2, 1), at + Duration::from_secs(4), &fixture(60, 120));
        let snapshot = ledger.snapshot(time(2, 1));
        assert_eq!(
            snapshot.today.all,
            Bytes {
                upload: 30,
                download: 60
            }
        );
        assert_eq!(
            snapshot.month.all,
            Bytes {
                upload: 50,
                download: 100
            }
        );
        assert_eq!(snapshot.hours[0].samples, 0);
        assert_eq!(snapshot.hours[1].all.upload, 30);
        ledger.flush(time(2, 1)).await;
        let loaded = open(root.path().join("traffic.json")).await;
        assert_eq!(loaded.snapshot(time(2, 1)), snapshot);
    }
    #[tokio::test]
    async fn damaged_file_is_preserved_and_repair_merges_pending_data_once() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("traffic.json");
        std::fs::write(&path, b"{broken").unwrap();
        let mut ledger = open(path.clone()).await;
        assert!(!ledger.snapshot(time(8, 11)).available);
        assert!(ledger.error().unwrap().contains("line 1"));
        let at = Instant::now();
        let now = time(8, 11);
        ledger.sample(now, at, &fixture(0, 0));
        ledger.sample(now, at + Duration::from_secs(1), &fixture(10, 20));
        ledger.flush(now).await;
        assert_eq!(std::fs::read(&path).unwrap(), b"{broken");
        std::fs::write(&path, serde_json::to_vec(&Document::default()).unwrap()).unwrap();
        ledger.flush(now).await;
        assert!(ledger.error().is_none());
        assert!(ledger.snapshot(now).available);
        assert_eq!(
            ledger.snapshot(now).today.all,
            Bytes {
                upload: 10,
                download: 20
            }
        );
        ledger.flush(now).await;
        assert_eq!(open(path).await.snapshot(now).today.all.upload, 10);
    }
    #[tokio::test]
    async fn failed_write_keeps_pending_bytes_and_retries_without_double_counting() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("cache/traffic.json");
        let mut ledger = open(path.clone()).await;
        let at = Instant::now();
        let now = time(8, 11);
        ledger.sample(now, at, &fixture(0, 0));
        ledger.sample(now, at + Duration::from_secs(1), &fixture(10, 20));
        std::fs::write(root.path().join("cache"), b"not a directory").unwrap();
        ledger.flush(now).await;
        assert!(ledger.error().is_some());
        assert_eq!(ledger.snapshot(now).today.all.upload, 10);
        std::fs::remove_file(root.path().join("cache")).unwrap();
        ledger.flush(now).await;
        assert!(ledger.error().is_none());
        assert_eq!(open(path).await.snapshot(now).today.all.upload, 10);
    }
    #[tokio::test]
    async fn new_connections_count_only_bytes_started_in_the_observed_interval() {
        let root = tempfile::tempdir().unwrap();
        let mut ledger = open(root.path().join("traffic.json")).await;
        ledger.set_proxies(&proxies());
        let at = Instant::now();
        let now = time(8, 11);
        ledger.sample(now, at, &Connections::default());
        let mut sample = fixture(100, 200);
        sample.connections[0].start = (now + chrono::Duration::milliseconds(500)).to_rfc3339();
        ledger.sample(
            now + chrono::Duration::seconds(1),
            at + Duration::from_secs(1),
            &sample,
        );
        assert_eq!(
            ledger.snapshot(now).today.proxy,
            Bytes {
                upload: 50,
                download: 100
            }
        );
    }
    #[test]
    fn records_reject_invalid_dates_versions_and_types_and_bound_retention() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("traffic.json");
        for text in [
            r#"{"version":2,"days":{}}"#,
            r#"{"version":1,"days":{"oops":[]}}"#,
            r#"{"version":1,"days":[],"other":0}"#,
        ] {
            std::fs::write(&path, text).unwrap();
            assert!(Document::read(&path).is_err());
        }
        let mut document = Document::default();
        let now = time(8, 11).date_naive();
        for days in 0..500 {
            document.days.insert(
                (now - chrono::Duration::days(days)).to_string(),
                [Hour::default(); 24],
            );
        }
        document.prune(now);
        assert_eq!(document.days.len(), 8);
    }
    #[test]
    fn full_month_storage_and_aggregation_are_bounded() {
        let hour = Hour {
            all: Bytes {
                upload: u64::MAX,
                download: u64::MAX,
            },
            proxy: Bytes {
                upload: u64::MAX,
                download: u64::MAX,
            },
            samples: u64::MAX,
        };
        let mut document = Document::default();
        for day in 1..=31 {
            document
                .days
                .insert(format!("2026-01-{day:02}"), [hour; 24]);
        }
        let encoded = serde_json::to_vec(&document).unwrap();
        assert!(encoded.len() < 192 * 1024, "{} bytes", encoded.len());
        assert!(size_of::<Hour>() * 24 * 31 * 2 < 64 * 1024);
        document.prune(NaiveDate::from_ymd_opt(2026, 2, 1).unwrap());
        assert!(document.days.is_empty());
    }
}
