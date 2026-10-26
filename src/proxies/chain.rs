use super::{ProxiesPage, model::SortOrder};
use crate::{
    integration as storage, ui,
    ui::theme::{control, layout, space, text},
};
use gpui_kit::{
    component::{
        IndexPath, Sizable, WindowExt, h_flex,
        input::{Editor, EditorState},
        select::{Select, SelectState},
        switch::Switch,
        v_flex,
    },
    *,
};
use std::collections::{BTreeMap, BTreeSet};

impl ProxiesPage {
    pub(super) fn edit_chain(&self, window: &mut Window, cx: &mut Context<Self>) {
        let nodes: Vec<SharedString> = self
            .groups
            .iter()
            .flat_map(|group| group.visible_nodes("", false, SortOrder::Default, false))
            .map(|node| SharedString::from(node.name()))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let editor = cx.new(|cx| ChainEditor::new(nodes, window, cx));
        window.open_dialog(cx, move |dialog, window, cx| {
            let editor = editor.clone();
            let body = editor.clone();
            ui::modal(dialog, "链式代理", layout::EDITOR_DIALOG_WIDTH, window, cx)
                .content(ui::modal_content(
                    "proxy-chain-body",
                    crate::ui::theme::layout::DIALOG_FORM_HEIGHT,
                    move |_, _| body.clone().into_any_element(),
                ))
                .footer(ui::dialog_footer("保存草稿"))
                .on_ok(move |_, w, cx| editor.update(cx, |editor, cx| editor.save(w, cx)))
        });
    }
}
struct ChainEditor {
    enabled: bool,
    entry: Entity<SelectState<Vec<SharedString>>>,
    exit: Entity<SelectState<Vec<SharedString>>>,
    source: Entity<EditorState>,
}
impl ChainEditor {
    fn new(nodes: Vec<SharedString>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let saved = storage::read("proxy-chain", cx);
        let entry_ix = nodes
            .iter()
            .position(|name| Some(name.as_ref()) == saved.get("entry").map(String::as_str))
            .unwrap_or(0);
        let exit_ix = nodes
            .iter()
            .position(|name| Some(name.as_ref()) == saved.get("exit").map(String::as_str))
            .unwrap_or_else(|| usize::from(nodes.len() > 1));
        let entry = cx
            .new(|cx| SelectState::new(nodes.clone(), Some(IndexPath::new(entry_ix)), window, cx));
        let exit = cx.new(|cx| SelectState::new(nodes, Some(IndexPath::new(exit_ix)), window, cx));
        let source = cx.new(|cx| {
            ui::editor::state(
                "yaml",
                saved.get("yaml").cloned().unwrap_or_default(),
                true,
                window,
                cx,
            )
        });
        Self {
            enabled: saved.get("enabled").is_some_and(|value| value == "true"),
            entry,
            exit,
            source,
        }
    }
    fn revision(&self, cx: &App) -> Vec<String> {
        vec![
            self.enabled.to_string(),
            self.source.read(cx).value().to_string(),
            self.entry
                .read(cx)
                .selected_value()
                .map(ToString::to_string)
                .unwrap_or_default(),
            self.exit
                .read(cx)
                .selected_value()
                .map(ToString::to_string)
                .unwrap_or_default(),
        ]
    }
    fn save(&self, w: &mut Window, cx: &mut Context<Self>) -> bool {
        let entry = self
            .entry
            .read(cx)
            .selected_value()
            .cloned()
            .unwrap_or_default()
            .to_string();
        let exit = self
            .exit
            .read(cx)
            .selected_value()
            .cloned()
            .unwrap_or_default()
            .to_string();
        if self.enabled && (entry.is_empty() || exit.is_empty() || entry == exit) {
            ui::show_error("选择两个不同的入口与出口节点".into(), w, cx);
            return false;
        }
        let yaml = self.source.read(cx).value().to_string();
        if !yaml.trim().is_empty()
            && let Err(error) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&yaml)
        {
            ui::show_error(error.to_string(), w, cx);
            return false;
        }
        storage::save_draft(
            "proxy-chain",
            BTreeMap::from([
                ("enabled".into(), self.enabled.to_string()),
                ("entry".into(), entry),
                ("exit".into(), exit),
                ("yaml".into(), yaml),
            ]),
            cx.weak_entity(),
            self.revision(cx),
            |editor, cx| editor.revision(cx),
            w,
            cx,
        );
        false
    }
}
impl Render for ChainEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap(space::LG)
            .child(
                h_flex().justify_between().child("启用链式代理").child(
                    Switch::new("chain-enabled")
                        .checked(self.enabled)
                        .accessibility_label("启用链式代理")
                        .on_change(cx.listener(|this, value, _, cx| {
                            this.enabled = *value;
                            cx.notify();
                        })),
                ),
            )
            .child(ui::field_label(
                "入口节点",
                Select::new(&self.entry)
                    .w_full()
                    .with_size(control::STANDARD)
                    .accessibility_label("入口节点"),
            ))
            .child(ui::field_label(
                "出口节点",
                Select::new(&self.exit)
                    .w_full()
                    .with_size(control::STANDARD)
                    .accessibility_label("出口节点"),
            ))
            .child(ui::muted("本机 → 入口节点 → 出口节点 → 目标网站", cx).text_size(text::CAPTION))
            .child(ui::field_label(
                "自定义链式配置（可选）",
                Editor::new(&self.source)
                    .w_full()
                    .h(layout::TEXTAREA_HEIGHT)
                    .aria_label("链式代理 YAML"),
            ))
    }
}
