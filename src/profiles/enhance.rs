//! Local subscription enhancement drafts. No runtime configuration is generated here.
use super::model::Document;
use crate::{
    integration as storage, ui,
    ui::theme::{control, layout, space},
};
use gpui_kit::{
    assets::IconName,
    component::{
        Disableable, IndexPath, Sizable, WindowExt,
        button::{Button, ButtonVariants},
        h_flex,
        input::{Editor, EditorState, Input, InputState},
        select::{Select, SelectState},
        switch::Switch,
        tab::{Tab, TabBar},
        v_flex,
    },
    prelude::FluentBuilder as _,
    *,
};
use serde_yaml_ng::{Mapping, Value};
use std::collections::BTreeMap;

const PROPERTIES: [&str; 3] = ["rules", "proxies", "proxy-groups"];
const LABELS: [&str; 3] = ["规则", "代理", "代理组"];
const PARTS: [&str; 3] = ["prepend", "append", "delete"];

pub(super) fn open(document: Document, property: usize, window: &mut Window, cx: &mut App) {
    let title = format!("订阅增强 · {}", document.name);
    let editor = cx.new(|cx| EnhancementEditor::new(document, property, window, cx));
    window.open_dialog(cx, move |dialog, window, cx| {
        let save = editor.clone();
        let body = editor.clone();
        ui::modal(
            dialog,
            title.clone(),
            layout::EDITOR_DIALOG_WIDTH,
            window,
            cx,
        )
        .content(ui::modal_panel(
            crate::ui::theme::layout::FORM_MAX_HEIGHT,
            move |_, _| body.clone().into_any_element(),
        ))
        .footer(ui::dialog_footer("保存草稿"))
        .on_ok(move |_, w, cx| save.update(cx, |editor, cx| editor.save(w, cx)))
    });
}
struct EnhancementEditor {
    profile_id: u64,
    property: usize,
    raw: bool,
    scroll: ScrollHandle,
    source: Value,
    drafts: [Value; 3],
    code: Entity<EditorState>,
    query: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}
