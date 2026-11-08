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
        menu::DropdownMenu,
        switch::Switch,
        v_flex,
    },
    prelude::FluentBuilder as _,
    *,
};

impl ProfilesPage {
    fn card(&self, document: &Document, cx: &mut Context<Self>) -> AnyElement {
        let id = document.id;
        let active = !self.overrides && self.session.read(cx).active_profile_id() == Some(id);
        let owner = cx.entity();
        let js = document.kind == DocumentKind::JavaScript;
        let remote = document.is_remote();
        let overrides = self.overrides;
        let home = document.home.clone();
        let menu = ui::icon_button(("document-menu", id), IconName::Ellipsis, "订阅操作")
            .dropdown_menu(move |menu, _, _| {
                let mut menu = menu;
                for (action, icon) in [
                    ("编辑信息", IconName::Pencil),
                    ("编辑文件", IconName::FileCode),
                    ("打开文件", IconName::FolderOpen),
                ] {
                    let owner = owner.clone();
                    menu = menu.item(crate::ui::actions::menu(action).icon(icon).on_click(
                        move |_, w, cx| {
                            owner.update(cx, |this, cx| this.document_action(id, action, w, cx));
                        },
                    ));
                }
                if !overrides && remote {
                    for (action, icon) in [
                        ("分享二维码", IconName::QrCode),
                        ("复制链接", IconName::Copy),
                    ] {
                        let owner = owner.clone();
                        menu = menu.item(crate::ui::actions::menu(action).icon(icon).on_click(
                            move |_, w, cx| {
                                owner.update(cx, |this, cx| this.document_action(id, action, w, cx))
                            },
                        ));
                    }
                }
                if !home.is_empty() {
                    let home = home.clone();
                    menu = menu.item(
                        crate::ui::actions::menu("订阅主页")
                            .icon(IconName::Link)
                            .on_click(move |_, _, cx| cx.open_url(&home)),
                    );
                }
                if !overrides {
                    let owner = owner.clone();
                    menu = menu.item(
                        crate::ui::actions::menu("订阅增强")
                            .icon(IconName::SlidersHorizontal)
                            .on_click(move |_, w, cx| {
                                owner.update(cx, |this, cx| {
                                    this.document_action(id, "订阅增强", w, cx)
                                });
                            }),
                    );
                }
                if js {
                    let owner = owner.clone();
                    menu = menu.item(
                        crate::ui::actions::menu("执行日志")
                            .icon(IconName::Info)
                            .on_click(move |_, w, cx| {
                                owner.update(cx, |this, cx| {
                                    this.document_action(id, "执行日志", w, cx)
                                });
                            }),
                    );
                }
                let owner = owner.clone();
                menu.separator().item(
                    crate::ui::actions::menu("删除")
                        .icon(IconName::Trash)
                        .on_click(move |_, w, cx| {
                            owner.update(cx, |this, cx| this.document_action(id, "删除", w, cx))
                        }),
                )
            });
        let source = if document.is_remote() {
            url::Url::parse(&document.url)
                .ok()
                .map(|url| url[url::Position::BeforeHost..url::Position::AfterPort].to_owned())
                .unwrap_or_default()
        } else {
            "本地文件".into()
        };
        let schedule = if document.automatic_updates() {
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
        div()
            .id(("profile-card-drag", id))
            .on_drag(
                super::organize::DocumentDrag {
                    id,
                    name: document.name.clone(),
                    overrides: false,
                },
                |drag, _, _, cx| cx.new(|_| drag.clone()),
            )
            .drag_over::<super::organize::DocumentDrag>(|style, _, _, cx| {
                style.bg(cx.theme().accent)
            })
            .on_drop(
                cx.listener(move |this, drag: &super::organize::DocumentDrag, w, cx| {
                    if !drag.overrides {
                        this.move_document(drag.id, id, w, cx)
                    }
                }),
            )
            .min_w(space::NONE)
            .child(
                Button::new(("profile-card", id))
                    .ghost()
                    .with_size(control::STANDARD)
                    .h_auto()
                    .w_full()
                    .track_focus(&focus)
                    .disabled(self.saving)
                    .accessibility_label(format!(
                        "{}{}",
                        document.name,
                        if active { "，当前配置" } else { "" }
                    ))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if this.batch {
                            if !this.selected.remove(&id) {
                                this.selected.insert(id);
                            }
                            cx.notify();
                        } else if !this.overrides
                            && this.session.read(cx).active_profile_id() != Some(id)
                        {
                            this.select_profile(id, window, cx);
                        }
                    }))
                    .min_w(space::NONE)
                    .min_h(profile::MIN_HEIGHT)
                    .px(profile::PADDING_X)
                    .py(profile::PADDING_Y)
                    .gap(space::NONE)
                    .rounded(cx.theme().radius)
                    .map(|card| {
                        ui::selectable_card(
                            card,
                            active || (self.batch && self.selected.contains(&id)),
                            cx,
                        )
                    })
                    .child(
                        v_flex()
                            .w_full()
                            .min_w(space::NONE)
                            .gap(space::NONE)
                            .child(
                                h_flex()
                                    .h(profile::HEADER_HEIGHT)
                                    .gap(space::SM)
                                    .when(self.batch, |row| {
                                        row.child(
                                            Checkbox::new(("select-document", id))
                                                .checked(self.selected.contains(&id))
                                                .accessibility_label(format!(
                                                    "选择{}",
                                                    document.name
                                                ))
                                                .on_change(cx.listener(
                                                    move |this, selected, _, cx| {
                                                        if *selected {
                                                            this.selected.insert(id);
                                                        } else {
                                                            this.selected.remove(&id);
                                                        }
                                                        cx.stop_propagation();
                                                        cx.notify();
                                                    },
                                                )),
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
                                            .on_click(
                                                cx.listener(move |this, _, w, cx| {
                                                    cx.stop_propagation();
                                                    this.document_action(id, "更新", w, cx);
                                                }),
                                            ),
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
                                            row.child(
                                                ui::muted(expires.clone(), cx)
                                                    .text_size(profile::META_SIZE),
                                            )
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
                                                    .accessibility_label(format!(
                                                        "启用{}",
                                                        document.name
                                                    ))
                                                    .on_change(cx.listener(
                                                        move |this, value, w, cx| {
                                                            let mut documents =
                                                                this.documents.clone();
                                                            if let Some(doc) = documents
                                                                .iter_mut()
                                                                .find(|doc| doc.id == id)
                                                            {
                                                                doc.enabled = *value;
                                                            }
                                                            if let Err(error) = this
                                                                .persist_documents(
                                                                    documents,
                                                                    None,
                                                                    w,
                                                                    cx,
                                                                    |_, _, _| {},
                                                                )
                                                            {
                                                                ui::show_error(error, w, cx);
                                                            }
                                                            cx.notify();
                                                        },
                                                    )),
                                            )
                                        }),
                                )
                            }),
                    ),
            )
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
                    crate::ui::actions::menu("从文件导入")
                        .icon(IconName::FolderOpen)
                        .on_click(move |_, w, cx| {
                            file.update(cx, |this, cx| this.import_file(w, cx))
                        }),
                )
                .separator()
                .item(
                    crate::ui::actions::menu("新建 YAML 配置")
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
                        crate::ui::actions::menu("新建 JavaScript")
                            .icon(IconName::FileCode)
                            .on_click(move |_, w, cx| {
                                js.update(cx, |this, cx| {
                                    this.create(DocumentKind::JavaScript, w, cx)
                                })
                            }),
                    )
                })
            })
    }
}

