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
    pub(crate) fn matches(&self, text: &str) -> bool {
        self.matcher
            .as_ref()
            .is_none_or(|matcher| matcher.is_match(text))
    }
}
