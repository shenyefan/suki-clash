use super::SettingsPage;
use crate::ui;
use gpui_kit::*;

// UI field names and Mihomo document keys stay at the document boundary.
impl SettingsPage {
    pub(super) fn dns_to_yaml(&self, _cx: &App) -> Result<String, String> {
        let root = if self.dns_document.is_null() {
            crate::preferences::native::initial(self.kind)?
        } else {
            self.dns_document.clone()
        };
        serde_yaml_ng::to_string(&root).map_err(|_| "无法编码 YAML 配置".into())
    }
    pub(super) fn read_dns_yaml(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let mut values = crate::preferences::native::decode(
            self.kind,
            self.dns_source.read(cx).value().as_bytes(),
        )?;
        let flag = crate::preferences::native::flag(self.kind);
        values.insert(
            flag.into(),
            self.values(cx)
                .get(flag)
                .cloned()
                .unwrap_or_else(|| "false".into()),
        );
        let key = crate::preferences::native::key(self.kind).ok_or("不是 YAML 设置")?;
        self.dns_document =
            serde_yaml_ng::from_str(&values[key]).map_err(|_| "无法读取 YAML 文档")?;
        self.core_syncing = true;
        self.assign_values(&crate::preferences::choices::decode(values), window, cx);
        self.core_syncing = false;
        Ok(())
    }
    fn dns_form_changes(&self, cx: &App) -> Result<crate::storage::Values, String> {
        let root = if self.dns_document.is_null() {
            crate::preferences::native::initial(self.kind)?
        } else {
            self.dns_document.clone()
        };
        let form = crate::preferences::choices::encode(self.values(cx));
        let enabled = form
            .get(crate::preferences::native::flag(self.kind))
            .is_some_and(|value| value == "true");
        let projected = crate::preferences::native::project(self.kind, &root, enabled)?;
        Ok(form
            .into_iter()
            .filter(|(key, value)| projected.get(key) != Some(value))
            .collect())
    }
    pub(super) fn switch_dns_mode(
        &mut self,
        raw: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if raw == self.dns_raw {
            return;
        }
        let result = if raw {
            self.dns_form_changes(cx).and_then(|changes| {
                if !changes.is_empty() {
                    let root = if self.dns_document.is_null() {
                        crate::preferences::native::initial(self.kind)?
                    } else {
                        self.dns_document.clone()
                    };
                    self.dns_document =
                        crate::preferences::native::patch(self.kind, &root, &changes)?;
                    self.commit_core(changes, window, cx);
                }
                self.dns_to_yaml(cx).map(|source| {
                    self.dns_source
                        .update(cx, |state, cx| state.set_value(source, window, cx))
                })
            })
        } else {
            self.read_dns_yaml(window, cx).map(|()| {
                // A mode-button click may precede the editor's blur event.
                self.commit_core(self.values(cx), window, cx);
            })
        };
        match result {
            Ok(()) => self.dns_raw = raw,
            Err(error) => ui::show_error(error, window, cx),
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use crate::configuration::flow::{get, mapping_text, parse_mapping, put};
    use serde_yaml_ng::Value;
    #[test]
    fn hosts_and_policy_roundtrip_multiple_values_and_ipv6() {
        let text = "example.com=::1;127.0.0.1\nalias.test=example.com";
        assert_eq!(mapping_text(&parse_mapping(text).unwrap()).unwrap(), text);
        assert!(parse_mapping("bad-line").is_err());
    }
    #[test]
    fn updating_known_dns_fields_preserves_unknown_fields() {
        let mut root: Value = serde_yaml_ng::from_str(
            "dns:\n  cache-algorithm: arc\n  fallback-filter:\n    geosite: [gfw]\n",
        )
        .unwrap();
        put(&mut root, "dns.fallback-filter.geoip", Value::Bool(true));
        assert_eq!(
            get(&root, "dns.cache-algorithm").unwrap().as_str(),
            Some("arc")
        );
        assert!(
            get(&root, "dns.fallback-filter.geosite")
                .unwrap()
                .is_sequence()
        );
    }
}

#[cfg(test)]
mod synchronization_tests {
    use crate::settings::{FieldControl, SettingsKind, SettingsPage};
    use gpui_kit::component::WindowExt;
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{AppContext, Bounds, TestAppContext, WindowBounds, WindowOptions, px, size};
    #[gpui_kit::test]
    fn form_switches_and_yaml_share_the_complete_persisted_document(cx: &mut TestAppContext) {
        let root = tempfile::tempdir().unwrap();
        let paths = crate::storage::AppPaths::isolated(root.path());
        let repository = crate::storage::Repository::open(paths).unwrap();
        for (kind, source) in [
            (
                SettingsKind::Dns,
                "dns: {enable: true, ipv6: true, nameserver: ['1.1.1.1'], cache-algorithm: arc}",
            ),
            (
                SettingsKind::Sniffer,
                "sniffer: {enable: true, custom: retained}",
            ),
        ] {
            let mut values = crate::preferences::native::decode(kind, source.as_bytes()).unwrap();
            values.insert(crate::preferences::native::flag(kind).into(), "true".into());
            repository
                .save(
                    &crate::storage::Namespace::parse(kind.id()).unwrap(),
                    &values,
                )
                .unwrap();
        }
        let service = crate::integration::Service::start(repository).unwrap();
        cx.update(|cx| {
            crate::integration::init(service, cx);
            gpui_kit::init(cx);
            cx.set_reduce_motion(true);
        });
        for (kind, global, service) in [
            (SettingsKind::Dns, "DnsOverrideEnabled", "DnsEnabled"),
            (
                SettingsKind::Sniffer,
                "SnifferOverrideEnabled",
                "SnifferEnabled",
            ),
        ] {
            let (window, page) = cx.update(|cx| {
                gpui_kit::open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                            Default::default(),
                            size(px(1280.), px(900.)),
                        ))),
                        ..Default::default()
                    },
                    cx,
                    |window, cx| {
                        let session = cx.new(crate::session::Session::new);
                        cx.new(|cx| SettingsPage::new(kind, session, window, cx))
                    },
                )
                .unwrap()
            });
            cx.run_until_parked();
            window
                .update(cx, |_, window, cx| {
                    window.render_frame(cx);
                    let header_switch = window
                        .find(gpui_kit::SharedString::from(format!("setting-{global}")))
                        .bounds();
                    let service_switch = window
                        .find(gpui_kit::SharedString::from(format!("setting-{service}")))
                        .bounds();
                    assert!(header_switch.origin.y < service_switch.origin.y);
                    assert_eq!(
                        window
                            .find_all(gpui_kit::SharedString::from(format!("setting-{global}")))
                            .len(),
                        1
                    );
                    window.click(
                        gpui_kit::SharedString::from(format!("setting-{service}")),
                        cx,
                    );
                    window.clear_notifications(cx);
                    window.click(
                        gpui_kit::SharedString::from(format!("setting-{global}")),
                        cx,
                    );
                    window.clear_notifications(cx);
                    page.update(cx, |page, cx| {
                        page.switch_dns_mode(!page.dns_raw, window, cx)
                    });
                    assert!(
                        page.read(cx).dns_raw,
                        "mode {:?}: {:?}",
                        kind,
                        page.read(cx).error
                    );
                    let text = page.read(cx).dns_source.read(cx).value();
                    let yaml: serde_yaml_ng::Value =
                        serde_yaml_ng::from_str(text.as_ref()).unwrap();
                    assert!(yaml.get("override-enabled").is_none());
                    assert_eq!(yaml[kind.id()]["enable"], false);
                    window.render_frame(cx);
                    assert!(!page.read(cx).dns_source.read(cx).is_editable());

                    if kind == SettingsKind::Dns {
                        assert_eq!(yaml["dns"]["cache-algorithm"], "arc");
                    } else {
                        assert_eq!(yaml["sniffer"]["custom"], "retained");
                    }
                    let mut yaml = yaml;
                    yaml[kind.id()]["enable"] = true.into();
                    if kind == SettingsKind::Dns {
                        yaml["dns"]["nameserver"] = serde_yaml_ng::from_str("[8.8.8.8]").unwrap();
                    }
                    page.read(cx).dns_source.clone().update(cx, |state, cx| {
                        state.set_value(serde_yaml_ng::to_string(&yaml).unwrap(), window, cx)
                    });
                    window.clear_notifications(cx);
                    page.update(cx, |page, cx| {
                        page.switch_dns_mode(!page.dns_raw, window, cx)
                    });
                    assert!(!page.read(cx).dns_raw);
                    assert!(matches!(
                        page.read(cx).fields[global].control,
                        FieldControl::Toggle(false)
                    ));
                    assert!(matches!(
                        page.read(cx).fields[service].control,
                        FieldControl::Toggle(true)
                    ));
                    if kind == SettingsKind::Dns {
                        // Switch mode before the input's blur: the current form value must win.
                        let FieldControl::Multiline(state) =
                            &page.read(cx).fields["Nameserver"].control
                        else {
                            panic!("nameserver editor");
                        };
                        state
                            .clone()
                            .update(cx, |state, cx| state.set_value("9.9.9.9", window, cx));
                    }
                    window.clear_notifications(cx);
                    page.update(cx, |page, cx| {
                        page.switch_dns_mode(!page.dns_raw, window, cx)
                    });
                    assert!(
                        page.read(cx).dns_raw,
                        "mode {:?}: {:?}",
                        kind,
                        page.read(cx).error
                    );
                    let text = page.read(cx).dns_source.read(cx).value();
                    let yaml: serde_yaml_ng::Value =
                        serde_yaml_ng::from_str(text.as_ref()).unwrap();
                    assert!(yaml.get("override-enabled").is_none());
                    assert_eq!(yaml[kind.id()]["enable"], true);
                    if kind == SettingsKind::Dns {
                        assert_eq!(yaml["dns"]["nameserver"][0], "9.9.9.9");
                    }
                    page.update(cx, |page, cx| page.toggle(global, true, window, cx));
                    assert!(matches!(
                        page.read(cx).fields[global].control,
                        FieldControl::Toggle(true)
                    ));
                    let text = page.read(cx).dns_source.read(cx).value();
                    let yaml: serde_yaml_ng::Value =
                        serde_yaml_ng::from_str(text.as_ref()).unwrap();
                    assert!(yaml.get("override-enabled").is_none());
                    assert_eq!(yaml[kind.id()]["enable"], true);
                    window.clear_notifications(cx);
                    window.render_frame(cx);
                    assert!(page.read(cx).dns_source.read(cx).is_editable());
                })
                .unwrap();
        }
        crate::integration::finish_test_services(cx);
    }
}
