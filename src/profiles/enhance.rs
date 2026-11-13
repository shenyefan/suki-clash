//! Local subscription enhancement drafts. No runtime configuration is generated here.
use super::model::Document;
use crate::storage::Values;
use crate::{
    integration as storage, ui,
    ui::theme::{control, layout, space, stroke},
};
use gpui_kit::{
    assets::IconName,
    component::{
        ActiveTheme, Disableable, Icon, IndexPath, Sizable, WindowExt,
        button::Button,
        combobox::{Combobox, ComboboxState},
        h_flex,
        input::{Editor, EditorState, Input, InputState},
        searchable_list::SearchableListItem,
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

fn saved_enhancement_text(saved: &Values) -> String {
    PROPERTIES
        .iter()
        .map(|key| {
            let source = saved.get(*key).map(String::as_str).unwrap_or("{}");
            // Valid fragments can contain document markers or root indentation;
            // normalize only those, and retain invalid fragment text for repair.
            let text = serde_yaml_ng::from_str::<Value>(source)
                .and_then(|value| serde_yaml_ng::to_string(&value))
                .unwrap_or_else(|_| source.to_owned());
            let indented = text
                .lines()
                .map(|line| format!("  {line}\n"))
                .collect::<String>();
            format!("{key}:\n{indented}")
        })
        .collect()
}

fn parse_enhancement(text: &str) -> Result<[Value; 3], String> {
    let value: Value = serde_yaml_ng::from_str(text).map_err(|error| match error.location() {
        Some(location) => format!(
            "增强 YAML 格式无效（第 {} 行，第 {} 列）",
            location.line(),
            location.column()
        ),
        None => "增强 YAML 格式无效".into(),
    })?;
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
            if map
                .get(Value::String(part.into()))
                .is_some_and(|value| !value.is_sequence())
            {
                return Err(format!("{property}.{part} 必须是列表"));
            }
        }
        drafts[index] = value.clone();
    }
    Ok(drafts)
}

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
        .content(ui::modal_panel(layout::FORM_MAX_HEIGHT, move |_, _| {
            body.clone().into_any_element()
        }))
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
    saved: bool,
    expected: Values,
    lists: BTreeMap<&'static str, component::VirtualListScrollHandle>,
    source: Value,
    drafts: [Value; 3],
    code: Entity<EditorState>,
    query: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
    importing: bool,
    import_task: Option<Task<()>>,
}
impl EnhancementEditor {
    fn new(
        document: Document,
        property: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let saved = storage::read(&format!("profile-enhance-{}", document.id), cx);
        let text = saved_enhancement_text(&saved);
        let (drafts, raw) = match parse_enhancement(&text) {
            Ok(drafts) => (drafts, false),
            Err(error) => {
                ui::show_error(error, window, cx);
                (
                    std::array::from_fn(|_| Value::Mapping(Mapping::new())),
                    true,
                )
            }
        };
        let code = cx.new(|cx| ui::editor::state("yaml", text, true, window, cx));
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("搜索名称或规则"));
        let subscriptions =
            vec![
                cx.subscribe(&query, |this, _, _: &component::input::InputEvent, cx| {
                    for handle in this.lists.values() {
                        handle.set_offset(Point::default());
                    }
                    cx.notify();
                }),
            ];
        Self {
            profile_id: document.id,
            property,
            raw,
            part: 1,
            saving: false,
            saved: false,
            expected: saved,
            lists: ["prepend", "original", "append", "delete"]
                .into_iter()
                .map(|name| (name, component::VirtualListScrollHandle::new()))
                .collect(),
            source: serde_yaml_ng::from_str(&document.content).unwrap_or_default(),
            drafts,
            code,
            query,
            _subscriptions: subscriptions,
            importing: false,
            import_task: None,
        }
    }
    fn read_code(&mut self, cx: &App) -> Result<(), String> {
        self.drafts = parse_enhancement(self.code.read(cx).value().as_ref())?;
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
                        editor.saved = result.is_ok();
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
    fn import_proxies(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = cx.new(|cx| ui::editor::state("text", String::new(), false, window, cx));
        let owner = cx.weak_entity();
        window.open_dialog(cx, move |dialog, window, cx| {
            let code = text.clone();
            let buttons = [("prepend", "前置导入"), ("append", "追加导入")]
                .into_iter()
                .map(|(part, label)| {
                    let owner = owner.clone();
                    let text = text.clone();
                    Button::new(SharedString::from(format!("import-{part}")))
                        .icon(IconName::FileInput)
                        .label(label)
                        .with_size(control::STANDARD)
                        .on_click(move |_, window, cx| {
                            let input = text.read(cx).value().to_string();
                            let owner = owner.clone();
                            let _ = owner.update(cx, |this, cx| {
                                if this.importing {
                                    return;
                                }
                                this.importing = true;
                                this.import_task =
                                    Some(cx.spawn_in(window, async move |owner, cx| {
                                        let result = cx
                                            .background_spawn(async move {
                                                super::proxy_uri::parse_batch(&input)
                                            })
                                            .await;
                                        let _ = owner.update_in(cx, |this, window, cx| {
                                            this.importing = false;
                                            match result {
                                                Ok(values) => {
                                                    let map = this.drafts[1].as_mapping_mut();
                                                    if let Some(map) = map {
                                                        let entry = map
                                                            .entry(Value::String(part.into()))
                                                            .or_insert_with(|| {
                                                                Value::Sequence(Vec::new())
                                                            });
                                                        if let Some(items) = entry.as_sequence_mut()
                                                        {
                                                            items.extend(values);
                                                        }
                                                    }
                                                    this.property = 1;
                                                    this.part =
                                                        if part == "prepend" { 1 } else { 3 };
                                                    window.close_dialog(cx);
                                                }
                                                Err(error) => ui::show_error(error, window, cx),
                                            }
                                            cx.notify();
                                        });
                                    }));
                                cx.notify();
                            });
                        })
                });
            let dismissed = owner.clone();
            ui::modal(
                dialog,
                "批量导入代理",
                layout::EDITOR_DIALOG_WIDTH,
                window,
                cx,
            )
            .on_close(move |_, _, cx| {
                let _ = dismissed.update(cx, |this, cx| {
                    this.import_task = None;
                    this.importing = false;
                    cx.notify();
                });
            })
            .content(ui::modal_content(
                "proxy-import-content",
                layout::FORM_MAX_HEIGHT,
                move |_, _| {
                    ui::field_label(
                        "代理链接（每行一个，或 Base64）",
                        Editor::new(&code)
                            .w_full()
                            .h(layout::EDITOR_HEIGHT)
                            .aria_label("批量代理链接"),
                    )
                    .into_any_element()
                },
            ))
            .footer(
                ui::modal_footer()
                    .child(
                        Button::new("proxy-import-cancel")
                            .icon(IconName::X)
                            .label("取消")
                            .with_size(control::STANDARD)
                            .on_click({
                                let owner = owner.clone();
                                move |_, window, cx| {
                                    let _ = owner.update(cx, |this, cx| {
                                        this.import_task = None;
                                        this.importing = false;
                                        cx.notify();
                                    });
                                    window.close_dialog(cx);
                                }
                            }),
                    )
                    .children(buttons),
            )
        });
    }
    #[cfg(test)]
    fn source_candidates(&self, cx: &App) -> Result<(Vec<String>, Vec<String>), String> {
        source_candidates(
            self.source.clone(),
            self.drafts.clone(),
            self.profile_id,
            super::store::load(true, cx)
                .map(|(docs, _)| docs)
                .unwrap_or_default(),
        )
    }
    fn edit_item(
        &mut self,
        part: &'static str,
        index: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = index.and_then(|index| self.items(part).get(index).cloned());
        if self.property == 0
            && let Some(value) = &value
        {
            let parsed = value
                .as_str()
                .ok_or("此规则请使用 YAML 编辑")
                .and_then(rule_parts);
            if let Err(error) = parsed {
                ui::show_error(error.into(), window, cx);
                return;
            }
        }
        let source = self.source.clone();
        let drafts = self.drafts.clone();
        let profile_id = self.profile_id;
        let documents = super::store::load(true, cx)
            .map(|(docs, _)| docs)
            .unwrap_or_default();
        let entry = cx.new(|cx| EntryEditor::new(self.property, value, window, cx));
        entry.update(cx, |this, cx| {
            this.sources_task = Some(cx.spawn_in(window, async move |owner, cx| {
                let result = cx
                    .background_spawn(async move {
                        source_candidates(source, drafts, profile_id, documents)
                    })
                    .await;
                let _ = owner.update_in(cx, |this, window, cx| {
                    match result {
                        Ok((members, providers)) => {
                            for (state, candidates) in
                                [(&this.members, members), (&this.providers, providers)]
                            {
                                state.update(cx, |state, cx| {
                                    let selected = state.selected_values();
                                    let mut items = candidates
                                        .into_iter()
                                        .map(SharedString::from)
                                        .collect::<Vec<_>>();
                                    for value in &selected {
                                        if !items.contains(value) {
                                            items.push(value.clone());
                                        }
                                    }
                                    state.set_items(items, window, cx);
                                    state.set_selected_values(&selected, window, cx);
                                });
                            }
                        }
                        Err(error) => ui::show_error(error, window, cx),
                    }
                    cx.notify();
                });
            }));
        });
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
                layout::FORM_MAX_HEIGHT,
                move |_, _| body.clone().into_any_element(),
            ))
            .footer(ui::dialog_footer("保存"))
            .on_ok(move |_, w, cx| {
                let result = entry.read(cx).value(cx);
                let value = match result {
                    Ok(value) => value,
                    Err(error) => {
                        entry.update(cx, |entry, cx| entry.restore_choices(w, cx));
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
                    .icon(IconName::Minus)
                    .outline()
                    .small()
                    .disabled(self.saving)
                    .label(if part == "original" {
                        if excluded { "恢复" } else { "排除" }
                    } else if part == "delete" {
                        "恢复"
                    } else {
                        "删除"
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let changed_part = if part == "original" { "delete" } else { part };
                        let property = this.property;
                        let before = this.items(changed_part);
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
                        let after = this.items(changed_part);
                        let profile_id = this.profile_id;
                        let owner = cx.weak_entity();
                        ui::notifications::undo(
                            window,
                            "已更新增强条目",
                            cx,
                            move |window, cx| {
                                if owner.upgrade().is_none_or(|editor| editor.read(cx).saved) {
                                    let expected =
                                        storage::read(&format!("profile-enhance-{profile_id}"), cx);
                                    let mut root: Value = match expected
                                        .get(PROPERTIES[property])
                                        .map(|s| serde_yaml_ng::from_str(s))
                                        .transpose()
                                    {
                                        Ok(Some(value)) => value,
                                        _ => {
                                            ui::show_error(
                                                "增强配置无法读取，无法撤销".into(),
                                                window,
                                                cx,
                                            );
                                            return false;
                                        }
                                    };
                                    if root
                                        .get(changed_part)
                                        .and_then(Value::as_sequence)
                                        .cloned()
                                        .unwrap_or_default()
                                        != after
                                    {
                                        ui::show_error(
                                            "条目已更改，无法撤销此次操作".into(),
                                            window,
                                            cx,
                                        );
                                        return false;
                                    }
                                    let Some(mapping) = root.as_mapping_mut() else {
                                        return false;
                                    };
                                    mapping.insert(
                                        changed_part.into(),
                                        Value::Sequence(before.clone()),
                                    );
                                    let mut values = expected.clone();
                                    values.insert(
                                        PROPERTIES[property].into(),
                                        serde_yaml_ng::to_string(&root).unwrap_or_default(),
                                    );
                                    return match crate::core::execute(
                                        crate::core::Command::Profiles(
                                            crate::core::ProfileCommand::Enhance {
                                                id: profile_id,
                                                expected,
                                                values,
                                            },
                                        ),
                                        window,
                                        cx,
                                        |_, _, _| {},
                                    ) {
                                        Ok(()) => true,
                                        Err(error) => {
                                            ui::show_error(error, window, cx);
                                            false
                                        }
                                    };
                                }
                                owner
                                    .update(cx, |this, cx| {
                                        if this.raw
                                            && let Err(error) = this.read_code(cx)
                                        {
                                            ui::show_error(error, window, cx);
                                            return false;
                                        }
                                        if this.saving
                                            || this.drafts[property]
                                                .get(changed_part)
                                                .and_then(Value::as_sequence)
                                                .cloned()
                                                .unwrap_or_default()
                                                != after
                                        {
                                            ui::show_error(
                                                "条目已更改，无法撤销此次操作".into(),
                                                window,
                                                cx,
                                            );
                                            return false;
                                        }
                                        if !this.drafts[property].is_mapping() {
                                            this.drafts[property] = Value::Mapping(Mapping::new());
                                        }
                                        this.drafts[property].as_mapping_mut().unwrap().insert(
                                            changed_part.into(),
                                            Value::Sequence(before.clone()),
                                        );
                                        if this.raw {
                                            let mapping: Mapping = PROPERTIES
                                                .iter()
                                                .enumerate()
                                                .map(|(index, key)| {
                                                    ((*key).into(), this.drafts[index].clone())
                                                })
                                                .collect();
                                            let text =
                                                serde_yaml_ng::to_string(&Value::Mapping(mapping))
                                                    .unwrap_or_default();
                                            this.code.update(cx, |state, cx| {
                                                state.set_value(text, window, cx)
                                            });
                                        }
                                        cx.notify();
                                        true
                                    })
                                    .unwrap_or(true)
                            },
                        );
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
        let editor = self.editor.clone();
        let state = editor.read(cx);
        let saving = state.saving;
        let raw = state.raw;
        let mode_owner = editor.clone();
        ui::modal_footer()
            .w_full()
            .child(div().flex_1())
            .child(
                Button::new("enhance-mode")
                    .icon(IconName::FileCode)
                    .outline()
                    .with_size(control::STANDARD)
                    .disabled(saving)
                    .label(if raw { "表单编辑" } else { "YAML 编辑" })
                    .on_click(move |_, window, cx| {
                        mode_owner.update(cx, |this, cx| this.mode(!this.raw, window, cx))
                    }),
            )
            .child(ui::dialog_footer_pending("保存", saving))
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
                None,
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
            .when(!self.raw, |view| {
                view.child(
                    h_flex()
                        .w_full()
                        .flex_shrink_0()
                        .gap(space::SM)
                        .child(
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
                        .child(div().flex_1())
                        .when(self.property == 1, |row| {
                            row.child(
                                Button::new("enhancement-import")
                                    .icon(IconName::FileInput)
                                    .outline()
                                    .with_size(control::STANDARD)
                                    .label("批量导入")
                                    .disabled(self.saving || self.importing)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.import_proxies(window, cx)
                                    })),
                            )
                        })
                        .child(
                            ui::icon_button("enhancement-add", IconName::Plus, "添加")
                                .disabled(self.saving)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    let part = if LIST_PARTS[this.part] == "append" {
                                        "append"
                                    } else {
                                        "prepend"
                                    };
                                    this.part = if part == "append" { 3 } else { 1 };
                                    this.edit_item(part, None, window, cx);
                                })),
                        ),
                )
            })
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
fn rule_parts(rule: &str) -> Result<Vec<&str>, &'static str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    let mut escaped = false;
    let mut quote = None;
    for (index, character) in rule.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        if let Some(delimiter) = quote {
            if character == delimiter {
                quote = None;
            }
            continue;
        }
        match character {
            '\'' | '"' => quote = Some(character),
            '(' => depth += 1,
            ')' => depth = depth.checked_sub(1).ok_or("此规则请使用 YAML 编辑")?,
            ',' if depth == 0 => {
                parts.push(&rule[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    if depth != 0 || quote.is_some() {
        return Err("此规则请使用 YAML 编辑");
    }
    parts.push(&rule[start..]);
    let required = if parts.first() == Some(&"MATCH") {
        2
    } else {
        3
    };
    if parts.len() < required {
        return Err("此规则请使用 YAML 编辑");
    }
    Ok(parts)
}

#[derive(Clone)]
struct EntryType {
    value: SharedString,
    label: SharedString,
}
impl SearchableListItem for EntryType {
    type Value = SharedString;
    fn title(&self) -> SharedString {
        self.label.clone()
    }
    fn value(&self) -> &Self::Value {
        &self.value
    }
}
fn type_label(value: &str) -> &str {
    match value {
        "select" => "手动选择",
        "url-test" => "自动测速",
        "fallback" => "故障转移",
        "load-balance" => "负载均衡",
        "relay" => "链式代理",
        "ss" => "影梭（SS）",
        "ssr" => "影梭 R（SSR）",
        "vmess" => "VMess 代理",
        "vless" => "VLESS 代理",
        "trojan" => "Trojan 代理",
        "hysteria2" => "Hysteria 2 代理",
        "tuic" => "TUIC 代理",
        "socks5" => "SOCKS 代理",
        "http" => "HTTP 代理",
        "wireguard" => "WireGuard 代理",
        "ssh" => "SSH 代理",
        "direct" => "直连",
        "DOMAIN" => "域名",
        "DOMAIN-SUFFIX" => "域名后缀",
        "DOMAIN-KEYWORD" => "域名关键词",
        "DOMAIN-REGEX" => "域名正则",
        "GEOSITE" => "站点集合",
        "SRC-GEOIP" => "来源 IP 地区",
        "IP-ASN" => "IP 自治系统",
        "SRC-IP-ASN" => "来源 IP 自治系统",
        "IP-SUFFIX" => "IP 后缀",
        "SRC-IP-SUFFIX" => "来源 IP 后缀",
        "IN-PORT" => "入站端口",
        "DSCP" => "差分服务标记",
        "PROCESS-NAME-REGEX" => "进程名称正则",
        "PROCESS-PATH-REGEX" => "进程路径正则",
        "UID" => "用户标识",
        "IN-TYPE" => "入站类型",
        "IN-USER" => "入站用户",
        "IN-NAME" => "入站名称",
        "SUB-RULE" => "子规则",
        "GEOIP" => "IP 地区",
        "IP-CIDR" => "IPv4 网段",
        "IP-CIDR6" => "IPv6 网段",
        "SRC-IP-CIDR" => "来源网段",
        "DST-PORT" => "目标端口",
        "SRC-PORT" => "来源端口",
        "PROCESS-NAME" => "进程名称",
        "PROCESS-PATH" => "进程路径",
        "RULE-SET" => "规则集合",
        "NETWORK" => "网络类型",
        "MATCH" => "最终匹配",
        "AND" => "全部满足",
        "OR" => "任一满足",
        "NOT" => "取反",
        _ => value,
    }
}
fn source_candidates(
    source: Value,
    drafts: [Value; 3],
    profile_id: u64,
    documents: Vec<Document>,
) -> Result<(Vec<String>, Vec<String>), String> {
    let mut nodes = ["DIRECT", "REJECT", "REJECT-DROP", "PASS"]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    for (property, draft) in [("proxies", &drafts[1]), ("proxy-groups", &drafts[2])] {
        let excluded = draft
            .get("delete")
            .and_then(Value::as_sequence)
            .into_iter()
            .flatten()
            .map(EnhancementEditor::item_name)
            .collect::<Vec<_>>();
        for value in source
            .get(property)
            .and_then(Value::as_sequence)
            .into_iter()
            .flatten()
            .chain(["prepend", "append"].into_iter().flat_map(|part| {
                draft
                    .get(part)
                    .and_then(Value::as_sequence)
                    .into_iter()
                    .flatten()
            }))
        {
            if let Some(name) = value.get("name").and_then(Value::as_str)
                && !excluded.iter().any(|removed| removed == name)
                && !nodes.iter().any(|node| node == name)
            {
                nodes.push(name.into());
            }
        }
    }
    let mut providers = source
        .get("proxy-providers")
        .and_then(Value::as_mapping)
        .into_iter()
        .flat_map(|providers| providers.keys())
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    {
        for document in documents.into_iter().filter(|document| {
            document.enabled
                && document.kind == super::model::DocumentKind::Yaml
                && document.scope_profile.is_none_or(|id| id == profile_id)
        }) {
            let value: Value = serde_yaml_ng::from_str(&document.content).map_err(|error| {
                crate::logging::rejected(
                    format!("覆写“{}”格式无效，无法读取来源候选", document.name),
                    error,
                )
            })?;
            for key in value
                .get("proxy-providers")
                .and_then(Value::as_mapping)
                .into_iter()
                .flat_map(|map| map.keys())
                .filter_map(Value::as_str)
            {
                if !providers.iter().any(|name| name == key) {
                    providers.push(key.into());
                }
            }
            for name in ["proxies", "proxy-groups"]
                .into_iter()
                .flat_map(|key| {
                    value
                        .get(key)
                        .and_then(Value::as_sequence)
                        .into_iter()
                        .flatten()
                })
                .filter_map(|value| value.get("name").and_then(Value::as_str))
            {
                if !nodes.iter().any(|node| node == name) {
                    nodes.push(name.into());
                }
            }
        }
    }
    Ok((nodes, providers))
}

const GROUP_FIELDS: &[(&str, &str)] = &[
    ("name", ""),
    ("type", "select"),
    ("icon", ""),
    ("url", ""),
    ("expected-status", ""),
    ("interval", ""),
    ("timeout", ""),
    ("max-failed-times", ""),
    ("interface-name", ""),
    ("routing-mark", ""),
    ("filter", ""),
    ("exclude-filter", ""),
    ("exclude-type", ""),
];
const GROUP_SWITCHES: &[(&str, &str)] = &[
    ("include-all", "包含全部来源"),
    ("include-all-proxies", "包含全部代理"),
    ("include-all-providers", "包含全部代理集合"),
    ("lazy", "惰性测速"),
    ("disable-udp", "禁用 UDP"),
    ("hidden", "隐藏代理组"),
];
struct EntryEditor {
    property: usize,
    original: Value,
    fields: BTreeMap<&'static str, Entity<InputState>>,
    types: Entity<SelectState<Vec<EntryType>>>,
    initial_type: SharedString,
    switches: BTreeMap<&'static str, bool>,
    providers: Entity<ComboboxState<Vec<SharedString>>>,
    sources_task: Option<Task<()>>,
    no_resolve: bool,
    rule_suffix: Vec<String>,
    members: Entity<ComboboxState<Vec<SharedString>>>,
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
        let rule = rule_parts(original.as_str().unwrap_or("DOMAIN-SUFFIX,,DIRECT"))
            .unwrap_or_else(|_| vec!["DOMAIN-SUFFIX", "", "DIRECT"]);
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
            _ => GROUP_FIELDS,
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
        let choices = choices
            .into_iter()
            .map(|value| EntryType {
                label: type_label(&value).to_owned().into(),
                value,
            })
            .collect::<Vec<_>>();
        let types = cx.new(|cx| SelectState::new(choices, Some(IndexPath::new(index)), window, cx));
        let suffix_start = if rule.first() == Some(&"MATCH") { 2 } else { 3 };
        let suffix = rule.get(suffix_start..).unwrap_or_default();
        let no_resolve = suffix.iter().any(|part| part.trim() == "no-resolve");
        let rule_suffix = suffix.iter().map(|value| (*value).to_owned()).collect();
        let selection = |key: &str, window: &mut Window, cx: &mut Context<Self>| {
            let items = original
                .get(key)
                .and_then(Value::as_sequence)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(SharedString::from)
                .collect::<Vec<_>>();
            let selected = (0..items.len()).map(IndexPath::new).collect();
            cx.new(|cx| {
                ComboboxState::new(items, selected, window, cx)
                    .multiple(true)
                    .searchable(true)
            })
        };
        let members = selection("proxies", window, cx);
        let providers = selection("use", window, cx);
        let switches = GROUP_SWITCHES
            .iter()
            .map(|(key, _)| {
                (
                    *key,
                    original.get(*key).and_then(Value::as_bool).unwrap_or(false),
                )
            })
            .collect();
        let subscription = cx.observe(&types, |_, _, cx| cx.notify());
        Self {
            property,
            original,
            fields,
            types,
            initial_type: current.into(),
            switches,
            providers,
            sources_task: None,
            no_resolve,
            rule_suffix,
            members,
            _types: subscription,
        }
    }
    fn restore_choices(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.types.update(cx, |state, cx| {
            state.set_selected_value(&self.initial_type, window, cx)
        });
        self.no_resolve = self
            .rule_suffix
            .iter()
            .any(|part| part.trim() == "no-resolve");
        for (key, checked) in &mut self.switches {
            *checked = self
                .original
                .get(*key)
                .and_then(Value::as_bool)
                .unwrap_or(false);
        }
        for (key, state) in [("proxies", &self.members), ("use", &self.providers)] {
            let selected = self
                .original
                .get(key)
                .and_then(Value::as_sequence)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(SharedString::from)
                .collect::<Vec<_>>();
            state.update(cx, |state, cx| {
                state.set_selected_values(&selected, window, cx)
            });
        }
        cx.notify();
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
                let value = self.fields[key].read(cx).value();
                if key == "password" {
                    value.to_string()
                } else {
                    value.trim().to_owned()
                }
            }
        };
        if self.property == 0 {
            let kind = get("type");
            let payload = get("payload");
            let policy = get("policy");
            if kind.is_empty() || policy.is_empty() || kind != "MATCH" && payload.is_empty() {
                return Err("填写规则类型、匹配内容和策略".into());
            }
            let mut parts = if kind == "MATCH" {
                vec![kind, policy]
            } else {
                vec![kind, payload, policy]
            };
            parts.extend(
                self.rule_suffix
                    .iter()
                    .filter(|part| part.trim() != "no-resolve" || self.no_resolve)
                    .cloned(),
            );
            if self.no_resolve
                && !self
                    .rule_suffix
                    .iter()
                    .any(|part| part.trim() == "no-resolve")
            {
                parts.push("no-resolve".into());
            }
            let text = parts.join(",");
            // Preserve the original text when the form did not change its meaning.
            if let Some(original) = self.original.as_str()
                && rule_parts(original).is_ok_and(|original| {
                    original
                        .iter()
                        .map(|part| part.trim())
                        .eq(parts.iter().map(String::as_str))
                })
            {
                return Ok(self.original.clone());
            }
            return Ok(Value::String(text));
        }
        if get("name").is_empty() || get("type").is_empty() {
            return Err("填写名称和类型".into());
        }
        let mut map = self.original.as_mapping().cloned().unwrap_or_default();
        for key in self.fields.keys() {
            let text = get(key);
            if self.property == 2 && text.is_empty() && !matches!(*key, "name" | "type") {
                map.remove(Value::String((*key).into()));
                continue;
            }
            let value = if self.property == 2
                && matches!(
                    *key,
                    "interval" | "timeout" | "max-failed-times" | "routing-mark"
                ) {
                Value::Number(
                    text.parse::<u32>()
                        .map_err(|error| {
                            crate::logging::rejected(
                                format!("{}需为非负整数", group_field_label(key)),
                                error,
                            )
                        })?
                        .into(),
                )
            } else if *key == "port" {
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
            for (key, editor) in [("proxies", &self.members), ("use", &self.providers)] {
                let values = editor
                    .read(cx)
                    .selected_values()
                    .into_iter()
                    .map(|value| Value::String(value.to_string()))
                    .collect::<Vec<_>>();
                if values.is_empty() && !map.contains_key(Value::String(key.into())) {
                    continue;
                }
                map.insert(Value::String(key.into()), Value::Sequence(values));
            }
            for (key, value) in &self.switches {
                if *value || map.contains_key(Value::String((*key).into())) {
                    map.insert(Value::String((*key).into()), Value::Bool(*value));
                }
            }
            let url = get("url");
            if !url.is_empty() && !super::model::valid_remote_url(&url) {
                return Err("健康检查地址需为 HTTP 或 HTTPS 地址".into());
            }
        }
        Ok(Value::Mapping(map))
    }
}
fn group_field_label(key: &str) -> &'static str {
    match key {
        "icon" => "图标地址",
        "url" => "健康检查地址",
        "expected-status" => "预期状态码",
        "interval" => "检查间隔（秒）",
        "timeout" => "检查超时（毫秒）",
        "max-failed-times" => "最大失败次数",
        "interface-name" => "网络接口",
        "routing-mark" => "路由标记",
        "filter" => "名称筛选",
        "exclude-filter" => "排除名称",
        "exclude-type" => "排除类型（以 | 分隔）",
        _ => "字段",
    }
}
impl EntryEditor {
    fn source_editor(&self, provider: bool) -> impl IntoElement {
        ui::field_label(
            if provider {
                "代理集合来源"
            } else {
                "代理组成员"
            },
            Combobox::new(if provider {
                &self.providers
            } else {
                &self.members
            })
            .w_full()
            .with_size(control::STANDARD)
            .placeholder("选择来源")
            .search_placeholder("搜索来源"),
        )
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
                            .items_center()
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
                form = form
                    .child(self.source_editor(false))
                    .child(self.source_editor(true));
                for (key, _) in GROUP_FIELDS
                    .iter()
                    .filter(|(key, _)| !matches!(*key, "name" | "type"))
                {
                    form = form.child(field(*key, group_field_label(key)));
                }
                for (key, label) in GROUP_SWITCHES {
                    let key = *key;
                    form = form.child(
                        ui::form_row()
                            .items_center()
                            .justify_between()
                            .child(*label)
                            .child(
                                Switch::new(SharedString::from(format!("enhance-{key}")))
                                    .with_size(control::STANDARD)
                                    .checked(self.switches[&key])
                                    .accessibility_label(*label)
                                    .on_change(cx.listener(move |this, value, _, cx| {
                                        this.switches.insert(key, *value);
                                        cx.notify();
                                    })),
                            ),
                    );
                }
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

    #[test]
    fn broken_enhancement_keeps_source_and_rejects_serialization_until_repaired() {
        let source = "prepend: [DOMAIN,private.example,DIRECT\n";
        let saved = crate::storage::Values::from([
            ("rules".into(), source.into()),
            (
                "proxies".into(),
                "---\nappend: [{name: unchanged, type: direct}]\n".into(),
            ),
        ]);
        let text = super::saved_enhancement_text(&saved);
        assert!(text.contains("  prepend: [DOMAIN,private.example,DIRECT\n"));
        assert!(super::parse_enhancement(&text).is_err());
        let repaired = text.replace(
            "prepend: [DOMAIN,private.example,DIRECT",
            "prepend: ['DOMAIN,private.example,DIRECT']",
        );
        let drafts = super::parse_enhancement(&repaired).unwrap();
        assert_eq!(drafts[1]["append"][0]["name"], "unchanged");
    }

    #[gpui_kit::test]
    fn malformed_enhancement_opens_raw_and_cannot_be_saved_as_empty_form(cx: &mut TestAppContext) {
        let root = tempfile::tempdir().unwrap();
        let repository = crate::integration::Repository::open(
            crate::integration::AppPaths::isolated(root.path()),
        )
        .unwrap();
        let namespace = crate::storage::Namespace::parse("profile-enhance-90002").unwrap();
        let bad = "prepend: [broken\n";
        repository
            .save(
                &namespace,
                &crate::storage::Values::from([("rules".into(), bad.into())]),
            )
            .unwrap();
        let service = crate::integration::Service::start(repository).unwrap();
        cx.update(|cx| {
            crate::integration::init(service, None, cx);
            gpui_kit::init(cx);
        });
        let (handle, _) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| Host)).unwrap()
        });
        cx.update_window(handle, |_, window, cx| {
            let mut document = Document::new(90002, DocumentKind::Profile);
            document.content = "rules: []".into();
            let editor = open(document, 0, window, cx);
            assert!(editor.read(cx).raw);
            let raw = editor.read(cx).code.read(cx).value().to_string();
            assert!(raw.contains("  prepend: [broken\n"));
            editor.update(cx, |editor, cx| {
                editor.mode(false, window, cx);
                assert!(editor.raw);
                assert!(!editor.save(window, cx));
                assert!(!editor.saving);
                assert_eq!(editor.code.read(cx).value().as_ref(), raw);
            });
        })
        .unwrap();
        crate::integration::finish_test_services(cx);
    }

    #[gpui_kit::test]
    fn group_fields_preserve_unknown_values_and_use_stable_localized_types(
        cx: &mut TestAppContext,
    ) {
        use gpui_kit::component::searchable_list::SearchableListItem as _;
        cx.update(gpui_kit::init);
        let (handle, _) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| Host)).unwrap()
        });
        cx.update_window(handle, |_,window,cx| {
            let original = serde_yaml_ng::from_str("name: group\ntype: url-test\nuse: [provider]\nurl: https://example.test/check\ninterval: 300\ntimeout: 5000\ninclude-all-providers: true\nfilter: HK\nexclude-type: Direct|Reject\ncustom-field: {kept: true}\n").unwrap();
            let editor = cx.new(|cx|super::EntryEditor::new(2,Some(original),window,cx));
            let result=editor.read(cx).value(cx).unwrap();
            assert_eq!(result["type"],"url-test");
            assert_eq!(result["use"][0],"provider");
            assert_eq!(result["interval"],300);
            assert_eq!(result["include-all-providers"],true);
            assert_eq!(result["custom-field"]["kept"],true);
            assert!(result.get("proxies").is_none());
            assert_eq!(super::EntryType {value:"url-test".into(),label:super::type_label("url-test").into()}.title().as_ref(),"自动测速");
            editor.read(cx).fields["timeout"].clone().update(cx,|state,cx|state.set_value("bad",window,cx));
            assert!(editor.read(cx).value(cx).is_err());
            editor.read(cx).fields["timeout"].clone().update(cx,|state,cx|state.set_value("1200",window,cx));
            editor.update(cx,|this,cx| { this.providers.update(cx,|state,cx|{ state.set_items(vec!["one".into(), "two".into()],window,cx); state.set_selected_values(&["one".into(), "two".into()],window,cx); });this.switches.insert("hidden",true); });
            let result=editor.read(cx).value(cx).unwrap();assert_eq!(result["timeout"],1200);assert_eq!(result["use"][1],"two");assert_eq!(result["hidden"],true);
        }).unwrap();
    }

    #[gpui_kit::test]
    fn source_candidates_include_original_and_draft_sources_without_excluded_names(
        cx: &mut TestAppContext,
    ) {
        let root = tempfile::tempdir().unwrap();
        let repository = crate::integration::Repository::open(
            crate::integration::AppPaths::isolated(root.path()),
        )
        .unwrap();
        let service = crate::integration::Service::start(repository).unwrap();
        cx.update(|cx| {
            crate::integration::init(service, None, cx);
            gpui_kit::init(cx);
        });
        let (handle, _) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| Host)).unwrap()
        });
        cx.update_window(handle,|_,window,cx|{
            let mut document=Document::new(99801,DocumentKind::Profile);
            document.content="proxies: [{name: one, type: direct}, {name: removed, type: direct}]\nproxy-groups: [{name: group, type: select}]\nproxy-providers: {provider: {type: inline, payload: []}}".into();
            let editor=open(document,2,window,cx);
            editor.update(cx,|this,cx|{
                this.drafts[1]=serde_yaml_ng::from_str("prepend: [{name: added, type: direct}]\ndelete: [removed]").unwrap();
                let (nodes,providers)=this.source_candidates(cx).unwrap();
                assert!(nodes.contains(&"DIRECT".into()));assert!(nodes.contains(&"one".into()));assert!(nodes.contains(&"group".into()));assert!(nodes.contains(&"added".into()));assert!(!nodes.contains(&"removed".into()));assert_eq!(providers,vec!["provider"]);
            });
        }).unwrap();
        crate::integration::finish_test_services(cx);
    }

    #[gpui_kit::test]
    fn composite_rule_and_secret_forms_keep_unedited_content(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (handle, _) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| Host)).unwrap()
        });
        cx.update_window(handle, |_, window, cx| {
            for rule in [
                "AND,((DOMAIN,example.com),(NETWORK,TCP)),DIRECT",
                "OR,((DOMAIN,example.com),(NOT,((NETWORK,UDP)))),PROXY",
                "NOT,((DOMAIN,example.com)),DIRECT,no-resolve",
                "IP-CIDR,192.0.2.0/24,DIRECT,no-resolve,src",
            ] {
                let editor = cx.new(|cx| super::EntryEditor::new(0, Some(rule.into()), window, cx));
                assert_eq!(editor.read(cx).value(cx).unwrap().as_str(), Some(rule));
                editor.read(cx).fields["policy"].clone().update(cx, |input, cx| input.set_value("NEW", window, cx));
                let changed = editor.read(cx).value(cx).unwrap();
                let old = super::rule_parts(rule).unwrap();
                let new = super::rule_parts(changed.as_str().unwrap()).unwrap();
                assert_eq!(new[1], old[1]);
                assert_eq!(new[2], "NEW");
                assert_eq!(&new[3..], &old[3..]);
            }
            let original = serde_yaml_ng::from_str("name: node\ntype: ss\nserver: example.com\nport: 443\ncipher: aes-128-gcm\npassword: ' secret '\n").unwrap();
            let editor = cx.new(|cx| super::EntryEditor::new(1, Some(original), window, cx));
            editor.read(cx).fields["name"].clone().update(cx, |input, cx| input.set_value("renamed", window, cx));
            let saved = editor.read(cx).value(cx).unwrap();
            assert_eq!(saved["name"], "renamed");
            assert_eq!(saved["password"], " secret ");
        }).unwrap();
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
            crate::integration::init(service, None, cx);
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
            let footer = window.within("dialog-0").find("dialog-commit").bounds();
            let cancel = window.find("dialog-cancel").bounds();
            assert!(mode.right() <= cancel.left());
            assert!((mode.center().y - cancel.center().y).abs() < px(1.));
            let parts=window.within("enhance-part").find(0usize).bounds();
            assert!(parts.left()>=search.right());
            assert!((parts.center().y-search.center().y).abs()<px(1.));
            assert!(window.within("enhance-property").find(0usize).visible());

            let add = window.find("enhancement-add").bounds();
            let tabs = window.within("enhance-property").find(0usize).bounds();
            assert!(add.left() > tabs.right());
            assert!((add.center().y - tabs.center().y).abs() < px(1.));
            let viewport = window.find("enhancement-viewport").bounds();
            assert!(mode.left() > viewport.center().x);
            assert!((footer.right() - viewport.right()).abs() <= px(1.));
            assert!(footer.size.width < viewport.size.width / 3.);
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
            assert_eq!(window.within("dialog-0").find("dialog-commit").bounds(), footer);
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
            assert!(window.find("enhancement-add").visible());
            let add=window.find("enhancement-add").bounds();let property_tab=window.within("enhance-property").find(0usize).bounds();
            assert!(add.left()>=property_tab.right());assert!((add.center().y-property_tab.center().y).abs()<px(1.));
            window.click("enhancement-add", cx);
            if property != 2 { assert!(window.find("enhancement-entry-form").bounds().size.height < px(400.)); }
            else {
                let footer = window.within("dialog-1").find("dialog-commit").bounds();
                assert!(window.find("field-名称").visible());
                window.scroll("field-名称", ScrollDelta::Pixels(point(px(0.), px(-3000.))), cx);
                window.render_frame(cx);
                assert_eq!(window.within("dialog-1").find("dialog-commit").bounds(), footer);
                assert!(window.find("enhance-hidden").visible());
            }
            if property==1 {
                let server=window.find("field-服务器").bounds();let port=window.find("field-端口").bounds();
                assert_eq!(server.top(),port.top());assert!(server.right()<=port.left());
            }

        })
        .unwrap();
        crate::integration::finish_test_services(cx);
    }
}