impl Render for ProfilesPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.overrides {
            return self.overrides_view(window, cx);
        }
        let count = self.documents.len();
        let columns = usize::from(self.columns.max(1));
        let height = (profile::MIN_HEIGHT + profile::GAP).to_pixels(window.rem_size());
        let cards = ui::virtual_rows::rows(
            cx.entity(),
            "profile-card-rows",
            vec![height; count.div_ceil(columns)],
            &self.list_scroll,
            move |this, row, _, cx| {
                let start = row * columns;
                let end = (start + columns).min(this.documents.len());
                div()
                    .h(height)
                    .pb(profile::GAP)
                    .grid()
                    .grid_cols(columns as u16)
                    .gap(profile::GAP)
                    .children(
                        this.documents[start..end]
                            .iter()
                            .map(|document| this.card(document, cx)),
                    )
                    .into_any_element()
            },
        );
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
                    .when(!self.batch, |header| {
                        header.child(
                            Button::new("profile-batch")
                                .icon(gpui_kit::assets::IconName::ListChecks)
                                .outline()
                                .with_size(control::STANDARD)
                                .label("批量管理")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.batch = !this.batch;
                                    this.selected.clear();
                                    cx.notify();
                                })),
                        )
                    })
                    .child(
                        Button::new("update-or-apply")
                            .outline()
                            .with_size(control::STANDARD)
                            .icon(IconName::RefreshCw)
                            .label("全部更新")
                            .on_click(cx.listener(|this, _, w, cx| {
                                let ids = this
                                    .documents
                                    .iter()
                                    .filter(|doc| doc.is_remote())
                                    .map(|doc| doc.id)
                                    .collect();
                                this.update_profiles(ids, w, cx);
                            })),
                    ),
            )
            .child(
                ui::workspace_toolbar()
                    .child(
                        Input::new(&self.url)
                            .aria_label("订阅导入地址")
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
                                .icon(gpui_kit::assets::IconName::CheckCheck)
                                .outline()
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
                                .icon(gpui_kit::assets::IconName::X)
                                .outline()
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
                                .icon(gpui_kit::assets::IconName::RefreshCw)
                                .outline()
                                .with_size(control::STANDARD)
                                .label("更新所选")
                                .disabled(self.selected.is_empty())
                                .on_click(cx.listener(|this, _, w, cx| {
                                    let ids: Vec<_> = this.selected.iter().copied().collect();
                                    this.update_profiles(ids, w, cx);
                                })),
                        )
                        .child(
                            Button::new("delete-selected-profiles")
                                .icon(gpui_kit::assets::IconName::Trash)
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
                        let columns = ui::adaptive_columns(bounds.size.width, min, gap) as u16;
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
                    .child(if count == 0 {
                        ui::empty(IconName::Layers, "暂无订阅", "从链接或文件添加配置", cx)
                            .into_any_element()
                    } else {
                        cards
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
                    })
                    .when(self.batch, |row| {
                        row.child(
                            Button::new("finish-profile-batch")
                                .icon(IconName::Check)
                                .with_size(control::STANDARD)
                                .label("完成")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.batch = false;
                                    this.selected.clear();
                                    cx.notify();
                                })),
                        )
                    }),
            )
    }
}

