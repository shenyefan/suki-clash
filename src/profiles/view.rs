use super::{
    ProfilesPage,
    model::{Document, DocumentKind, TrafficUsage},
};
use crate::{
    ui,
    ui::theme::{control, layout, opacity, profile, space, text},
};
use gpui_kit::{
    assets::IconName,
    component::{
        ActiveTheme, Disableable, Icon, Sizable,
        button::{Button, ButtonVariants, DropdownButton},
        checkbox::Checkbox,
        h_flex,
        input::Input,
        menu::{DropdownMenu, PopupMenuItem},
        scroll::ScrollableElement,
        switch::Switch,
        v_flex,
    },
    prelude::FluentBuilder as _,
    *,
};

impl ProfilesPage {
    fn document_details(document: &Document, window: &mut Window, cx: &mut App) {
        ui::detail_dialog(
            document.name.clone(),
            format!(
                "来源\t{}\n格式\t{}\n更新方式\t{}\n更新间隔\t{} 分钟\n下载超时\t{} 秒\n配置大小\t{} B\n文件行数\t{}\n订阅地址\t{}",
                if document.is_remote() {
                    "远程"
                } else {
                    "本地"
                },
                document.kind.label(),
                if document.auto_update {
                    "自动"
                } else {
                    "手动"
                },
                document.interval,
                document.timeout,
                document.content.len(),
                document.content.lines().count(),
                if document.url.is_empty() {
                    "—"
                } else {
                    &document.url
                }
            ),
            window,
            cx,
        );
    }

