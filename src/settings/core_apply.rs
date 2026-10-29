//! Per-field submissions; the lifecycle owner merges each patch with committed settings.
use super::{FieldControl, SettingsPage};
use crate::{storage::Values, ui};
use gpui_kit::*;
impl SettingsPage {
    pub(super) fn restore_default_text(
        &mut self,
        key: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let before = self.values(cx)[key].clone();
        let default = self.fields[key].spec.default.to_owned();
        if before == default {
            return;
        }
        if let FieldControl::Multiline(state) = &self.fields[key].control {
            state.update(cx, |state, cx| state.set_value(default.clone(), window, cx));
        }
        self.commit_core_field(key, window, cx);
        let owner = cx.weak_entity();
        ui::notifications::undo(window, "已恢复默认内容", cx, move |window, cx| {
            owner
                .update(cx, |this, cx| {
                    if this.saving || this.session.read(cx).core_busy() {
                        ui::show_error("正在应用配置，请稍后撤销".into(), window, cx);
                        return false;
                    }
                    if this.values(cx).get(key) != Some(&default) {
                        ui::show_error("内容已更改，无法撤销此次恢复".into(), window, cx);
                        return false;
                    }
                    if let FieldControl::Multiline(state) = &this.fields[key].control {
                        state.update(cx, |state, cx| state.set_value(before.clone(), window, cx));
                    }
                    this.commit_core_field(key, window, cx);
                    true
                })
                .unwrap_or(true)
        });
    }
    pub(super) fn commit_core_field(
        &mut self,
        key: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.core_syncing {
            return;
        }
        if self.kind != super::SettingsKind::Core
            && self
                .fields
                .get(key)
                .is_some_and(|field| field.spec.kind == super::schema::FieldKind::Secret)
        {
            ui::show_error("凭据安全存储尚未接入，未保存该字段".into(), window, cx);
            return;
        }
        let values = self.values(cx);
        if let Some(value) = values.get(key) {
            self.commit_core(Values::from([(key.into(), value.clone())]), window, cx);
        }
    }
    pub(super) fn commit_core(
        &mut self,
        mut patch: Values,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.core_syncing {
            return;
        }
        if matches!(
            self.kind,
            super::SettingsKind::Dns | super::SettingsKind::Sniffer
        ) {
            if self.dns_raw {
                if let Err(error) = self.read_dns_yaml(window, cx) {
                    ui::show_error(error, window, cx);
                    return;
                }
                patch = self.values(cx);
            } else {
                let initial = if self.dns_document.is_null() {
                    crate::preferences::native::initial(self.kind)
                } else {
                    Ok(self.dns_document.clone())
                };
                match initial
                    .and_then(|root| crate::preferences::native::patch(self.kind, &root, &patch))
                {
                    Ok(root) => self.dns_document = root,
                    Err(error) => {
                        ui::show_error(error, window, cx);
                        return;
                    }
                }
            }
            let yaml =
                serde_yaml_ng::to_string(&self.dns_document).map_err(|_| "无法编码配置".to_owned());
            match yaml {
                Ok(yaml) => {
                    patch.insert(
                        if self.kind == super::SettingsKind::Dns {
                            "DnsYaml"
                        } else {
                            "SnifferYaml"
                        }
                        .into(),
                        yaml,
                    );
                }
                Err(error) => {
                    ui::show_error(error, window, cx);
                    return;
                }
            }
        }
        let committed = crate::integration::read(self.kind.id(), cx);
        let patch: Values = patch
            .into_iter()
            .filter(|(k, v)| {
                if self.kind == super::SettingsKind::SysProxy && k == "IsSystemProxyEnabled" {
                    self.session.read(cx).system_proxy() != Some(v == "true")
                } else {
                    k == "WebUiConfigured" || committed.get(k) != Some(v)
                }
            })
            .collect();
        if patch.is_empty()
            || self
                .core_pending
                .as_ref()
                .is_some_and(|(_, pending, _)| pending == &patch)
        {
            return;
        }
        let command = crate::core::Command::Settings {
            kind: self.kind,
            patch: patch.clone(),
        };
        match crate::core::submit(command, window, cx) {
            Ok(id) => {
                self.saving = true;
                self.core_pending = Some((id, patch, window.window_handle()));
                self.error = None;
            }
            Err(error) => {
                self.error = Some(error.clone());
                ui::show_error(error, window, cx);
            }
        }
        cx.notify();
    }
    pub(super) fn restore_core_fields(
        &mut self,
        touched: &Values,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let values = crate::integration::read(self.kind.id(), cx);
        self.core_syncing = true;
        for key in touched.keys() {
            let Some(field) = self.fields.get_mut(key.as_str()) else {
                continue;
            };
            let value = values
                .get(key)
                .map(String::as_str)
                .unwrap_or(field.spec.default);
            match &mut field.control {
                FieldControl::Toggle(checked) => *checked = value == "true",
                FieldControl::Text(state) => {
                    state.update(cx, |state, cx| state.set_value(value, window, cx))
                }
                FieldControl::Multiline(state) => {
                    state.update(cx, |state, cx| state.set_value(value, window, cx))
                }
                FieldControl::Select(state) => {
                    let value = SharedString::from(value.to_owned());
                    state.update(cx, |state, cx| {
                        if key == "CoreSpecificVersion" {
                            state.set_items(vec![value.clone()], window, cx);
                        }
                        state.set_selected_value(&value, window, cx);
                    });
                }
                FieldControl::ReadOnly => {}
            }
        }
        if touched.keys().any(|k| k.starts_with("WebUi")) {
            self.panels = super::schema::panels_from_values(&values);
        }
        if matches!(
            self.kind,
            super::SettingsKind::Dns | super::SettingsKind::Sniffer
        ) {
            self.dns_document = values
                .get(if self.kind == super::SettingsKind::Dns {
                    "DnsYaml"
                } else {
                    "SnifferYaml"
                })
                .and_then(|yaml| serde_yaml_ng::from_str(yaml).ok())
                .unwrap_or_default();
            let yaml = self.dns_to_yaml(cx);
            if let Ok(yaml) = yaml {
                self.dns_source
                    .update(cx, |state, cx| state.set_value(yaml, window, cx));
            }
        }
        if self.kind == super::SettingsKind::General {
            match values.get("ThemeIndex").map(String::as_str) {
                Some("深色") => gpui_kit::component::Theme::change(
                    gpui_kit::component::ThemeMode::Dark,
                    Some(window),
                    cx,
                ),
                Some("浅色") => gpui_kit::component::Theme::change(
                    gpui_kit::component::ThemeMode::Light,
                    Some(window),
                    cx,
                ),
                _ => gpui_kit::component::Theme::sync_system_appearance(Some(window), cx),
            }
            ui::configure_theme(cx);
            cx.emit(super::SettingsSaved {
                backdrop: values
                    .get("BackdropIndex")
                    .cloned()
                    .unwrap_or_else(|| "原生磨砂".into()),
            });
        }
        self.core_syncing = false;
        self.dirty = false;
        cx.notify();
    }
    pub(super) fn commit_panels(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut values = Values::new();
        super::schema::store_panels(&self.panels, &mut values);
        self.commit_core(values, window, cx);
    }
}
