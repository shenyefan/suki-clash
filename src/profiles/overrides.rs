use super::{ProfilesPage, model::Document};
use crate::{
    integration, ui,
    ui::theme::{control, space, text},
};
use gpui_kit::{
    assets::IconName,
    component::{
        Disableable, Sizable,
        button::{Button, ButtonVariants},
        h_flex,
        menu::{DropdownMenu, PopupMenuItem},
        scroll::ScrollableElement,
        switch::Switch,
        v_flex,
    },
    *,
};

impl ProfilesPage {
    fn update_override(
        &mut self,
        id: u64,
        offset: isize,
        enabled: Option<bool>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.documents.iter().position(|doc| doc.id == id) else {
            return;
        };
        let mut documents = self.documents.clone();
        if let Some(enabled) = enabled {
            documents[index].enabled = enabled;
        } else {
            let target = index.saturating_add_signed(offset);
            if target >= documents.len() {
                return;
            }
            documents.swap(index, target);
        }
        if let Err(error) = self.persist_documents(documents, None, window, cx, |_, _, _| {}) {
            ui::show_error(error, window, cx);
        }
    }

    fn override_row(
        &self,
        document: &Document,
        index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = document.id;
        let owner = cx.entity();
        let remote = document.is_remote();
        let script = document.kind == super::model::DocumentKind::JavaScript;
        let menu = ui::icon_button(("override-menu", id), IconName::Ellipsis, "覆写操作")
            .dropdown_menu(move |mut menu, _, _| {
                for action in ["更新", "编辑信息…", "打开文件", "执行日志…", "删除"]
                {
                    if action == "更新" && !remote {
                        continue;
                    }
                    if action == "执行日志…" && !script {
                        continue;
                    }
                    let owner = owner.clone();
                    menu = menu.item(PopupMenuItem::new(action).on_click(move |_, w, cx| {
                        owner.update(cx, |this, cx| this.document_action(id, action, w, cx));
                    }));
                }
                menu
            });
        h_flex()
            .w_full()
            .gap(space::MD)
            .px(space::LG)
            .py(space::MD)
            .child(ui::muted(format!("{:02}", index + 1), cx).text_size(text::CAPTION))
            .child(
                v_flex()
                    .flex_1()
                    .min_w(space::NONE)
                    .gap(space::XS)
                    .child(
                        div()
                            .font_weight(text::EMPHASIS)
                            .truncate()
                            .child(document.name.clone()),
                    )
                    .child(
                        ui::muted(
                            format!(
                                "{} · {} · {} 行",
                                document.stage(),
                                document
                                    .scope_profile
                                    .map(|_| document.scope_name.as_str())
                                    .unwrap_or("所有订阅"),
                                document.content.lines().count()
                            ),
                            cx,
                        )
                        .text_size(text::CAPTION),
                    ),
            )
            .child(
                ui::icon_button(("override-up", id), IconName::ArrowUp, "上移")
                    .disabled(index == 0)
                    .on_click(
                        cx.listener(move |this, _, w, cx| {
                            this.update_override(id, -1, None, w, cx)
                        }),
                    ),
            )
            .child(
                ui::icon_button(("override-down", id), IconName::ArrowDown, "下移")
                    .disabled(index + 1 == self.documents.len())
                    .on_click(
                        cx.listener(move |this, _, w, cx| this.update_override(id, 1, None, w, cx)),
                    ),
            )
            .child(
                Button::new(("override-edit", id))
                    .ghost()
                    .with_size(control::STANDARD)
                    .icon(IconName::Pencil)
                    .label("编辑…")
                    .on_click(cx.listener(move |this, _, w, cx| {
                        this.document_action(id, "编辑文件…", w, cx)
                    })),
            )
            .child(
                Switch::new(("override-enabled", id))
                    .with_size(control::STANDARD)
                    .checked(document.enabled)
                    .accessibility_label(format!("启用{}", document.name))
                    .on_change(cx.listener(move |this, enabled, w, cx| {
                        this.update_override(id, 0, Some(*enabled), w, cx)
                    })),
            )
            .child(menu)
            .into_any_element()
    }

    pub(super) fn overrides_view(&self, cx: &mut Context<Self>) -> Div {
        let enabled = self.documents.iter().filter(|doc| doc.enabled).count();
        ui::workspace()
            .child(
                ui::workspace_header("覆写", "", cx).child(
                    Button::new("apply-overrides")
                        .outline()
                        .with_size(control::STANDARD)
                        .icon(IconName::CheckCheck)
                        .label("应用覆写")
                        .on_click(cx.listener(|_, _, w, cx| {
                            integration::request("应用覆写", w, cx);
                        })),
                ),
            )
            .child(
                ui::workspace_toolbar()
                    .child(
                        div().flex_1().min_w(space::NONE).child(
                            gpui_kit::component::input::Input::new(&self.url)
                                .with_size(control::STANDARD)
                                .w_full()
                                .aria_label("远程覆写地址"),
                        ),
                    )
                    .child(self.import_button(cx)),
            )
            .child(
                ui::workspace_body()
                    .id("override-list")
                    .overflow_y_scrollbar()
                    .on_drop(cx.listener(|this, paths: &ExternalPaths, w, cx| {
                        this.import_paths(paths.paths().to_vec(), w, cx)
                    }))
                    .child(if self.documents.is_empty() {
                        ui::empty(
                            IconName::FileCode,
                            "暂无覆写",
                            "导入覆写文件，或从导入菜单新建 YAML / JavaScript。",
                            cx,
                        )
                        .into_any_element()
                    } else {
                        v_flex()
                            .gap(space::LG)
                            .child(
                                ui::muted("启用项按列表顺序执行，作用范围在编辑信息中设置。", cx)
                                    .text_size(text::CAPTION),
                            )
                            .child(ui::preferences_group(
                                "覆写文件",
                                self.documents
                                    .iter()
                                    .enumerate()
                                    .map(|(index, doc)| self.override_row(doc, index, cx))
                                    .collect(),
                                cx,
                            ))
                            .into_any_element()
                    }),
            )
            .child(
                ui::workspace_footer()
                    .child(
                        ui::muted(
                            format!("{} 个文件 · {enabled} 个已启用", self.documents.len()),
                            cx,
                        )
                        .text_size(text::CAPTION),
                    )
                    .child(div().flex_1())
                    .child(ui::muted("作用范围在编辑信息中设置", cx).text_size(text::CAPTION)),
            )
    }
}