    fn card(&self, document: &Document, cx: &mut Context<Self>) -> AnyElement {
        let id = document.id;
        let active = !self.overrides && self.session.read(cx).active_profile_id() == Some(id);
        let owner = cx.entity();
        let details = document.clone();
        let overrides = self.overrides;
        let menu = ui::icon_button(("document-menu", id), IconName::Ellipsis, "订阅操作")
            .dropdown_menu(move |menu, _, _| {
                let details = details.clone();
                let js = details.kind == DocumentKind::JavaScript;
                let remote = details.is_remote();
                let mut menu = menu.item(
                    PopupMenuItem::new("查看详情…")
                        .icon(IconName::Info)
                        .on_click(move |_, w, cx| Self::document_details(&details, w, cx)),
                );
                for action in ["编辑信息…", "编辑文件…", "打开文件"] {
                    let owner = owner.clone();
                    menu = menu.item(PopupMenuItem::new(action).on_click(move |_, w, cx| {
                        owner.update(cx, |this, cx| this.document_action(id, action, w, cx));
                    }));
                }
                if !overrides && remote {
                    for action in ["分享二维码…", "复制链接"] {
                        let owner = owner.clone();
                        menu = menu.item(PopupMenuItem::new(action).on_click(move |_, w, cx| {
                            owner.update(cx, |this, cx| this.document_action(id, action, w, cx))
                        }));
                    }
                }
                if !overrides {
                    for action in ["编辑规则…", "编辑代理…", "编辑代理组…"] {
                        let owner = owner.clone();
                        menu = menu.item(PopupMenuItem::new(action).on_click(move |_, w, cx| {
                            owner.update(cx, |this, cx| this.document_action(id, action, w, cx))
                        }));
                    }
                }
                if js {
                    let owner = owner.clone();
                    menu = menu.item(PopupMenuItem::new("执行日志…").on_click(move |_, w, cx| {
                        owner.update(cx, |this, cx| this.document_action(id, "执行日志…", w, cx));
                    }));
                }
                let owner = owner.clone();
                menu.separator()
                    .item(PopupMenuItem::new("删除").icon(IconName::Trash).on_click(
                        move |_, w, cx| {
                            owner.update(cx, |this, cx| this.document_action(id, "删除", w, cx))
                        },
                    ))
            });
        let source = if document.is_remote() {
            document
                .url
                .split("://")
                .nth(1)
                .unwrap_or(&document.url)
                .split('/')
                .next()
                .unwrap_or_default()
                .to_owned()
        } else {
            "本地文件".into()
        };
        let schedule = if document.auto_update {
            match document.interval.parse::<u32>() {
                Ok(minutes) if minutes > 0 && minutes % 1440 == 0 => {
                    format!("每 {} 天更新", minutes / 1440)
                }
                Ok(minutes) if minutes > 0 && minutes % 60 == 0 => {
                    format!("每 {} 小时更新", minutes / 60)
                }
                _ => format!("每 {} 分钟更新", document.interval),
            }
        } else if document.is_remote() {
            "手动更新".into()
        } else {
            document.kind.label().into()
        };
        let focus = self.card_focus[&id].clone();
        let keyboard_focus = focus.clone();
        v_flex()
            .id(("profile-card", id))
            .track_focus(&focus)
            .tab_stop(true)
            .aria_label(format!(
                "{}{}",
                document.name,
                if active { "，当前配置" } else { "" }
            ))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, w, cx| {
                if keyboard_focus.is_focused(w)
                    && matches!(event.keystroke.key.as_str(), "enter" | "space")
                {
                    cx.stop_propagation();
                    if this.batch {
                        if !this.selected.remove(&id) {
                            this.selected.insert(id);
                        }
                        cx.notify();
                    } else if !this.overrides {
                        this.select_profile(id, w, cx);
                    }
                }
            }))
            .when(!self.overrides, |card| {
                card.cursor_pointer()
                    .on_click(cx.listener(move |this, _, w, cx| {
                        if this.batch {
                            if !this.selected.remove(&id) {
                                this.selected.insert(id);
                            }
                            cx.notify();
                        } else if this.session.read(cx).active_profile_id() != Some(id) {
                            this.select_profile(id, w, cx);
                        }
                    }))
            })
            .on_drag(
                super::organize::DocumentDrag {
                    id,
                    name: document.name.clone(),
                },
                |drag, _, _, cx| cx.new(|_| drag.clone()),
            )
            .drag_over::<super::organize::DocumentDrag>(|style, _, _, cx| {
                style.bg(cx.theme().accent)
            })
            .on_drop(
                cx.listener(move |this, drag: &super::organize::DocumentDrag, w, cx| {
                    this.move_document(drag.id, id, w, cx)
                }),
            )
            .min_w(space::NONE)
            .min_h(profile::MIN_HEIGHT)
            .px(profile::PADDING_X)
            .py(profile::PADDING_Y)
            .gap(space::NONE)
            .rounded(cx.theme().radius)
            .bg(cx.theme().muted.opacity(opacity::PANEL))
            .when(active, |card| card.bg(ui::theme::selection_tint(cx)))
            .focus_visible(|card| card.border_1().border_color(cx.theme().ring))
            .child(
                h_flex()
                    .h(profile::HEADER_HEIGHT)
                    .gap(space::SM)
                    .when(self.batch, |row| {
                        row.child(
                            Checkbox::new(("select-document", id))
                                .checked(self.selected.contains(&id))
                                .accessibility_label(format!("选择{}", document.name))
                                .on_change(cx.listener(move |this, selected, _, cx| {
                                    if *selected {
                                        this.selected.insert(id);
                                    } else {
                                        this.selected.remove(&id);
                                    }
                                    cx.stop_propagation();
                                    cx.notify();
                                })),
                        )
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w(space::NONE)
                            .truncate()
                            .text_left()
                            .text_size(profile::NAME_SIZE)
                            .font_weight(text::HEADING)
                            .text_color(if active {
                                cx.theme().primary
                            } else {
                                cx.theme().foreground
                            })
                            .child(document.name.clone()),
                    )
                    .when(document.is_remote(), |row| {
                        row.child(
                            ui::icon_button(
                                ("update-document", id),
                                IconName::RefreshCw,
                                "更新订阅",
                            )
                            .on_click(cx.listener(
                                move |this, _, w, cx| {
                                    cx.stop_propagation();
                                    this.document_action(id, "更新", w, cx);
                                },
                            )),
                        )
                    })
                    .child(
                        div()
                            .id(("profile-menu-boundary", id))
                            .on_click(|_, _, cx| cx.stop_propagation())
                            .child(menu),
                    ),
            )
            .child(
                h_flex().gap(space::SM).h(profile::SOURCE_HEIGHT).child(
                    ui::muted(format!("{source} · {schedule}"), cx)
                        .flex_1()
                        .min_w(space::NONE)
                        .truncate()
                        .text_size(profile::META_SIZE),
                ),
            )
            .when_some(document.usage.as_ref(), |card, usage| {
                card.child(
                    h_flex()
                        .gap(space::SM)
                        .justify_between()
                        .h(profile::ROW_HEIGHT)
                        .child(div().text_size(profile::META_SIZE).child(format!(
                            "{} / {}",
                            TrafficUsage::amount(usage.used_bytes),
                            TrafficUsage::amount(usage.total_bytes)
                        )))
                        .when_some(usage.expires.as_ref(), |row, expires| {
                            row.child(ui::muted(expires.clone(), cx).text_size(profile::META_SIZE))
                        }),
                )
                .child(
                    div()
                        .h(layout::PROGRESS_HEIGHT)
                        .w_full()
                        .rounded_full()
                        .bg(cx.theme().foreground.opacity(opacity::SELECTED))
                        .child(
                            div()
                                .h_full()
                                .w(relative(usage.fraction()))
                                .rounded_full()
                                .bg(cx.theme().primary),
                        ),
                )
            })
            .when(document.usage.is_none(), |card| {
                card.child(
                    h_flex()
                        .h(profile::ROW_HEIGHT)
                        .gap(space::SM)
                        .justify_between()
                        .child(
                            ui::muted(
                                format!(
                                    "{} · {} 行",
                                    document.kind.label(),
                                    document.content.lines().count()
                                ),
                                cx,
                            )
                            .text_size(profile::META_SIZE),
                        )
                        .when(self.overrides, |row| {
                            row.child(
                                Switch::new(("enable-override", id))
                                    .small()
                                    .checked(document.enabled)
                                    .accessibility_label(format!("启用{}", document.name))
                                    .on_change(cx.listener(move |this, value, w, cx| {
                                        let mut documents = this.documents.clone();
                                        if let Some(doc) =
                                            documents.iter_mut().find(|doc| doc.id == id)
                                        {
                                            doc.enabled = *value;
                                        }
                                        if let Err(error) = this.persist_documents(
                                            documents,
                                            None,
                                            w,
                                            cx,
                                            |_, _, _| {},
                                        ) {
                                            ui::show_error(error, w, cx);
                                        }
                                        cx.notify();
                                    })),
                            )
                        }),
                )
            })
            .into_any_element()
    }

    pub(super) fn import_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let owner = cx.entity();
        let overrides = self.overrides;
        DropdownButton::new("import-subscription")
            .outline()
            .with_size(control::STANDARD)
            .menu_accessibility_label("更多导入方式")
            .button(
                Button::new("import-url")
                    .icon(IconName::Download)
                    .label("导入")
                    .on_click(cx.listener(|this, _, w, cx| this.import_remote(w, cx))),
            )
            .dropdown_menu(move |menu, _, _| {
                let file = owner.clone();
                let yaml = owner.clone();
                let js = owner.clone();
                menu.item(
                    PopupMenuItem::new("从文件导入…")
                        .icon(IconName::FolderOpen)
                        .on_click(move |_, w, cx| {
                            file.update(cx, |this, cx| this.import_file(w, cx))
                        }),
                )
                .separator()
                .item(
                    PopupMenuItem::new("新建 YAML 配置…")
                        .icon(IconName::FileCode)
                        .on_click(move |_, w, cx| {
                            yaml.update(cx, |this, cx| {
                                this.create(
                                    if overrides {
                                        DocumentKind::Yaml
                                    } else {
                                        DocumentKind::Profile
                                    },
                                    w,
                                    cx,
                                )
                            })
                        }),
                )
                .when(overrides, |menu| {
                    menu.item(
                        PopupMenuItem::new("新建 JavaScript…").on_click(move |_, w, cx| {
                            js.update(cx, |this, cx| this.create(DocumentKind::JavaScript, w, cx))
                        }),
                    )
                })
            })
    }
}

