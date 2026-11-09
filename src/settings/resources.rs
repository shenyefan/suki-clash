use super::{FieldControl, SettingsPage};
use crate::{
    configuration::resources::DATABASES,
    core, ui,
    ui::theme::{control, preferences, space, text},
};
use gpui_kit::{
    assets::IconName,
    component::{Disableable, Sizable, button::Button, h_flex, input::Input, v_flex},
    prelude::FluentBuilder,
    *,
};

impl SettingsPage {
    pub(super) fn refresh_resources(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.session.read(cx).core_busy() || !self.session.read(cx).is_running() {
            return;
        }
        let generation = self.session.read(cx).core().generation();
        if self.resource_generation == Some(generation) {
            return;
        }
        // A failed submission is still this generation's automatic attempt.
        // Session sampling must not turn it into a repeated toast/request.
        self.resource_generation = Some(generation);
        match core::submit(core::Command::Resources, window, cx) {
            Ok(_) => {}
            Err(error) => ui::show_error(error, window, cx),
        }
    }
    pub(super) fn update_resources(
        &mut self,
        name: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let command = match self.runtime_tab {
            0 => core::Command::UpdateGeo,
            tab => core::Command::UpdateProviders {
                kind: if tab == 1 {
                    core::ProviderKind::Proxies
                } else {
                    core::ProviderKind::Rules
                },
                name,
                generation: self.session.read(cx).core().generation(),
            },
        };
        core::request(command, window, cx);
    }
    fn update_database(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(database) = crate::configuration::resources::Database::from_key(key) {
            core::request(core::Command::UpdateDatabase(database), window, cx);
        }
    }
    pub(super) fn resource_content(
        &self,
        query: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        if self.runtime_tab != 0 {
            return self.provider_content(query, window, cx);
        }
        let mut body = v_flex().gap(space::SECTION);
        let keys: Vec<_> = ["GeoAutoUpdate", "GeoUpdateIntervalValue", "GeoMode"]
            .into_iter()
            .filter(|key| {
                let spec = &self.fields[key].spec;
                query.is_empty()
                    || format!("更新设置 {}", spec.label)
                        .to_lowercase()
                        .contains(query)
            })
            .collect();
        let mut found = !keys.is_empty();
        if found {
            body = body.child(ui::preferences_group(
                "更新设置",
                keys.into_iter()
                    .map(|key| self.field_row(key, cx))
                    .collect(),
                cx,
            ));
        }
        let snapshot = self.session.read(cx).core();
        let disabled = self.saving || self.session.read(cx).core_busy();
        for (key, _, name, file) in DATABASES {
            if !format!("{name} {file}").to_lowercase().contains(query) {
                continue;
            }
            found = true;
            let FieldControl::Text(state) = &self.fields[key].control else {
                continue;
            };
            let status = snapshot
                .resources()
                .files()
                .iter()
                .find(|status| status.key() == key);
            let metadata = match status {
                Some(status) if status.size().is_some() => {
                    format!("最后更新：{}", status.modified().unwrap_or("—"))
                }
                Some(_) => "未下载".into(),
                None => "最后更新：—".into(),
            };
            let source = Input::new(state)
                .with_size(control::STANDARD)
                .w(preferences::URL_WIDTH)
                .disabled(disabled)
                .aria_label(format!("{name} 下载地址"));
            let update = h_flex()
                .gap(space::LG)
                .child(ui::muted(metadata, cx).text_size(text::CAPTION))
                .child(
                    Button::new(SharedString::from(format!("resource-update-{key}")))
                        .outline()
                        .icon(IconName::RefreshCw)
                        .with_size(control::STANDARD)
                        .label("现在更新")
                        .disabled(disabled)
                        .accessibility_label(format!("更新 {name} 数据库"))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.update_database(key, window, cx)
                        })),
                );
            body = body.child(ui::preferences_group(
                format!("{name} 数据库"),
                vec![
                    ui::data_preference_row("URL", "", source, cx).into_any_element(),
                    ui::data_preference_row("数据库更新", "", update, cx).into_any_element(),
                ],
                cx,
            ));
        }
        if !found {
            return ui::empty(IconName::Search, "无匹配资源", "", cx);
        }
        body
    }
    fn provider_content(&self, query: &str, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let session = self.session.read(cx);
        let kind = if self.runtime_tab == 1 {
            core::ProviderKind::Proxies
        } else {
            core::ProviderKind::Rules
        };
        if !session.is_running() {
            return ui::empty(IconName::FolderDown, "内核未运行", "", cx);
        }
        let resources = session.core().resources();
        let providers: Vec<_> = resources
            .providers(kind)
            .iter()
            .filter(|provider| provider.name().to_lowercase().contains(query))
            .cloned()
            .collect();
        if providers.is_empty() {
            return ui::empty(
                IconName::FolderDown,
                if query.is_empty() {
                    "暂无资源集合"
                } else {
                    "无匹配资源"
                },
                "",
                cx,
            );
        }
        let row_height = ui::theme::list::DETAIL_ROW_HEIGHT.to_pixels(window.rem_size());
        let heights = providers
            .iter()
            .map(|provider| {
                row_height
                    + if provider.quota().is_some() {
                        (space::LG + space::MD).to_pixels(window.rem_size())
                    } else {
                        px(0.)
                    }
            })
            .collect();
        let providers = std::rc::Rc::new(providers);
        let rows = ui::virtual_rows::rows(
            cx.entity(),
            "provider-rows",
            heights,
            &self.lists["providers"],
            move |this, index, _, cx| {
                let provider = &providers[index];
                let name = provider.name().to_owned();
                let updated = provider
                    .updated()
                    .and_then(|time| chrono::DateTime::parse_from_rfc3339(time).ok())
                    .map(|time| {
                        time.with_timezone(&chrono::Local)
                            .format("%Y-%m-%d %H:%M")
                            .to_string()
                    })
                    .unwrap_or_else(|| "—".into());
                let details = format!(
                    "{} · {} {}{} · {}",
                    provider.vehicle(),
                    provider.count(),
                    if kind == core::ProviderKind::Proxies {
                        "个节点"
                    } else {
                        "条规则"
                    },
                    provider
                        .behavior()
                        .map(|behavior| format!(" · {behavior}"))
                        .unwrap_or_default(),
                    updated
                );
                let controls = h_flex()
                    .gap(space::SM)
                    .when(provider.uncertain(), |row| {
                        row.child(ui::muted("待确认", cx))
                    })
                    .child(
                        Button::new(SharedString::from(format!(
                            "provider-update-{}-{name}",
                            this.runtime_tab
                        )))
                        .icon(IconName::RefreshCw)
                        .with_size(control::STANDARD)
                        .label("更新")
                        .accessibility_label(format!("更新 {name}"))
                        .disabled(
                            this.saving
                                || this.session.read(cx).core_busy()
                                || !provider.can_update(),
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.update_resources(Some(name.clone()), window, cx)
                            },
                        )),
                    );
                let mut row = v_flex().child(ui::data_preference_row(
                    provider.name().to_owned(),
                    details,
                    controls,
                    cx,
                ));
                if let Some(quota) = provider.quota() {
                    row = row.child(
                        ui::muted(quota, cx)
                            .px(space::LG)
                            .pb(space::MD)
                            .text_size(text::CAPTION),
                    );
                }
                row.into_any_element()
            },
        );
        ui::preferences_list(kind.label(), rows, cx)
    }
}
