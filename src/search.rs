//! Shared session-only text matching.
use regex::{Regex, RegexBuilder};
#[derive(Default)]
pub(crate) struct Search {
    pub(crate) case_sensitive: bool,
    pub(crate) whole_word: bool,
    pub(crate) regular_expression: bool,
    pub(crate) matcher: Option<Regex>,
    pub(crate) error: Option<String>,
}
impl Search {
    pub(crate) fn compile(&mut self, query: &str) {
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
        let pattern = if self.whole_word && !self.regular_expression {
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
                self.matcher = None;
            }
        }
    }
    pub(crate) fn matches(&self, text: &str) -> bool {
        self.error.is_none()
            && self
                .matcher
                .as_ref()
                .is_none_or(|matcher| matcher.is_match(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_regex_matches_nothing_and_recovers_after_editing() {
        let mut search = Search {
            regular_expression: true,
            ..Default::default()
        };
        search.compile("[");
        assert!(search.error.is_some());
        assert!(!search.matches("a [ literal"));
        search.compile("a+");
        assert!(search.error.is_none());
        assert!(search.matches("BAAA"));
        search.compile("");
        assert!(search.matches("anything"));
    }
    #[test]
    fn regex_takes_priority_over_whole_word() {
        let mut search = Search {
            regular_expression: true,
            whole_word: true,
            ..Default::default()
        };
        search.compile("foo");
        assert!(search.matches("foobar"));
        search.regular_expression = false;
        search.compile("foo");
        assert!(!search.matches("foobar"));
        assert!(search.matches("FOO bar"));
        search.case_sensitive = true;
        search.compile("foo");
        assert!(!search.matches("FOO bar"));
    }
}
