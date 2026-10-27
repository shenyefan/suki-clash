use super::{ProfilesPage, model::Document};
use crate::{
    ui,
    ui::theme::{control, space, text},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    assets::IconName,
    component::{
        ActiveTheme, Disableable, Sizable,
        button::{Button, ButtonVariants},
        h_flex,
        menu::DropdownMenu,
        switch::Switch,
        v_flex,
    },
    *,
};

impl ProfilesPage {
    fn update_override(
        &mut self,
        id: u64,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut documents = self.documents.clone();
        let Some(document) = documents.iter_mut().find(|doc| doc.id == id) else {
            return;
        };
        document.enabled = enabled;
        let draft = documents.clone();
        match self.persist_documents(documents, None, window, cx, |_, _, _| {}) {
            Ok(()) => self.documents = draft,
            Err(error) => ui::show_error(error, window, cx),
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
                for action in ["更新", "编辑信息", "打开文件", "执行日志", "删除"] {
                    if action == "更新" && !remote {
                        continue;
                    }
                    if action == "执行日志" && !script {
                        continue;
                    }
                    let owner = owner.clone();
                    menu = menu.item(crate::ui::actions::menu(action).on_click(move |_, w, cx| {
                        owner.update(cx, |this, cx| this.document_action(id, action, w, cx));
                    }));
                }
                menu
            });
        h_flex()
            .id(("override-row", id))
            .when(!self.saving && !self.session.read(cx).core_busy(), |row| {
                row.on_drag(
                    super::organize::DocumentDrag {
                        id,
                        name: document.name.clone(),
                        overrides: true,
                    },
                    |drag, _, _, cx| cx.new(|_| drag.clone()),
                )
            })
            .drag_over::<super::organize::DocumentDrag>(|style, _, _, cx| {
                style.bg(cx.theme().accent)
            })
            .on_drop(cx.listener(
                move |this, drag: &super::organize::DocumentDrag, window, cx| {
                    if drag.overrides {
                        this.move_document(drag.id, id, window, cx);
                    }
                },
            ))
            .w_full()
            .gap(space::MD)
            .px(space::LG)
            .py(space::MD)
            .child(
                gpui_kit::component::Icon::new(IconName::ArrowDownUp)
                    .size(crate::ui::theme::layout::ICON)
                    .text_color(cx.theme().muted_foreground),
            )
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
                        .text_size(text::CAPTION)
                        .truncate(),
                    ),
            )
            .child(
                Button::new(("override-edit", id))
                    .ghost()
                    .with_size(control::STANDARD)
                    .icon(IconName::Pencil)
                    .label("编辑")
                    .on_click(cx.listener(move |this, _, w, cx| {
                        this.document_action(id, "编辑文件", w, cx)
                    })),
            )
            .child(
                Switch::new(("override-enabled", id))
                    .with_size(control::STANDARD)
                    .checked(document.enabled)
                    .disabled(self.saving || self.session.read(cx).core_busy())
                    .accessibility_label(format!("启用{}", document.name))
                    .on_change(cx.listener(move |this, enabled, w, cx| {
                        this.update_override(id, *enabled, w, cx)
                    })),
            )
            .child(menu)
            .into_any_element()
    }

    pub(super) fn overrides_view(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let enabled = self.documents.iter().filter(|doc| doc.enabled).count();
        ui::workspace()
            .child(ui::workspace_header("覆写", "", cx))
            .child(
                ui::workspace_toolbar()
                    .child(
                        div().flex_1().min_w(space::NONE).child(
                            gpui_kit::component::input::Input::new(&self.url)
                                .aria_label("覆写导入地址")
                                .with_size(control::STANDARD)
                                .w_full()
                                .aria_label("远程覆写地址"),
                        ),
                    )
                    .child(self.import_button(cx))
                    .child(
                        Button::new("update-all-overrides")
                            .outline()
                            .with_size(control::STANDARD)
                            .icon(IconName::RefreshCw)
                            .label("全部更新")
                            .disabled(
                                self.saving
                                    || self.session.read(cx).core_busy()
                                    || !self.documents.iter().any(|doc| doc.is_remote()),
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                let ids = this
                                    .documents
                                    .iter()
                                    .filter(|doc| doc.is_remote())
                                    .map(|doc| doc.id)
                                    .collect();
                                this.update_profiles(ids, window, cx);
                            })),
                    ),
            )
            .child(
                ui::workspace_body()
                    .id("override-list")
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
                        let height =
                            crate::ui::theme::list::DETAIL_ROW_HEIGHT.to_pixels(window.rem_size());
                        v_flex()
                            .size_full()
                            .gap(space::LG)
                            .child(
                                ui::muted("启用项按列表顺序执行，作用范围在编辑信息中设置。", cx)
                                    .text_size(text::CAPTION),
                            )
                            .child(ui::section("覆写文件", cx).px(space::LG))
                            .child(
                                div()
                                    .flex_1()
                                    .min_h(space::NONE)
                                    .rounded(cx.theme().radius_lg)
                                    .bg(ui::theme::preferences_tint(cx))
                                    .child(ui::virtual_rows::rows(
                                        cx.entity(),
                                        "override-rows",
                                        vec![height; self.documents.len()],
                                        &self.list_scroll,
                                        move |this, index, _, cx| {
                                            this.override_row(&this.documents[index], index, cx)
                                        },
                                    )),
                            )
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

#[cfg(test)]
mod tests {
    use super::{Document, ProfilesPage};
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{AppContext, TestAppContext, WindowOptions};
    #[gpui_kit::test]
    fn consecutive_override_commits_use_lifecycle_index_and_have_no_apply_button(
        cx: &mut TestAppContext,
    ) {
        let root = tempfile::tempdir().unwrap();
        let service = crate::integration::Service::start(
            crate::storage::Repository::open(crate::storage::AppPaths::isolated(root.path()))
                .unwrap(),
        )
        .unwrap();
        cx.update(|cx| {
            crate::integration::init(service, cx);
            gpui_kit::init(cx);
            cx.set_reduce_motion(true);
        });
        let (window, page) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                let session = cx.new(crate::session::Session::new);
                cx.new(|cx| ProfilesPage::new(true, session, window, cx))
            })
            .unwrap()
        });
        for enabled in [true, false, true] {
            let mut document = Document::new(7, super::super::model::DocumentKind::Yaml);
            document.enabled = enabled;
            let expected = super::super::store::values(&[document], None);
            cx.update(|cx| {
                crate::core::controller(cx).update(cx, |owner, cx| {
                    owner.publish_test_settings("override-index", expected.clone(), cx)
                });
            });
            cx.run_until_parked();
            window
                .update(cx, |_, window, cx| {
                    window.render_frame(cx);
                    assert_eq!(crate::integration::read("override-index", cx), expected);
                    assert_eq!(page.read(cx).documents[0].enabled, enabled);
                    assert!(window.try_find("apply-overrides").is_none());
                })
                .unwrap();
        }
        crate::integration::finish_test_services(cx);
    }
}