#[cfg(test)]
mod interaction_tests {
    use super::ProfilesPage;
    use crate::{
        profiles::model::{Document, DocumentKind},
        session::Session,
    };
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{AppContext, TestAppContext, WindowOptions};

    #[gpui_kit::test]
    fn import_and_settings_share_the_editor_and_keep_the_address_synchronized(
        cx: &mut TestAppContext,
    ) {
        use gpui_kit::component::WindowExt as _;
        use gpui_kit::{Bounds, ScrollDelta, WindowBounds, point, px, size};
        let root = tempfile::tempdir().unwrap();
        let service = crate::integration::Service::start(
            crate::integration::Repository::open(crate::integration::AppPaths::isolated(
                root.path(),
            ))
            .unwrap(),
        )
        .unwrap();
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::integration::init(service, None, cx);
            cx.set_reduce_motion(true);
        });
        let (handle, page) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        Default::default(),
                        size(px(1100.), px(850.)),
                    ))),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    let session = cx.new(|_| Session::default());
                    cx.new(|cx| ProfilesPage::new(false, session, window, cx))
                },
            )
            .unwrap()
        });
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("profile-batch", cx);
            assert!(page.read(cx).batch);
            let done = window.find("finish-profile-batch").bounds();
            let import = window.within("import-subscription").find("popup").bounds();
            assert!((done.right() - import.right()).abs() <= px(1.));
            assert!(done.bottom() <= window.viewport_size().height);
            assert!(done.top() > window.viewport_size().height * 0.8);
            assert!(done.size.width < window.viewport_size().width / 3.);
            window.click("finish-profile-batch", cx);
            assert!(!page.read(cx).batch);
            window.click("import-url", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let name = window.find("field-名称").bounds();
            let url = window.find("field-订阅地址").bounds();
            let agent = window.find("field-User-Agent").bounds();
            let timeout = window.find("field-超时（秒）").bounds();
            let interval = window.find("field-更新间隔（分钟）").bounds();
            assert_eq!(interval.top(), timeout.top());
            assert_eq!(agent.size.width, url.size.width);
            assert!(window.try_find("editor-auto-update").is_none());
            let token = window.find("field-访问令牌").bounds();
            assert_eq!(name.left(), url.left());
            assert_eq!(name.right(), url.right());
            assert_eq!(agent.left(), url.left());
            assert_eq!(timeout.right(), url.right());
            assert_eq!(token.right(), url.right());
            let age = window.find("field-Age 私钥").bounds();
            let system = window.find("with-proxy").bounds();
            let kernel = window.find("self-proxy").bounds();
            let insecure = window.find("invalid-certs").bounds();
            assert!(system.top() >= age.bottom());
            assert!(kernel.top() >= system.bottom());
            assert!(insecure.top() >= kernel.bottom());
            assert_eq!(system.right(), kernel.right());
            assert_eq!(kernel.right(), insecure.right());
            window.scroll(
                "document-name",
                ScrollDelta::Pixels(point(px(0.), px(-500.))),
                cx,
            );
            window.render_frame(cx);
            assert!(window.find("with-proxy").visible());
            window.click("with-proxy", cx);
            window.render_frame(cx);
            assert_eq!(window.find("with-proxy").checked(), Some(true));
            assert_eq!(window.find("self-proxy").checked(), Some(false));
            window.click("self-proxy", cx);
            window.render_frame(cx);
            assert_eq!(window.find("with-proxy").checked(), Some(false));
            assert_eq!(window.find("self-proxy").checked(), Some(true));
            window.close_dialog(cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            page.update(cx, |page, cx| {
                page.url.update(cx, |input, cx| {
                    input.set_value("https://example.com/sub.yaml", window, cx)
                })
            });
            window.render_frame(cx);
            window.click("download-options", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("document-url").is_some());
            window.click("document-url", cx);
            window.press(
                if cx.platform_info().is_mac() {
                    "cmd-a"
                } else {
                    "ctrl-a"
                },
                cx,
            );
            window.input("https://example.com/changed.yaml", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            assert_eq!(
                page.read(cx).url.read(cx).value().as_ref(),
                "https://example.com/changed.yaml"
            );
            window.close_dialog(cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("import-url", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("document-url").is_none());
        })
        .unwrap();
        cx.update_window(handle, |_, window, cx| {
            page.update(cx, |page, cx| {
                let mut document = Document::new(42, DocumentKind::Profile);
                document.name = "已保存的订阅".into();
                document.url = "https://example.com/saved.yaml".into();
                page.open_editor(document, false, window, cx);
            });
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            for id in [
                "document-name",
                "document-url",
                "document-agent",
                "document-token",
                "document-age",
            ] {
                assert!(window.try_find(id).is_some());
            }
        })
        .unwrap();
        crate::integration::finish_test_services(cx);
    }
    #[gpui_kit::test]
    fn native_card_keyboard_and_inner_checkbox_change_selection_once(cx: &mut TestAppContext) {
        use gpui_kit::test::TestQueryExt as _;
        let root = tempfile::tempdir().unwrap();
        let repository = crate::integration::Repository::open(
            crate::integration::AppPaths::isolated(root.path()),
        )
        .unwrap();
        let service = crate::integration::Service::start(repository).unwrap();
        cx.update(|cx| {
            crate::integration::init(service, None, cx);
            gpui_kit::init(cx);
            cx.set_reduce_motion(true);
        });
        let (handle, page) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                let session = cx.new(|_| Session::default());
                cx.new(|cx| {
                    let mut page = ProfilesPage::new(false, session, window, cx);
                    page.batch = true;
                    let mut document = Document::new(42, DocumentKind::Profile);
                    document.name = "测试配置".into();
                    page.documents = vec![document];
                    page.card_focus.insert(42, cx.focus_handle());
                    page
                })
            })
            .unwrap()
        });
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click(("profile-card", 42u64), cx);
            assert!(page.read(cx).selected.contains(&42));
            window.render_frame(cx);
            assert!(page.read(cx).selected.contains(&42));
            window.hover(("profile-card", 42u64), cx);
            assert!(page.read(cx).selected.contains(&42));
            window.click(("select-document", 42u64), cx);
            assert!(
                !page.read(cx).selected.contains(&42),
                "inner checkbox must not also activate its card"
            );
            let focus = page.read(cx).card_focus[&42].clone();
            focus.focus(window, cx);
            window.press("enter", cx);
            assert!(page.read(cx).selected.contains(&42));
            window.press("space", cx);
            assert!(!page.read(cx).selected.contains(&42));
            window.click(("document-menu", 42u64), cx);
            for removed in ["查看详情", "编辑规则", "编辑代理", "编辑代理组"] {
                assert!(window.try_find_by_label(removed).is_none());
            }
            assert_eq!(window.find_all_by_label("订阅增强").len(), 1);
            let action = window
                .find_by_label("订阅增强")
                .path()
                .last()
                .unwrap()
                .clone();
            window.click(action, cx);
            assert!(window.find("enhance-mode").visible());
            assert!(window.within("enhance-property").find(0usize).visible());
            assert!(window.within("enhance-property").find(1usize).visible());
            assert!(window.within("enhance-property").find(2usize).visible());
        })
        .unwrap();
        crate::integration::finish_test_services(cx);
    }
    #[gpui_kit::test]
    fn large_profile_grid_keeps_selection_when_rows_leave_the_viewport(cx: &mut TestAppContext) {
        use gpui_kit::ScrollStrategy;
        let root = tempfile::tempdir().unwrap();
        let repository = crate::integration::Repository::open(
            crate::integration::AppPaths::isolated(root.path()),
        )
        .unwrap();
        let service = crate::integration::Service::start(repository).unwrap();
        cx.update(|cx| {
            crate::integration::init(service, None, cx);
            gpui_kit::init(cx);
            cx.set_reduce_motion(true);
        });
        let (window, page) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                let session = cx.new(|_| Session::default());
                cx.new(|cx| {
                    let mut page = ProfilesPage::new(false, session, window, cx);
                    page.batch = true;
                    page.documents = (1..=600)
                        .map(|id| {
                            let mut document = Document::new(id, DocumentKind::Profile);
                            document.name = format!("配置{id}");
                            page.card_focus.insert(id, cx.focus_handle());
                            document
                        })
                        .collect();
                    page
                })
            })
            .unwrap()
        });
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find(("profile-card", 401u64)).is_none());
            let columns = usize::from(page.read(cx).columns);
            page.read(cx)
                .list_scroll
                .scroll_to_item(400 / columns, ScrollStrategy::Top);
            page.update(cx, |_, cx| cx.notify());
            window.render_frame(cx);
            window.render_frame(cx);
            assert!(window.try_find(("profile-card", 401u64)).is_some());
            window.click(("profile-card", 401u64), cx);
            assert!(page.read(cx).selected.contains(&401));
            page.read(cx)
                .list_scroll
                .scroll_to_item(0, ScrollStrategy::Top);
            page.update(cx, |_, cx| cx.notify());
            window.render_frame(cx);
            assert!(page.read(cx).selected.contains(&401));
        })
        .unwrap();
        for (width, wide) in [(520., false), (2200., true)] {
            cx.simulate_window_resize(
                window,
                gpui_kit::size(gpui_kit::px(width), gpui_kit::px(800.)),
            );
            cx.run_until_parked();
            cx.update_window(window, |_, window, cx| window.render_frame(cx))
                .unwrap();
            cx.run_until_parked();
            cx.update(|cx| {
                let columns = page.read(cx).columns;
                if wide {
                    assert!(columns > 3);
                } else {
                    assert_eq!(columns, 2);
                }
                assert!(page.read(cx).selected.contains(&401));
            });
        }
        crate::integration::finish_test_services(cx);
    }
}