impl EnhancementEditor {
    fn new(
        document: Document,
        property: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let saved = storage::read(&format!("profile-enhance-{}", document.id), cx);
        let drafts = std::array::from_fn(|index| {
            saved
                .get(PROPERTIES[index])
                .and_then(|source| serde_yaml_ng::from_str(source).ok())
                .unwrap_or_else(|| Value::Mapping(Mapping::new()))
        });
        let code = cx.new(|cx| ui::editor::state("yaml", "", true, window, cx));
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("搜索名称或规则…"));
        let subscriptions = vec![cx.subscribe(
            &query,
            |this, _, _: &gpui_kit::component::input::InputEvent, cx| {
                this.scroll.set_offset(Point::default());
                cx.notify();
            },
        )];
        Self {
            profile_id: document.id,
            property,
            raw: false,
            scroll: ScrollHandle::new(),
            source: serde_yaml_ng::from_str(&document.content).unwrap_or_default(),
            drafts,
            code,
            query,
            _subscriptions: subscriptions,
        }
    }
    fn read_code(&mut self, cx: &App) -> Result<(), String> {
        let value: Value = serde_yaml_ng::from_str(self.code.read(cx).value().as_ref())
            .map_err(|e| e.to_string())?;
        let map = value
            .as_mapping()
            .ok_or("增强内容需为 prepend / append / delete 映射")?;
        for part in PARTS {
            if map
                .get(Value::String(part.into()))
                .is_some_and(|value| !value.is_sequence())
            {
                return Err(format!("{part} 必须是列表"));
            }
        }
        self.drafts[self.property] = value;
        Ok(())
    }
    fn mode(&mut self, raw: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.raw
            && let Err(error) = self.read_code(cx)
        {
            ui::show_error(error, window, cx);
            return;
        }
        if raw {
            let text = serde_yaml_ng::to_string(&self.drafts[self.property]).unwrap_or_default();
            self.code
                .update(cx, |state, cx| state.set_value(text, window, cx));
        }
        self.raw = raw;
        cx.notify();
    }
    fn revision(&self, cx: &App) -> Vec<String> {
        vec![
            self.raw.to_string(),
            self.code.read(cx).value().to_string(),
            serde_yaml_ng::to_string(&self.drafts).unwrap_or_default(),
        ]
    }
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.raw
            && let Err(error) = self.read_code(cx)
        {
            ui::show_error(error, window, cx);
            return false;
        }
        let values = PROPERTIES
            .iter()
            .enumerate()
            .map(|(index, key)| {
                (
                    key.to_string(),
                    serde_yaml_ng::to_string(&self.drafts[index]).unwrap_or_default(),
                )
            })
            .collect();
        storage::save_draft(
            &format!("profile-enhance-{}", self.profile_id),
            values,
            cx.weak_entity(),
            self.revision(cx),
            |editor, cx| editor.revision(cx),
            window,
            cx,
        );
        false
    }
    fn items(&self, part: &str) -> Vec<Value> {
        self.drafts[self.property]
            .get(part)
            .and_then(Value::as_sequence)
            .cloned()
            .unwrap_or_default()
    }
    fn set_items(&mut self, part: &str, items: Vec<Value>) {
        if !self.drafts[self.property].is_mapping() {
            self.drafts[self.property] = Value::Mapping(Mapping::new());
        }
        self.drafts[self.property]
            .as_mapping_mut()
            .unwrap()
            .insert(Value::String(part.into()), Value::Sequence(items));
    }
    fn item_name(value: &Value) -> String {
        value
            .as_str()
            .map(str::to_owned)
            .or_else(|| value.get("name").and_then(Value::as_str).map(str::to_owned))
            .unwrap_or_else(|| serde_yaml_ng::to_string(value).unwrap_or_default())
    }
    fn edit_item(
        &mut self,
        part: &'static str,
        index: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = index.and_then(|index| self.items(part).get(index).cloned());
        let entry = cx.new(|cx| EntryEditor::new(self.property, value, window, cx));
        let owner = cx.entity();
        let property = self.property;
        window.open_dialog(cx, move |dialog, window, cx| {
            let entry = entry.clone();
            let body = entry.clone();
            let owner = owner.clone();
            ui::modal(
                dialog,
                format!(
                    "{}{}",
                    if index.is_some() { "编辑" } else { "添加" },
                    LABELS[property]
                ),
                layout::DIALOG_WIDTH,
                window,
                cx,
            )
            .content(ui::modal_content(
                "enhancement-entry-body",
                crate::ui::theme::layout::FORM_MAX_HEIGHT,
                move |_, _| body.clone().into_any_element(),
            ))
            .footer(ui::dialog_footer("保存"))
            .on_ok(move |_, w, cx| {
                let value = match entry.read(cx).value(cx) {
                    Ok(value) => value,
                    Err(error) => {
                        ui::show_error(error, w, cx);
                        return false;
                    }
                };
                owner.update(cx, |editor, cx| {
                    let mut items = editor.items(part);
                    if let Some(index) = index {
                        if let Some(item) = items.get_mut(index) {
                            *item = value;
                        }
                    } else {
                        items.push(value);
                    }
                    editor.set_items(part, items);
                    cx.notify();
                });
                true
            })
        });
    }
}
impl Render for EnhancementEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.query.read(cx).value().to_lowercase();
        let mut body = v_flex().gap(space::LG);
        if !self.raw {
            for (part, title, editable) in [
                ("prepend", "前置", true),
                ("original", "原始内容", false),
                ("append", "追加", true),
                ("delete", "已排除", false),
            ] {
                let items = if part == "original" {
                    self.source
                        .get(PROPERTIES[self.property])
                        .and_then(Value::as_sequence)
                        .cloned()
                        .unwrap_or_default()
                } else {
                    self.items(part)
                };
                let deleted = self.items("delete");
                let mut rows = Vec::new();
                for (index, value) in items.iter().enumerate() {
                    let name = Self::item_name(value);
                    if !name.to_lowercase().contains(&query) {
                        continue;
                    }
                    let identity = format!("{part}-{index}-{name}");
                    let excluded = deleted.iter().any(|v| Self::item_name(v) == name);
                    let delete_value = if self.property == 0 {
                        value.clone()
                    } else {
                        Value::String(name.clone())
                    };
                    let row = h_flex()
                        .gap(space::SM)
                        .px(space::LG)
                        .py(space::SM)
                        .child(div().flex_1().min_w(space::NONE).truncate().child(name))
                        .when(editable, |row| {
                            row.child(
                                ui::icon_button(
                                    SharedString::from(format!("edit-{identity}")),
                                    IconName::Pencil,
                                    "编辑…",
                                )
                                .on_click(cx.listener(
                                    move |this, _, w, cx| this.edit_item(part, Some(index), w, cx),
                                )),
                            )
                            .child(
                                ui::icon_button(
                                    SharedString::from(format!("up-{identity}")),
                                    IconName::ArrowUp,
                                    "上移",
                                )
                                .disabled(index == 0)
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        let mut items = this.items(part);
                                        items.swap(index, index - 1);
                                        this.set_items(part, items);
                                        cx.notify();
                                    },
                                )),
                            )
                        })
                        .child(
                            Button::new(SharedString::from(format!("remove-{identity}")))
                                .ghost()
                                .small()
                                .label(if part == "original" {
                                    if excluded { "恢复" } else { "排除" }
                                } else if part == "delete" {
                                    "恢复"
                                } else {
                                    "删除"
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if part == "original" {
                                        let mut items = this.items("delete");
                                        if excluded {
                                            items.retain(|v| {
                                                Self::item_name(v) != Self::item_name(&delete_value)
                                            });
                                        } else {
                                            items.push(delete_value.clone());
                                        }
                                        this.set_items("delete", items);
                                    } else {
                                        let mut items = this.items(part);
                                        items.remove(index);
                                        this.set_items(part, items);
                                    }
                                    cx.notify();
                                })),
                        );
                    rows.push(row.into_any_element());
                }
                if rows.is_empty() {
                    rows.push(
                        ui::empty(IconName::List, "暂无内容", "添加条目或调整搜索条件", cx)
                            .h(layout::EMPTY_INLINE_HEIGHT)
                            .min_h(space::NONE)
                            .py(space::SM)
                            .into_any_element(),
                    );
                }
                if editable {
                    rows.push(
                        h_flex()
                            .px(space::LG)
                            .py(space::SM)
                            .child(
                                Button::new(SharedString::from(format!("add-{part}")))
                                    .ghost()
                                    .with_size(control::STANDARD)
                                    .icon(IconName::Plus)
                                    .label("添加…")
                                    .on_click(cx.listener(move |this, _, w, cx| {
                                        this.edit_item(part, None, w, cx)
                                    })),
                            )
                            .into_any_element(),
                    );
                }
                body = body.child(ui::preferences_group(title, rows, cx));
            }
        }
        v_flex()
            .size_full()
            .min_h(space::NONE)
            .gap(space::LG)
            .child(
                h_flex()
                    .flex_shrink_0()
                    .gap(space::SM)
                    .child(
                        TabBar::new("enhance-property")
                            .segmented()
                            .with_size(control::STANDARD)
                            .children(LABELS.iter().map(|label| Tab::new().label(*label)))
                            .selected_index(self.property)
                            .on_click(cx.listener(|this, index, w, cx| {
                                if this.raw
                                    && let Err(error) = this.read_code(cx)
                                {
                                    ui::show_error(error, w, cx);
                                    return;
                                }
                                let raw = this.raw;
                                this.raw = false;
                                this.property = *index;
                                this.scroll.set_offset(Point::default());
                                if raw {
                                    this.mode(true, w, cx);
                                }
                                cx.notify();
                            })),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("enhance-mode")
                            .outline()
                            .with_size(control::STANDARD)
                            .label(if self.raw {
                                "可视化编辑"
                            } else {
                                "YAML 编辑"
                            })
                            .on_click(cx.listener(|this, _, w, cx| this.mode(!this.raw, w, cx))),
                    ),
            )
            .when(!self.raw, |view| {
                view.child(
                    div().flex_shrink_0().child(
                        Input::new(&self.query)
                            .id("enhancement-search")
                            .w_full()
                            .with_size(control::STANDARD)
                            .cleanable(true)
                            .aria_label("搜索订阅增强"),
                    ),
                )
            })
            .child(if self.raw {
                div().flex_1().min_h(space::NONE).child(
                    Editor::new(&self.code)
                        .w_full()
                        .h_full()
                        .aria_label("订阅增强 YAML"),
                )
            } else {
                div().flex_1().min_h(space::NONE).child(ui::modal_scroll(
                    "enhancement-list-scroll",
                    &self.scroll,
                    body.flex_none().into_any_element(),
                ))
            })
    }
}
struct EntryEditor {
    property: usize,
    original: Value,
    fields: BTreeMap<&'static str, Entity<InputState>>,
    types: Entity<SelectState<Vec<SharedString>>>,
    no_resolve: bool,
    members: Entity<EditorState>,
}
impl EntryEditor {
    fn new(
        property: usize,
        value: Option<Value>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let original = value.unwrap_or_default();
        let rule: Vec<_> = original
            .as_str()
            .unwrap_or("DOMAIN-SUFFIX,,DIRECT")
            .split(',')
            .collect();
        let specs: &[(&str, &str)] = match property {
            0 => &[
                ("type", "DOMAIN-SUFFIX"),
                ("payload", ""),
                ("policy", "DIRECT"),
            ],
            1 => &[
                ("name", ""),
                ("type", "ss"),
                ("server", ""),
                ("port", "443"),
                ("cipher", "aes-128-gcm"),
                ("password", ""),
            ],
            _ => &[("name", ""), ("type", "select")],
        };
        let fields: BTreeMap<_, _> = specs
            .iter()
            .enumerate()
            .map(|(index, (key, default))| {
                let value = if property == 0 {
                    if rule.first() == Some(&"MATCH") {
                        match *key {
                            "type" => "MATCH".into(),
                            "payload" => String::new(),
                            _ => rule.get(1).copied().unwrap_or("DIRECT").to_owned(),
                        }
                    } else {
                        rule.get(index).copied().unwrap_or(default).to_owned()
                    }
                } else {
                    original
                        .get(*key)
                        .and_then(|v| {
                            v.as_str()
                                .map(str::to_owned)
                                .or_else(|| v.as_u64().map(|v| v.to_string()))
                        })
                        .unwrap_or_else(|| default.to_string())
                };
                (
                    *key,
                    cx.new(|cx| {
                        InputState::new(window, cx)
                            .default_value(value)
                            .masked(*key == "password")
                    }),
                )
            })
            .collect();
        let current = fields["type"].read(cx).value().to_string();
        let mut choices: Vec<SharedString> = match property {
            0 => ui::RULE_TYPES.iter().copied().map(Into::into).collect(),
            1 => [
                "ss",
                "ssr",
                "vmess",
                "vless",
                "trojan",
                "hysteria2",
                "tuic",
                "socks5",
                "http",
                "wireguard",
                "ssh",
                "direct",
            ]
            .into_iter()
            .map(Into::into)
            .collect(),
            _ => ["select", "url-test", "fallback", "load-balance", "relay"]
                .into_iter()
                .map(Into::into)
                .collect(),
        };
        if !choices.iter().any(|choice| choice.as_ref() == current) {
            choices.push(current.clone().into());
        }
        let index = choices
            .iter()
            .position(|choice| choice.as_ref() == current)
            .unwrap_or(0);
        let types = cx.new(|cx| SelectState::new(choices, Some(IndexPath::new(index)), window, cx));
        let no_resolve = rule.contains(&"no-resolve");
        let text = original
            .get("proxies")
            .and_then(Value::as_sequence)
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        let members = cx.new(|cx| ui::editor::state("text", text, false, window, cx));
        Self {
            property,
            original,
            fields,
            types,
            no_resolve,
            members,
        }
    }
    fn value(&self, cx: &App) -> Result<Value, String> {
        let get = |key| {
            if key == "type" {
                self.types
                    .read(cx)
                    .selected_value()
                    .cloned()
                    .unwrap_or_default()
                    .to_string()
            } else {
                self.fields[key].read(cx).value().trim().to_owned()
            }
        };
        if self.property == 0 {
            let kind = get("type");
            let payload = get("payload");
            let policy = get("policy");
            if kind.is_empty() || policy.is_empty() || kind != "MATCH" && payload.is_empty() {
                return Err("填写规则类型、匹配内容和策略".into());
            }
            return Ok(Value::String(if kind == "MATCH" {
                format!("MATCH,{policy}")
            } else {
                format!(
                    "{kind},{payload},{policy}{}",
                    if self.no_resolve { ",no-resolve" } else { "" }
                )
            }));
        }
        if get("name").is_empty() || get("type").is_empty() {
            return Err("填写名称和类型".into());
        }
        let mut map = self.original.as_mapping().cloned().unwrap_or_default();
        for key in self.fields.keys() {
            let text = get(key);
            let value = if *key == "port" {
                Value::Number(
                    text.parse::<u16>()
                        .ok()
                        .filter(|v| *v > 0)
                        .ok_or("端口需在 1–65535 之间")?
                        .into(),
                )
            } else {
                Value::String(text)
            };
            map.insert(Value::String((*key).into()), value);
        }
        if self.property == 2 {
            map.insert(
                Value::String("proxies".into()),
                Value::Sequence(
                    self.members
                        .read(cx)
                        .value()
                        .lines()
                        .map(str::trim)
                        .filter(|v| !v.is_empty())
                        .map(|v| Value::String(v.into()))
                        .collect(),
                ),
            );
        }
        Ok(Value::Mapping(map))
    }
}
impl Render for EntryEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut form = v_flex().gap(space::MD);
        for key in [
            "name", "type", "payload", "policy", "server", "port", "cipher", "password",
        ] {
            if key == "type" {
                form = form.child(ui::field_label(
                    "类型",
                    Select::new(&self.types)
                        .w_full()
                        .with_size(control::STANDARD)
                        .accessibility_label("类型"),
                ));
                continue;
            }
            if let Some(state) = self.fields.get(key) {
                let label = match key {
                    "name" => "名称",
                    "type" => "类型",
                    "payload" => "匹配内容",
                    "policy" => "出站策略",
                    "server" => "服务器",
                    "port" => "端口",
                    "cipher" => "加密方式",
                    _ => "密码",
                };
                form = form.child(ui::field_label(
                    label,
                    Input::new(state)
                        .w_full()
                        .with_size(control::STANDARD)
                        .aria_label(label),
                ));
            }
        }
        if self.property == 0 {
            form = form.child(
                h_flex()
                    .justify_between()
                    .child("不解析域名（no-resolve）")
                    .child(
                        Switch::new("enhance-no-resolve")
                            .checked(self.no_resolve)
                            .accessibility_label("不解析域名")
                            .on_change(cx.listener(|this, value, _, cx| {
                                this.no_resolve = *value;
                                cx.notify();
                            })),
                    ),
            );
        }
        if self.property == 2 {
            form = form.child(ui::field_label(
                "成员（每行一个）",
                Editor::new(&self.members)
                    .w_full()
                    .h(layout::TEXTAREA_HEIGHT)
                    .aria_label("代理组成员"),
            ));
        }
        form
    }
}

