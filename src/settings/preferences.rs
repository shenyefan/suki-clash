use super::{SettingsKind, SettingsPage, schema};
use crate::{ui, ui::theme::space};
use gpui_kit::{assets::IconName, component::v_flex, *};

impl SettingsPage {
    pub(super) fn preferences_content(&self, query: &str, cx: &mut Context<Self>) -> Div {
        let groups: Vec<_> = self
            .sections
            .iter()
            .filter_map(|section| {
                let keys: Vec<_> = section
                    .fields
                    .iter()
                    .filter(|field| {
                        (query.is_empty()
                            && schema::section_tab(self.kind, section.title, field.key)
                                == self.runtime_tab
                            || !query.is_empty()
                                && format!(
                                    "{} {} {}",
                                    section.title, field.label, field.description
                                )
                                .to_lowercase()
                                .contains(query))
                            && schema::field_visible(self.kind, field.key)
                            && !(self.kind == SettingsKind::Tun && field.key == "IsTunEnabled")
                            && !(matches!(self.kind, SettingsKind::Dns | SettingsKind::Sniffer)
                                && field.key == crate::preferences::native::flag(self.kind))
                    })
                    .map(|field| field.key)
                    .collect();
                (!keys.is_empty()).then_some((section.title, keys))
            })
            .collect();
        let mut body = v_flex().gap(space::SECTION);
        if groups.is_empty() {
            return ui::empty(IconName::Search, "未找到设置项", "尝试其他关键词", cx);
        }
        for (title, keys) in groups {
            let rows = keys
                .into_iter()
                .map(|key| self.field_row(key, cx))
                .collect();
            let title =
                if self.kind == SettingsKind::General && title == "常规" && self.runtime_tab == 1
                {
                    "外观"
                } else {
                    title
                };
            body = body.child(ui::preferences_group(title, rows, cx));
        }
        body
    }
}
