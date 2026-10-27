//! Local subscription enhancement drafts. No runtime configuration is generated here.
use super::model::Document;
use crate::{
    integration as storage, ui,
    ui::theme::{control, layout, space, stroke},
};
use gpui_kit::{
    assets::IconName,
    component::{
        ActiveTheme, Disableable, Icon, IndexPath, Sizable, WindowExt,
        button::{Button, ButtonVariants},
        h_flex,
        input::{Editor, EditorState, Input, InputState},
        select::{Select, SelectState},
        switch::Switch,
        tab::{Tab, TabBar},
    },
    prelude::FluentBuilder as _,
    *,
};
use serde_yaml_ng::{Mapping, Value};
use std::collections::BTreeMap;

const PROPERTIES: [&str; 3] = ["rules", "proxies", "proxy-groups"];
const LABELS: [&str; 3] = ["规则", "代理", "代理组"];
const PARTS: [&str; 3] = ["prepend", "append", "delete"];
const LIST_PARTS: [&str; 4] = ["original", "prepend", "delete", "append"];
const PART_LABELS: [&str; 4] = ["原始", "前置", "已排除", "追加"];

pub(super) fn open(
    document: Document,
    property: usize,
    window: &mut Window,
    cx: &mut App,
) -> Entity<EnhancementEditor> {
    let title = format!("订阅增强 · {}", document.name);
    let editor = cx.new(|cx| EnhancementEditor::new(document, property, window, cx));
    let retained = editor.clone();
    let footer = cx.new(|cx| EnhancementFooter::new(editor.clone(), cx));
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
        .footer(footer.clone())
        .on_ok(move |_, w, cx| save.update(cx, |editor, cx| editor.save(w, cx)))
    });
    retained
}
pub(super) struct EnhancementEditor {
    profile_id: u64,
    property: usize,
    raw: bool,
    part: usize,
    saving: bool,
    expected: crate::storage::Values,
    lists: BTreeMap<&'static str, gpui_kit::component::VirtualListScrollHandle>,
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
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("搜索名称或规则"));
        let subscriptions = vec![cx.subscribe(
            &query,
            |this, _, _: &gpui_kit::component::input::InputEvent, cx| {
                for handle in this.lists.values() {
                    handle.set_offset(Point::default());
                }
                cx.notify();
            },
        )];
        Self {
            profile_id: document.id,
            property,
            raw: false,
            part: 1,
            saving: false,
            expected: saved,
            lists: ["prepend", "original", "append", "delete"]
                .into_iter()
                .map(|name| (name, gpui_kit::component::VirtualListScrollHandle::new()))
                .collect(),
            source: serde_yaml_ng::from_str(&document.content).unwrap_or_default(),
            drafts,
            code,
            query,
            _subscriptions: subscriptions,
        }
    }
    fn read_code(&mut self, cx: &App) -> Result<(), String> {
        let value: Value = serde_yaml_ng::from_str(self.code.read(cx).value().as_ref())
            .map_err(|_| "增强 YAML 格式无效")?;
        let root = value.as_mapping().ok_or("增强顶层需为对象")?;
        if root
            .keys()
            .any(|key| key.as_str().is_none_or(|key| !PROPERTIES.contains(&key)))
        {
            return Err("增强仅支持 rules、proxies 和 proxy-groups".into());
        }
        let mut drafts = std::array::from_fn(|_| Value::Mapping(Mapping::new()));
        for (index, property) in PROPERTIES.iter().enumerate() {
            let Some(value) = root.get(Value::String((*property).into())) else {
                continue;
            };
            let map = value
                .as_mapping()
                .ok_or_else(|| format!("{property} 需为对象"))?;
            if map
                .keys()
                .any(|key| key.as_str().is_none_or(|key| !PARTS.contains(&key)))
            {
                return Err(format!("{property} 仅支持 prepend、append 和 delete"));
            }
            for part in PARTS {
                if let Some(value) = map.get(Value::String(part.into()))
                    && !value.is_sequence()
                {
                    return Err(format!("{property}.{part} 必须是列表"));
                }
            }
            drafts[index] = value.clone();
        }
        self.drafts = drafts;
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
            let mapping: Mapping = PROPERTIES
                .iter()
                .enumerate()
                .map(|(index, key)| (Value::String((*key).into()), self.drafts[index].clone()))
                .collect();
            let text = match serde_yaml_ng::to_string(&Value::Mapping(mapping)) {
                Ok(text) => text,
                Err(_) => {
                    ui::show_error("无法编码订阅增强".into(), window, cx);
                    return;
                }
            };
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
        if self.saving {
            return false;
        }
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
        let expected = self.expected.clone();
        let owner = cx.weak_entity();
        let revision = self.revision(cx);
        let result = crate::core::execute(
            crate::core::Command::Profiles(crate::core::ProfileCommand::Enhance {
                id: self.profile_id,
                expected,
                values,
            }),
            window,
            cx,
            move |result, window, cx| {
                let close = owner.upgrade().is_some_and(|owner| {
                    owner.update(cx, |editor, cx| {
                        editor.saving = false;
                        cx.notify();
                        result.is_ok() && editor.revision(cx) == revision
                    })
                });
                if close {
                    window.close_dialog(cx);
                }
            },
        );
        match result {
            Ok(()) => {
                self.saving = true;
                cx.notify();
            }
            Err(error) => ui::show_error(error, window, cx),
        }

        false
    }
    fn item_refs(&self, part: &str) -> &[Value] {
        let value = if part == "original" {
            self.source.get(PROPERTIES[self.property])
        } else {
            self.drafts[self.property].get(part)
        };
        value
            .and_then(Value::as_sequence)
            .map(Vec::as_slice)
            .unwrap_or(&[])
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
impl EnhancementEditor {
    fn enhancement_row(
        &self,
        part: &'static str,
        index: usize,
        editable: bool,
        deleted: &std::collections::BTreeSet<String>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(value) = self.item_refs(part).get(index) else {
            return div().into_any_element();
        };
        let name = Self::item_name(value);
        let identity = format!("{part}-{index}-{name}");
        let excluded = deleted.contains(&name);
        let delete_value = if self.property == 0 {
            value.clone()
        } else {
            Value::String(name.clone())
        };
        let row = h_flex()
            .gap(space::SM)
            .px(space::SM)
            .py(space::SM)
            .border_b(stroke::HAIRLINE)
            .border_color(cx.theme().border)
            .child(
                div()
                    .flex_1()
                    .min_w(space::NONE)
                    .truncate()
                    .when(excluded, |label| {
                        label.text_color(cx.theme().muted_foreground)
                    })
                    .child(name),
            )
            .when(editable, |row| {
                row.child(
                    ui::icon_button(
                        SharedString::from(format!("edit-{identity}")),
                        IconName::Pencil,
                        "编辑",
                    )
                    .disabled(self.saving)
                    .on_click(
                        cx.listener(move |this, _, w, cx| this.edit_item(part, Some(index), w, cx)),
                    ),
                )
                .child(
                    ui::icon_button(
                        SharedString::from(format!("up-{identity}")),
                        IconName::ArrowUp,
                        "上移",
                    )
                    .disabled(self.saving || index == 0)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let mut items = this.items(part);
                        items.swap(index, index - 1);
                        this.set_items(part, items);
                        cx.notify();
                    })),
                )
                .child(
                    ui::icon_button(
                        SharedString::from(format!("down-{identity}")),
                        IconName::ArrowDown,
                        "下移",
                    )
                    .disabled(self.saving || index + 1 >= self.item_refs(part).len())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let mut items = this.items(part);
                        if index + 1 < items.len() {
                            items.swap(index, index + 1);
                            this.set_items(part, items);
                            cx.notify();
                        }
                    })),
                )
            })
            .child(
                Button::new(SharedString::from(format!("remove-{identity}")))
                    .icon(gpui_kit::assets::IconName::Minus)
                    .ghost()
                    .small()
                    .disabled(self.saving)
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
        row.into_any_element()
    }
}
struct EnhancementFooter {
    editor: Entity<EnhancementEditor>,
    _subscription: Subscription,
}
impl EnhancementFooter {
    fn new(editor: Entity<EnhancementEditor>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&editor, |_, _, cx| cx.notify());
        Self {
            editor,
            _subscription: subscription,
        }
    }
}
impl Render for EnhancementFooter {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        ui::dialog_footer_pending("保存", self.editor.read(cx).saving)
    }
}
impl Render for EnhancementEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let part = LIST_PARTS[self.part];
        let query = self.query.read(cx).value().trim().to_lowercase();
        let indices = self
            .item_refs(part)
            .iter()
            .enumerate()
            .filter_map(|(index, value)| {
                Self::item_name(value)
                    .to_lowercase()
                    .contains(&query)
                    .then_some(index)
            })
            .collect::<Vec<_>>();
        let editable = matches!(part, "prepend" | "append");
        let content = if self.raw {
            Editor::new(&self.code)
                .w_full()
                .h_full()
                .disabled(self.saving)
                .aria_label("订阅增强 YAML")
                .into_any_element()
        } else if indices.is_empty() {
            ui::empty(
                IconName::List,
                if query.is_empty() {
                    "暂无内容"
                } else {
                    "无匹配内容"
                },
                "",
                cx,
            )
            .min_h(space::NONE)
            .into_any_element()
        } else {
            let deleted = std::rc::Rc::new(
                self.item_refs("delete")
                    .iter()
                    .map(Self::item_name)
                    .collect(),
            );
            let heights = vec![
                (layout::CONTROL_HEIGHT + space::SM + space::SM)
                    .to_pixels(window.rem_size());
                indices.len()
            ];
            let indices = std::rc::Rc::new(indices);
            ui::virtual_rows::rows(
                cx.entity(),
                "enhancement-list-scroll",
                heights,
                &self.lists[part],
                move |this, row, _, cx| {
                    this.enhancement_row(part, indices[row], editable, &deleted, cx)
                },
            )
        };
        let viewport = div()
            .id("enhancement-viewport")
            .flex_1()
            .min_h(space::NONE)
            .w_full()
            .child(content);
        #[cfg(test)]
        let viewport = {
            use gpui_kit::test::TestSupportExt as _;
            viewport.test_support()
        };
        ui::form()
            .size_full()
            .min_h(space::NONE)
            .child(
                h_flex()
                    .w_full()
                    .flex_shrink_0()
                    .gap(space::SM)
                    .when(!self.raw, |row| {
                        row.child(
                            TabBar::new("enhance-property")
                                .segmented()
                                .with_size(control::STANDARD)
                                .children(
                                    LABELS.iter().map(|label| {
                                        Tab::new().label(*label).disabled(self.saving)
                                    }),
                                )
                                .selected_index(self.property)
                                .on_click(cx.listener(|this, index, _, cx| {
                                    this.property = *index;
                                    for list in this.lists.values() {
                                        list.set_offset(Point::default());
                                    }
                                    cx.notify();
                                })),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        Button::new("enhance-mode")
                            .icon(gpui_kit::assets::IconName::FileCode)
                            .outline()
                            .with_size(control::STANDARD)
                            .disabled(self.saving)
                            .label(if self.raw {
                                "表单编辑"
                            } else {
                                "YAML 编辑"
                            })
                            .on_click(
                                cx.listener(|this, _, window, cx| this.mode(!this.raw, window, cx)),
                            ),
                    )
                    .when(!self.raw && editable, |row| {
                        row.child(
                            Button::new(SharedString::from(format!("add-{part}")))
                                .outline()
                                .with_size(control::STANDARD)
                                .icon(IconName::Plus)
                                .label("添加")
                                .disabled(self.saving)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.edit_item(part, None, window, cx)
                                })),
                        )
                    }),
            )
            .when(!self.raw, |view| {
                view.child(
                    h_flex()
                        .w_full()
                        .flex_shrink_0()
                        .gap(space::SM)
                        .child(
                            Input::new(&self.query)
                                .id("enhancement-search")
                                .flex_1()
                                .min_w(space::NONE)
                                .with_size(control::STANDARD)
                                .cleanable(true)
                                .disabled(self.saving)
                                .prefix(Icon::new(IconName::Search).size(layout::ICON))
                                .aria_label("搜索订阅增强"),
                        )
                        .child(
                            TabBar::new("enhance-part")
                                .segmented()
                                .with_size(control::STANDARD)
                                .flex_shrink_0()
                                .children(PART_LABELS.iter().enumerate().map(|(index, label)| {
                                    Tab::new()
                                        .label(format!(
                                            "{label} {}",
                                            self.item_refs(LIST_PARTS[index]).len()
                                        ))
                                        .disabled(self.saving)
                                }))
                                .selected_index(self.part)
                                .on_click(cx.listener(|this, index, _, cx| {
                                    this.part = *index;
                                    cx.notify();
                                })),
                        ),
                )
            })
            .child(viewport)
    }
}
struct EntryEditor {
    property: usize,
    original: Value,
    fields: BTreeMap<&'static str, Entity<InputState>>,
    types: Entity<SelectState<Vec<SharedString>>>,
    no_resolve: bool,
    members: Entity<EditorState>,
    _types: Subscription,
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
        let subscription = cx.observe(&types, |_, _, cx| cx.notify());
        Self {
            property,
            original,
            fields,
            types,
            no_resolve,
            members,
            _types: subscription,
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
        let field = |key, label| {
            ui::field_label(
                label,
                Input::new(&self.fields[key])
                    .w_full()
                    .with_size(control::STANDARD)
                    .aria_label(label.to_owned()),
            )
        };
        let mut form = ui::form();
        if self.property != 0 {
            form = form.child(field("name", "名称"));
        }
        form = form.child(ui::field_label(
            "类型",
            Select::new(&self.types)
                .w_full()
                .with_size(control::STANDARD)
                .accessibility_label("类型"),
        ));
        match self.property {
            0 => {
                let matches_all = self
                    .types
                    .read(cx)
                    .selected_value()
                    .is_some_and(|value| value.as_ref() == "MATCH");
                if !matches_all {
                    form = form.child(field("payload", "匹配内容"));
                }
                form = form.child(field("policy", "出站策略"));
                if !matches_all {
                    form = form.child(
                        ui::form_row()
                            .justify_between()
                            .child("不解析域名（no-resolve）")
                            .child(
                                Switch::new("enhance-no-resolve")
                                    .with_size(control::STANDARD)
                                    .checked(self.no_resolve)
                                    .accessibility_label("不解析域名")
                                    .on_change(cx.listener(|this, value, _, cx| {
                                        this.no_resolve = *value;
                                        cx.notify();
                                    })),
                            ),
                    );
                }
            }
            1 => {
                form = form
                    .child(
                        ui::form_row()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(space::NONE)
                                    .child(field("server", "服务器")),
                            )
                            .child(
                                div()
                                    .w(layout::FILTER_WIDTH)
                                    .flex_shrink_0()
                                    .child(field("port", "端口")),
                            ),
                    )
                    .child(field("cipher", "加密方式"))
                    .child(field("password", "密码"));
            }
            _ => {
                form = form.child(ui::field_label(
                    "成员",
                    Editor::new(&self.members)
                        .w_full()
                        .h(layout::TEXTAREA_HEIGHT)
                        .aria_label("代理组成员，每行一个"),
                ));
            }
        }
        let form = form.id("enhancement-entry-form");
        #[cfg(test)]
        let form = {
            use gpui_kit::test::TestSupportExt as _;
            form.test_support()
        };
        form
    }
}

