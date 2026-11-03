pub(super) const MAX_LOGS: usize = 1000;

#[derive(Clone)]
pub(crate) struct LogEntry {
    pub(crate) id: u64,
    pub(crate) time: String,
    pub(crate) level: String,
    pub(crate) source: usize,
    pub(crate) origin: String,
    pub(crate) message: String,
}
impl LogEntry {
    pub(super) fn time_key(&self) -> (Option<chrono::DateTime<chrono::FixedOffset>>, u64) {
        (
            chrono::DateTime::parse_from_rfc3339(&self.time).ok(),
            self.id,
        )
    }
    pub(super) fn text(&self) -> String {
        format!(
            "{}  {}  [{}]  {}",
            self.time, self.level, self.origin, self.message
        )
    }
}

#[derive(Default)]
pub(super) struct LogBuffer {
    pub(super) entries: Vec<LogEntry>,
    pub(super) paused: bool,
    next_id: u64,
}
impl LogBuffer {
    pub(crate) fn append(&mut self, incoming: impl IntoIterator<Item = LogEntry>) {
        if self.paused {
            return;
        }
        for mut entry in incoming {
            entry.level = match entry.level.to_ascii_lowercase().as_str() {
                "warn" | "warning" => "WARNING".into(),
                "err" | "error" => "ERROR".into(),
                level => level.to_ascii_uppercase(),
            };
            self.next_id = self.next_id.max(entry.id);
            if entry.id == 0 {
                self.next_id += 1;
                entry.id = self.next_id;
            }
            self.entries.push(entry);
        }
        for source in [0, 1] {
            let mut excess = self
                .entries
                .iter()
                .filter(|entry| entry.source == source)
                .count()
                .saturating_sub(MAX_LOGS);
            self.entries.retain(|entry| {
                if entry.source == source && excess > 0 {
                    excess -= 1;
                    false
                } else {
                    true
                }
            });
        }
    }
    pub(super) fn restore(&mut self, entries: Vec<LogEntry>) {
        self.entries.extend(entries);
        self.entries.sort_by_key(|entry| entry.id);
        self.entries.dedup_by_key(|entry| entry.id);
        let paused = self.paused;
        self.paused = false;
        self.append([]);
        self.paused = paused;
    }
    pub(super) fn clear(&mut self, source: usize) {
        self.entries.retain(|entry| entry.source != source);
    }
}

pub(super) use crate::search::Search as LogSearch;

#[cfg(test)]
pub(super) fn initial_entries() -> Vec<LogEntry> {
    [
        ("18:42:06", "INFO", "应用工作区已准备就绪", 1),
        ("18:42:06", "INFO", "已加载订阅 Suki Cloud", 1),
        ("18:42:07", "DEBUG", "代理组：节点选择、自动选择、流媒体", 1),
        ("18:42:08", "WARNING", "等待内核事件", 1),
        (
            "18:42:06",
            "INFO",
            "Mixed proxy listening at: 127.0.0.1:7890",
            0,
        ),
        ("18:42:07", "INFO", "[TCP] github.com:443 → 节点选择", 0),
        (
            "18:42:08",
            "DEBUG",
            "[DNS] resolve github.com from nameserver",
            0,
        ),
        ("18:42:09", "WARNING", "[UDP] connection timeout", 0),
        (
            "18:42:10",
            "ERROR",
            "[DNS] upstream resolver unavailable",
            0,
        ),
    ]
    .into_iter()
    .map(|(time, level, message, source)| LogEntry {
        id: 0,
        time: time.into(),
        level: level.into(),
        source,
        origin: if source == 0 { "mihomo" } else { "application" }.into(),
        message: message.into(),
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn undo_clear_preserves_new_records_pause_and_bounded_history() {
        let mut buffer = LogBuffer::default();
        buffer.append(initial_entries());
        let removed = buffer
            .entries
            .iter()
            .filter(|entry| entry.source == 0)
            .cloned()
            .collect::<Vec<_>>();
        buffer.clear(0);
        buffer.append(initial_entries());
        let new_id = buffer.entries.last().unwrap().id;
        buffer.paused = true;
        buffer.restore(removed.clone());
        buffer.restore(removed);
        assert!(buffer.paused);
        assert!(buffer.entries.iter().any(|entry| entry.id == new_id));
        let ids = buffer
            .entries
            .iter()
            .map(|entry| entry.id)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(ids.len(), buffer.entries.len());
        assert!(buffer.entries.iter().any(|entry| entry.source == 0));
    }
    #[test]
    fn retention_pause_and_clear_preserve_other_source() {
        let mut buffer = LogBuffer::default();
        for _ in 0..240 {
            buffer.append(initial_entries());
        }
        assert_eq!(buffer.entries.len(), 1960);
        assert!(
            buffer
                .entries
                .iter()
                .filter(|entry| entry.source == 0)
                .count()
                <= MAX_LOGS
        );
        let last = buffer.entries.last().unwrap().id;
        buffer.paused = true;
        buffer.append(initial_entries());
        assert_eq!(buffer.entries.last().unwrap().id, last);
        buffer.clear(0);
        assert!(buffer.entries.iter().all(|entry| entry.source == 1));
        buffer.paused = false;
        buffer.append(initial_entries());
        assert!(buffer.entries.last().unwrap().id > last);
    }
    #[test]
    fn runtime_level_aliases_match_the_filter_options() {
        let mut buffer = LogBuffer::default();
        for level in ["warn", "warning", "err", "error", "info", "debug"] {
            let mut entry = initial_entries().remove(0);
            entry.level = level.into();
            buffer.append([entry]);
        }
        let levels: Vec<_> = buffer
            .entries
            .iter()
            .map(|entry| entry.level.as_str())
            .collect();
        assert_eq!(
            levels,
            ["WARNING", "WARNING", "ERROR", "ERROR", "INFO", "DEBUG"]
        );
    }
    #[test]
    fn search_combines_case_word_and_regex_without_literal_fallback() {
        let entry = &initial_entries()[4];
        let mut search = LogSearch::default();
        search.compile("mixed");
        assert!(search.matches(&entry.text()));
        search.case_sensitive = true;
        search.compile("mixed");
        assert!(!search.matches(&entry.text()));
        search.case_sensitive = false;
        search.whole_word = true;
        search.compile("mix");
        assert!(!search.matches(&entry.text()));
        search.regular_expression = true;
        search.compile("Mixed.*listening");
        assert!(search.matches(&entry.text()));
        search.compile("[");
        assert!(search.error.is_some());
        assert!(!search.matches("["));
        search.whole_word = false;
        search.compile("代理组");
        assert!(search.matches(&initial_entries()[2].text()));
    }
}