impl Render for ProfilesPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.overrides {
            return self.overrides_view(cx);
        }
        let documents = self.documents.clone();
        let count = documents.len();
        let current = self
            .session
            .read(cx)
            .profile_name()
            .unwrap_or("—")
            .to_owned();
        let owner = cx.weak_entity();
        let current_columns = self.columns;
        ui::workspace()
            .child(
                ui::workspace_header(if self.overrides { "覆写" } else { "订阅" }, "", cx)
                    .child(
                        Button::new("profile-batch")
                            .ghost()
                            .with_size(control::STANDARD)
                            .label(if self.batch { "完成" } else { "批量管理" })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.batch = !this.batch;
                                this.selected.clear();
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("update-or-apply")
                            .ghost()
                            .with_size(control::STANDARD)
                            .icon(if self.overrides {
                                IconName::CheckCheck
                            } else {
                                IconName::RefreshCw
                            })
                            .label(if self.overrides {
                                "应用覆写"
                            } else {
                                "全部更新"
                            })
                            .on_click(cx.listener(|this, _, w, cx| {
                                ui::request_command(
                                    if this.overrides {
                                        "应用覆写"
                                    } else {
                                        "更新全部订阅"
                                    },
                                    w,
                                    cx,
                                )
                            })),
                    ),
            )
            .child(
                ui::workspace_toolbar()
                    .child(
                        Input::new(&self.url)
                            .with_size(control::STANDARD)
                            .flex_1()
                            .min_w(space::NONE)
                            .prefix(Icon::new(IconName::Link).size(layout::ICON))
                            .aria_label("下载 URL")
                            .suffix(
                                ui::icon_button(
                                    "download-options",
                                    IconName::SlidersHorizontal,
                                    "下载选项",
                                )
                                .on_click(cx.listener(|this, _, w, cx| this.import_dialog(w, cx))),
                            ),
                    )
                    .child(self.import_button(cx)),
            )
            .when(self.batch, |page| {
                page.child(
                    ui::workspace_toolbar()
                        .child(
                            Button::new("select-all-profiles")
                                .ghost()
                                .with_size(control::STANDARD)
                                .label("全选")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.selected =
                                        this.documents.iter().map(|doc| doc.id).collect();
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("clear-profile-selection")
                                .ghost()
                                .with_size(control::STANDARD)
                                .label("取消选择")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.selected.clear();
                                    cx.notify();
                                })),
                        )
                        .child(
                            ui::muted(format!("已选择 {} 项", self.selected.len()), cx)
                                .text_size(text::CAPTION),
                        )
                        .child(div().flex_1())
                        .child(
                            Button::new("update-selected-profiles")
                                .outline()
                                .with_size(control::STANDARD)
                                .label("更新所选")
                                .disabled(self.selected.is_empty())
                                .on_click(cx.listener(|this, _, w, cx| {
                                    let ids: Vec<_> = this.selected.iter().copied().collect();
                                    for id in ids {
                                        if this
                                            .documents
                                            .iter()
                                            .any(|doc| doc.id == id && doc.is_remote())
                                        {
                                            this.document_action(id, "更新", w, cx);
                                        }
                                    }
                                })),
                        )
                        .child(
                            Button::new("delete-selected-profiles")
                                .outline()
                                .with_size(control::STANDARD)
                                .label("移除所选")
                                .disabled(self.selected.is_empty())
                                .on_click(cx.listener(|this, _, w, cx| this.batch_delete(w, cx))),
                        ),
                )
            })
            .child(
                ui::workspace_body()
                    .on_children_prepainted(move |bounds, window, cx| {
                        let Some(bounds) = bounds.first() else {
                            return;
                        };
                        let gap = profile::GAP.to_pixels(window.rem_size());
                        let min = profile::MIN_WIDTH.to_pixels(window.rem_size());
                        let columns =
                            ((bounds.size.width + gap) / (min + gap)).floor().max(1.) as u16;
                        if columns == current_columns {
                            return;
                        }
                        // Defer the geometry-derived update; never mutate an entity during paint.
                        let owner = owner.clone();
                        window.defer(cx, move |_, cx| {
                            let _ = owner.update(cx, |this, cx| {
                                if this.columns != columns {
                                    this.columns = columns;
                                    cx.notify();
                                }
                            });
                        });
                    })
                    .id("document-list")
                    .on_drop(cx.listener(|this, paths: &ExternalPaths, w, cx| {
                        this.import_paths(paths.paths().to_vec(), w, cx)
                    }))
                    .overflow_y_scrollbar()
                    .child(if documents.is_empty() {
                        ui::empty(IconName::Layers, "暂无订阅", "从链接或文件添加配置", cx)
                            .into_any_element()
                    } else {
                        div()
                            .grid()
                            .grid_cols(self.columns)
                            .gap(profile::GAP)
                            .children(documents.iter().map(|doc| self.card(doc, cx)))
                            .into_any_element()
                    }),
            )
            .child(
                ui::workspace_footer()
                    .child(ui::muted(format!("{count} 项配置"), cx).text_size(text::CAPTION))
                    .child(div().flex_1())
                    .when(!self.overrides, |row| {
                        row.child(
                            ui::muted(format!("当前配置 · {current}"), cx).text_size(text::CAPTION),
                        )
                    }),
            )
    }
}
