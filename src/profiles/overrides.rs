use super::{ProfilesPage, model::Document};
use crate::{
    ui,
    ui::theme::{control, layout, space, stroke, text},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    assets::IconName,
    component::{
        ActiveTheme, Disableable, Sizable, button::Button, h_flex, list::ListItem,
        menu::DropdownMenu, switch::Switch, v_flex,
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
        if let Err(error) = self.persist_documents(documents, None, window, cx, |_, _, _| {}) {
            ui::show_error(error, window, cx);
        }
    }

    fn override_row(&self, document: &Document, cx: &mut Context<Self>) -> AnyElement {
        let id = document.id;
        let owner = cx.entity();
        let remote = document.is_remote();
        let script = document.kind == super::model::DocumentKind::JavaScript;
        let draggable = !self.saving && !self.session.read(cx).core_busy();
        let after = self.override_drop == Some((id, true));
        let drag_owner = cx.entity();
        let menu = ui::icon_button(("override-menu", id), IconName::Ellipsis, "覆写操作")
            .dropdown_menu(move |mut menu, _, _| {
                for (action, icon) in [
                    ("更新", IconName::RefreshCw),
                    ("编辑信息", IconName::Pencil),
                    ("打开文件", IconName::FolderOpen),
                    ("执行日志", IconName::Info),
                    ("删除", IconName::Trash),
                ] {
                    if action == "更新" && !remote {
                        continue;
                    }
                    if action == "执行日志" && !script {
                        continue;
                    }
                    let owner = owner.clone();
                    menu = menu.item(ui::actions::menu(action).icon(icon).on_click(
                        move |_, w, cx| {
                            owner.update(cx, |this, cx| this.document_action(id, action, w, cx));
                        },
                    ));
                }
                menu
            });
        let row = ListItem::new(("override-row", id))
            .accessibility_label(format!("覆写 {}，拖动排序", document.name))
            .when(draggable, |row| {
                row.on_drag(
                    super::organize::DocumentDrag {
                        id,
                        name: document.name.clone(),
                        overrides: true,
                    },
                    move |drag, _, _, cx| {
                        drag_owner.update(cx, |this, cx| {
                            this.override_drop = None;
                            cx.notify();
                        });
                        cx.new(|_| drag.clone())
                    },
                )
            })
            .can_drop(move |value, _, _| {
                draggable
                    && value
                        .downcast_ref::<super::organize::DocumentDrag>()
                        .is_some_and(|drag| drag.overrides && drag.id != id)
            })
            .on_drag_move(cx.listener(
                move |this, event: &DragMoveEvent<super::organize::DocumentDrag>, _, cx| {
                    if event.drag(cx).overrides
                        && event.drag(cx).id != id
                        && event.bounds.contains(&event.event.position)
                    {
                        let target = Some((id, event.event.position.y >= event.bounds.center().y));
                        if this.override_drop != target {
                            this.override_drop = target;
                            cx.notify();
                        }
                    } else if this.override_drop.is_some_and(|(target, _)| target == id) {
                        this.override_drop = None;
                        cx.notify();
                    }
                },
            ))
            .drag_over::<super::organize::DocumentDrag>(move |style, _, _, cx| {
                style.bg(cx.theme().drop_target)
            })
            .on_drop(cx.listener(
                move |this, drag: &super::organize::DocumentDrag, window, cx| {
                    let Some((target, after)) = this.override_drop.take() else {
                        return;
                    };
                    let mut documents = this.documents.clone();
                    let Some(from) = documents.iter().position(|document| document.id == drag.id)
                    else {
                        return;
                    };
                    let document = documents.remove(from);
                    let Some(target_index) =
                        documents.iter().position(|document| document.id == target)
                    else {
                        return;
                    };
                    let to = target_index + usize::from(after);
                    if from == to {
                        return;
                    }
                    documents.insert(to, document);
                    let draft = documents.clone();
                    match this.persist_documents(documents, None, window, cx, |_, _, _| {}) {
                        Ok(()) => this.documents = draft,
                        Err(error) => ui::show_error(error, window, cx),
                    }
                },
            ))
            .w_full()
            .h_full()
            .rounded(cx.theme().radius)
            .px(space::LG)
            .py(space::MD)
            .child(
                h_flex()
                    .w_full()
                    .min_w(space::NONE)
                    .gap(space::MD)
                    .child(
                        div().cursor_grab().child(
                            component::Icon::new(IconName::GripVertical)
                                .size(layout::ICON)
                                .text_color(cx.theme().muted_foreground),
                        ),
                    )
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
                            .outline()
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
                    .child(menu),
            );
        div()
            .relative()
            .size_full()
            .child(row)
            .when(
                cx.has_active_drag() && self.override_drop.is_some_and(|(target, _)| target == id),
                |row| {
                    let indicator = div()
                        .id(("override-insertion", id))
                        .absolute()
                        .left(space::NONE)
                        .right(space::NONE)
                        .h(stroke::INSERTION)
                        .bg(cx.theme().primary)
                        .when(after, |line| line.bottom(space::NONE))
                        .when(!after, |line| line.top(space::NONE));
                    #[cfg(test)]
                    let indicator = {
                        use gpui_kit::test::TestSupportExt as _;
                        indicator.test_support()
                    };
                    row.child(indicator)
                },
            )
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
                            component::input::Input::new(&self.url)
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
                            ui::theme::list::DETAIL_ROW_HEIGHT.to_pixels(window.rem_size());
                        let surface = div()
                            .id("override-list-surface")
                            .flex_1()
                            .min_h(space::NONE)
                            .rounded(cx.theme().radius_lg)
                            .overflow_hidden()
                            // GPUI overflow masks are rectangular. Inset the
                            // scroll viewport so hovered rows stay inside the corners.
                            .p(space::SM)
                            .bg(ui::theme::preferences_tint(cx))
                            .child(ui::virtual_rows::rows(
                                cx.entity(),
                                "override-rows",
                                vec![height; self.documents.len()],
                                &self.list_scroll,
                                move |this, index, _, cx| {
                                    this.override_row(&this.documents[index], cx)
                                },
                            ));
                        #[cfg(test)]
                        let surface = {
                            use gpui_kit::test::TestSupportExt as _;
                            surface.test_support()
                        };
                        v_flex()
                            .size_full()
                            .gap(space::LG)
                            .child(
                                ui::muted("启用项按列表顺序执行，作用范围在编辑信息中设置。", cx)
                                    .text_size(text::CAPTION),
                            )
                            .child(ui::section("覆写文件", cx).px(space::LG))
                            .child(surface)
                            .into_any_element()
                    }),
            )
            .child(
                ui::workspace_footer().child(
                    ui::muted(
                        format!("{} 个文件 · {enabled} 个已启用", self.documents.len()),
                        cx,
                    )
                    .text_size(text::CAPTION),
                ),
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
            crate::integration::init(service, None, cx);
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

#[cfg(test)]
mod drag_tests {
    use super::{Document, ProfilesPage};
    use gpui_kit::component::WindowExt;
    use gpui_kit::test::{TestEventExt, TestWindowExt};
    use gpui_kit::{
        AppContext, Bounds, MouseButton, TestAppContext, WindowBounds, WindowOptions, point, px,
        size,
    };

    #[gpui_kit::test]
    fn native_row_drag_uses_the_hovered_half_and_skips_unchanged_order(cx: &mut TestAppContext) {
        let root = tempfile::tempdir().unwrap();
        let service = crate::integration::Service::start(
            crate::storage::Repository::open(crate::storage::AppPaths::isolated(root.path()))
                .unwrap(),
        )
        .unwrap();
        cx.update(|cx| {
            crate::integration::init(service, None, cx);
            gpui_kit::init(cx);
            crate::ui::configure_theme(cx);
            cx.set_reduce_motion(true);
        });
        let (handle, page) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        Default::default(),
                        size(px(1000.), px(800.)),
                    ))),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    let session = cx.new(crate::session::Session::new);
                    cx.new(|cx| ProfilesPage::new(true, session, window, cx))
                },
            )
            .unwrap()
        });
        handle
            .update(cx, |_, window, cx| {
                page.update(cx, |page, cx| {
                    page.documents = (1..=3)
                        .map(|id| Document::new(id, super::super::model::DocumentKind::Yaml))
                        .collect();
                    cx.notify();
                });
                window.render_frame(cx);
                let first = window.find(("override-row", 1u64)).bounds();
                let second = window.find(("override-row", 2u64)).bounds();
                let surface = window.find("override-list-surface").bounds();
                let inset = crate::ui::theme::space::SM.to_pixels(window.rem_size());
                assert_eq!(first.left(), surface.left() + inset);
                assert_eq!(first.top(), surface.top() + inset);
                assert_eq!(first.right(), surface.right() - inset);
                window.hover(("override-row", 1u64), cx);
                assert_eq!(window.find(("override-row", 1u64)).bounds(), first);
                let start = point(first.left() + px(12.), first.center().y);
                let top = point(
                    second.left() + px(12.),
                    second.top() + second.size.height / 4.,
                );
                let bottom = point(
                    second.left() + px(12.),
                    second.bottom() - second.size.height / 4.,
                );
                window.clear_notifications(cx);
                window.pointer_down(start, MouseButton::Left, cx);
                window.pointer_move(top, Some(MouseButton::Left), cx);
                window.pointer_move(top, Some(MouseButton::Left), cx);
                let line = window.find(("override-insertion", 2u64)).bounds();
                assert_eq!(line.top(), second.top());
                assert_eq!(line.size.height, crate::ui::theme::stroke::INSERTION);
                window.pointer_move(bottom, Some(MouseButton::Left), cx);
                let line = window.find(("override-insertion", 2u64)).bounds();
                assert_eq!(line.bottom(), second.bottom());
                window.pointer_move(top, Some(MouseButton::Left), cx);
                window.pointer_up(top, MouseButton::Left, cx);
                assert!(window.try_find(("override-insertion", 2u64)).is_none());
                assert!(page.read(cx).override_drop.is_none());
                assert!(crate::ui::notifications::current(window, cx).is_empty());
                window.drag(start, bottom, cx);
                assert!(page.read(cx).override_drop.is_none());
                // The UI test adapter has no lifecycle worker. A changed order reaches
                // submission and reports that real error; the unchanged order did not.
                assert_eq!(crate::ui::notifications::current(window, cx).len(), 1);
                assert!(!cx.has_active_drag());
            })
            .unwrap();
        crate::integration::finish_test_services(cx);
    }
}
