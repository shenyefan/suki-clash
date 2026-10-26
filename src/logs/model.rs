use regex::{Regex, RegexBuilder};

pub(super) const MAX_LOGS: usize = 1000;

#[derive(Clone)]
pub(crate) struct LogEntry {
    pub(crate) id: u64,
    pub(crate) time: String,
    pub(crate) level: String,
    pub(crate) source: usize,
    pub(crate) message: String,
}
impl LogEntry {
    pub(super) fn text(&self) -> String {
        format!("{}  {}  {}", self.time, self.level, self.message)
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
            self.next_id += 1;
            entry.id = self.next_id;
            self.entries.push(entry);
        }
        if self.entries.len() > MAX_LOGS {
            self.entries.drain(..self.entries.len() - MAX_LOGS);
        }
    }
    pub(super) fn clear(&mut self, source: usize) {
        self.entries.retain(|entry| entry.source != source);
    }
}

#[derive(Default)]
pub(super) struct LogSearch {
    pub(super) case_sensitive: bool,
    pub(super) whole_word: bool,
    pub(super) regular_expression: bool,
    pub(super) matcher: Option<Regex>,
    pub(super) error: Option<String>,
}
impl LogSearch {
    pub(super) fn compile(&mut self, query: &str) {
        self.error = None;
        if query.is_empty() {
            self.matcher = None;
            return;
        }
        let pattern = if self.regular_expression {
            query.to_owned()
        } else {
            regex::escape(query)
        };
        let pattern = if self.whole_word {
            format!(r"\b(?:{pattern})\b")
        } else {
            pattern
        };
        match RegexBuilder::new(&pattern)
            .case_insensitive(!self.case_sensitive)
            .build()
        {
            Ok(matcher) => self.matcher = Some(matcher),
            Err(error) => {
                self.error = Some(format!("正则表达式无效：{error}"));
                // Match Clash Verge's literal fallback while the expression is incomplete.
                self.matcher = RegexBuilder::new(&regex::escape(query))
                    .case_insensitive(!self.case_sensitive)
                    .build()
                    .ok();
            }
        }
    }
    pub(super) fn matches(&self, entry: &LogEntry) -> bool {
        self.matcher
            .as_ref()
            .is_none_or(|matcher| matcher.is_match(&entry.text()))
    }
}

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
        message: message.into(),
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retention_pause_and_clear_preserve_other_source() {
        let mut buffer = LogBuffer::default();
        for _ in 0..120 {
            buffer.append(initial_entries());
        }
        assert_eq!(buffer.entries.len(), MAX_LOGS);
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
    fn search_combines_case_word_regex_and_literal_fallback() {
        let entry = &initial_entries()[4];
        let mut search = LogSearch::default();
        search.compile("mixed");
        assert!(search.matches(entry));
        search.case_sensitive = true;
        search.compile("mixed");
        assert!(!search.matches(entry));
        search.case_sensitive = false;
        search.whole_word = true;
        search.compile("mix");
        assert!(!search.matches(entry));
        search.regular_expression = true;
        search.compile("Mixed.*listening");
        assert!(search.matches(entry));
        search.compile("[");
        assert!(search.error.is_some());
        search.whole_word = false;
        search.compile("代理组");
        assert!(search.matches(&initial_entries()[2]));
    }
}