#[cfg(test)]
mod ui_tests {
    use super::{Document, open};
    use crate::profiles::model::DocumentKind;
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{
        AppContext, Bounds, Context, IntoElement, Render, ScrollDelta, Styled, TestAppContext,
        Window, WindowBounds, WindowOptions, div, point, px, size,
    };

    struct Host;
    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full()
        }
    }

    #[gpui_kit::test]
    fn enhancement_scroll_keeps_toolbar_search_and_footer_fixed(cx: &mut TestAppContext) {
        check_fixed_chrome(cx, 100);
    }

    #[gpui_kit::test]
    fn short_enhancement_lists_also_scroll_inside_their_viewport(cx: &mut TestAppContext) {
        check_fixed_chrome(cx, 1);
    }

    fn check_fixed_chrome(cx: &mut TestAppContext, count: usize) {
        let root = tempfile::tempdir().unwrap();
        let service = crate::integration::Service::start(
            crate::integration::Repository::open(
                crate::integration::AppPaths::isolated(root.path()),
                None,
            )
            .unwrap(),
        )
        .unwrap();
        cx.update(|cx| {
            crate::integration::init(service, cx);
            gpui_kit::init(cx);
            cx.set_reduce_motion(true);
        });
        let (handle, _) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        Default::default(),
                        size(px(960.), px(640.)),
                    ))),
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(|_| Host),
            )
            .unwrap()
        });
        cx.update_window(handle, |_, window, cx| {
            let mut document = Document::new(90001, DocumentKind::Profile);
            document.content = format!(
                "rules:\n{}",
                (0..count)
                    .map(|index| format!("  - DOMAIN-SUFFIX,test{index}.example,DIRECT\n"))
                    .collect::<String>()
            );
            open(document, 0, window, cx);
            window.render_frame(cx);
            let mode = window.find("enhance-mode").bounds();
            let search = window.find("enhancement-search").bounds();
            let footer = window.find("dialog-commit").bounds();
            let viewport = window.find("enhancement-list-scroll").bounds();
            assert!(viewport.origin.y >= search.bottom());
            assert!(viewport.bottom() <= footer.origin.y);
            let before = window.find("add-prepend").bounds();
            window.scroll(
                "enhancement-list-scroll",
                ScrollDelta::Pixels(point(px(0.), px(-320.))),
                cx,
            );
            let after = window.find("add-prepend").bounds();
            assert!(
                after.origin.y < before.origin.y,
                "the list did not scroll: {before:?} -> {after:?}"
            );
            assert_eq!(window.find("enhancement-list-scroll").bounds(), viewport);
            assert_eq!(window.find("enhance-mode").bounds(), mode);
            assert_eq!(window.find("enhancement-search").bounds(), search);
            assert_eq!(window.find("dialog-commit").bounds(), footer);
            assert!(window.find("dialog-commit").visible());
        })
        .unwrap();
    }
}