#[cfg(test)]
mod ui_tests {
    use super::{Document, open};
    use crate::profiles::model::DocumentKind;
    use gpui_kit::base::ScrollbarHandle as _;
    use gpui_kit::test::{TestQueryExt, TestWindowExt};
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
        check_fixed_chrome(cx, 100, 0);
    }

    #[gpui_kit::test]
    fn short_enhancement_lists_fill_viewport_without_extra_scroll(cx: &mut TestAppContext) {
        check_fixed_chrome(cx, 1, 0);
    }

    #[gpui_kit::test]
    fn proxy_entry_uses_shared_form_and_server_port_row(cx: &mut TestAppContext) {
        check_fixed_chrome(cx, 1, 1);
    }
    #[gpui_kit::test]
    fn group_entry_uses_shared_form_and_members_editor(cx: &mut TestAppContext) {
        check_fixed_chrome(cx, 1, 2);
    }
    fn check_fixed_chrome(cx: &mut TestAppContext, count: usize, property: usize) {
        let root = tempfile::tempdir().unwrap();
        let service = crate::integration::Service::start(
            crate::integration::Repository::open(crate::integration::AppPaths::isolated(
                root.path(),
            ))
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
            if property>0 {
                let key=if property==1 {"proxies"}else{"proxy-groups"};
                let entries=(0..count).map(|index|if property==1 {
                    serde_json::json!({"name":format!("测试代理{index}"),"type":"ss","server":"example.com","port":443,"cipher":"aes-128-gcm","password":"fixture"})
                }else{serde_json::json!({"name":format!("测试组{index}"),"type":"select","proxies":["DIRECT"]})}).collect::<Vec<_>>();
                document.content=serde_yaml_ng::to_string(&serde_json::json!({key:entries})).unwrap();
            }
            let editor = open(document, property, window, cx);
            editor.update(cx, |editor, cx| {
                editor.part = 0;
                cx.notify();
            });
            window.render_frame(cx);
            let mode = window.find("enhance-mode").bounds();
            let search = window.find("enhancement-search").bounds();
            let footer = window.find("dialog-commit").bounds();
            let parts=window.within("enhance-part").find(0usize).bounds();
            assert!(parts.left()>=search.right());
            assert!((parts.center().y-search.center().y).abs()<px(1.));
            assert!(window.within("enhance-property").find(0usize).visible());

            let viewport = window.find("enhancement-viewport").bounds();
            assert!(viewport.top() >= search.bottom());
            assert!(viewport.bottom() <= footer.top());
            let first=match property {0=>"remove-original-0-DOMAIN-SUFFIX,test0.example,DIRECT",1=>"remove-original-0-测试代理0",_=>"remove-original-0-测试组0"};
            assert!(window.find(first).visible());
            window.scroll(
                "enhancement-viewport",
                ScrollDelta::Pixels(point(px(0.), px(-320.))),
                cx,
            );
            let offset = editor.read(cx).lists["original"].offset();
            if count > 1 {
                assert!(offset.y < px(0.));
            } else {
                assert_eq!(offset.y, px(0.));
            }
            assert_eq!(window.find("enhancement-viewport").bounds(), viewport);
            assert_eq!(window.find("enhance-mode").bounds(), mode);
            assert_eq!(window.find("enhancement-search").bounds(), search);
            assert_eq!(window.find("dialog-commit").bounds(), footer);
            window.click("enhance-mode", cx);
            assert!(editor.read(cx).raw);
            assert!(!window.elements().iter().any(|element|element.path().contains(&gpui_kit::ElementId::from("enhance-property"))));
            assert!(!window.elements().iter().any(|element|element.path().contains(&gpui_kit::ElementId::from("enhance-part"))));
            assert!(window.try_find("enhancement-search").is_none());
            let code:serde_yaml_ng::Value=serde_yaml_ng::from_str(editor.read(cx).code.read(cx).value().as_ref()).unwrap();
            for key in super::PROPERTIES {assert!(code.get(key).is_some());}
            let before=editor.read(cx).drafts.clone();
            editor.read(cx).code.clone().update(cx,|state,cx|state.set_value("rules: []",window,cx));
            window.click("enhance-mode",cx);
            assert!(editor.read(cx).raw);assert_eq!(editor.read(cx).drafts,before);
            editor.read(cx).code.clone().update(cx,|state,cx|state.set_value("rules:\n  prepend: ['DOMAIN,added.example,DIRECT']\nproxies:\n  append: [{name: added, type: direct}]\nproxy-groups:\n  delete: [removed]\n",window,cx));
            window.click("enhance-mode", cx);
            assert_eq!(editor.read(cx).drafts[0]["prepend"].as_sequence().unwrap().len(),1);
            assert_eq!(editor.read(cx).drafts[1]["append"].as_sequence().unwrap().len(),1);
            assert_eq!(editor.read(cx).drafts[2]["delete"].as_sequence().unwrap().len(),1);
            assert!(!editor.read(cx).raw);
            editor.update(cx, |editor, cx| {
                editor.part = 1;
                cx.notify();
            });
            window.render_frame(cx);
            assert!(window.find("add-prepend").visible());
            let add=window.find("add-prepend").bounds();let yaml=window.find("enhance-mode").bounds();
            assert!(add.left()>=yaml.right());assert!((add.center().y-yaml.center().y).abs()<px(1.));
            window.click("add-prepend", cx);
            assert!(window.find("enhancement-entry-form").bounds().size.height < px(400.));
            if property==1 {
                let server=window.find_by_label("服务器").bounds();let port=window.find_by_label("端口").bounds();
                assert_eq!(server.top(),port.top());assert!(server.right()<=port.left());
            }

        })
        .unwrap();
        crate::integration::finish_test_services(cx);
    }
}
