use super::{FieldControl, SettingsPage};
use crate::{
    ui,
    ui::theme::{control, preferences, space, text},
};
use gpui_kit::{
    assets::IconName,
    component::{Disableable, Sizable, button::Button, h_flex, input::Input, v_flex},
    *,
};

const DATABASES: [(&str, &str, &str, &str); 4] = [
    ("GeoIpUrl", "GeoIP 数据库", "geoip.dat", "IP 地理位置数据"),
    (
        "GeoSiteUrl",
        "GeoSite 数据库",
        "geosite.dat",
        "域名分类数据",
    ),
    (
        "MmdbUrl",
        "MMDB 数据库",
        "country.mmdb",
        "IP 国家与地区数据",
    ),
    ("AsnUrl", "ASN 数据库", "ASN.mmdb", "自治系统编号数据"),
];

impl SettingsPage {
    pub(super) fn resource_content(&self, query: &str, cx: &mut Context<Self>) -> Div {
        if self.runtime_tab != 0 {
            return self.provider_content(query, cx);
        }
        let mut body = v_flex().gap(space::SECTION);
        let update_keys: Vec<_> = ["GeoMode", "GeoAutoUpdate", "GeoUpdateIntervalValue"]
            .into_iter()
            .filter(|key| {
                let spec = &self.fields[key].spec;
                query.is_empty()
                    || format!("更新设置 {} {}", spec.label, spec.description)
                        .to_lowercase()
                        .contains(query)
            })
            .collect();
        let mut found = !update_keys.is_empty();
        if found {
            let rows = update_keys
                .into_iter()
                .map(|key| self.field_row(key, cx))
                .collect();
            body = body.child(ui::preferences_group("更新设置", rows, cx));
        }
        for (key, name, file, description) in DATABASES {
            if !format!("{name} {file} {description}")
                .to_lowercase()
                .contains(query)
            {
                continue;
            }
            found = true;
            let FieldControl::Text(state) = &self.fields[key].control else {
                continue;
            };
            let source = Input::new(state)
                .with_size(control::STANDARD)
                .w(preferences::URL_WIDTH)
                .aria_label(format!("{name}下载地址"));
            let update = h_flex()
                .gap(space::LG)
                .child(
                    ui::muted(
                        if self.resource_pending.contains(key) {
                            "等待处理"
                        } else {
                            "最后更新：—"
                        },
                        cx,
                    )
                    .text_size(text::CAPTION),
                )
                .child(
                    Button::new(SharedString::from(format!("resource-update-{key}")))
                        .with_size(control::STANDARD)
                        .label(if self.resource_pending.contains(key) {
                            "已提交"
                        } else {
                            "现在更新"
                        })
                        .disabled(self.resource_pending.contains(key))
                        .accessibility_label(format!("更新 {name}"))
                        .on_click(cx.listener(move |this, _, w, cx| {
                            if let Some(error) =
                                super::schema::validate(&this.sections, &this.command_values(cx))
                            {
                                ui::show_error(error, w, cx);
                                return;
                            }
                            this.field_action(key, this.fields[key].spec.actions[0], w, cx);
                        })),
                );
            body = body.child(ui::preferences_group(
                name,
                vec![
                    ui::preference_row("URL", format!("{description} · {file}"), source, cx)
                        .into_any_element(),
                    ui::preference_row("数据库更新", "", update, cx).into_any_element(),
                ],
                cx,
            ));
        }
        if !found {
            return ui::empty(IconName::Search, "无匹配资源", "尝试其他关键词", cx);
        }
        body
    }

    fn provider_content(&self, query: &str, cx: &mut Context<Self>) -> Div {
        ui::empty(
            IconName::FolderDown,
            if query.is_empty() {
                "暂无外部资源"
            } else {
                "无匹配资源"
            },
            "加载配置后显示实际资源集合",
            cx,
        )
    }
}
