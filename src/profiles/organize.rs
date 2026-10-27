use super::{ProfilesPage, model::Document, store};
use crate::{
    ui,
    ui::theme::{control, layout, space, text},
};
use gpui_kit::{
    component::{ActiveTheme, Sizable, WindowExt, button::Button, h_flex, v_flex},
    *,
};
use std::sync::Arc;

#[derive(Clone)]
pub(super) struct DocumentDrag {
    pub(super) id: u64,
    pub(super) name: String,
    pub(super) overrides: bool,
}
impl Render for DocumentDrag {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .p(space::MD)
            .rounded(cx.theme().radius)
            .bg(cx.theme().popover)
            .child(self.name.clone())
    }
}
impl ProfilesPage {
    pub(super) fn move_document(
        &mut self,
        id: u64,
        target: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.saving || self.session.read(cx).core_busy() || id == target {
            return;
        }
        let (Some(from), Some(to)) = (
            self.documents.iter().position(|doc| doc.id == id),
            self.documents.iter().position(|doc| doc.id == target),
        ) else {
            return;
        };
        let mut documents = self.documents.clone();
        let document = documents.remove(from);
        documents.insert(to, document);
        let active = self.desired_profile;
        let draft = documents.clone();
        match self.persist_documents(documents, active, window, cx, |_, _, _| {}) {
            Ok(()) => {
                if self.overrides {
                    self.documents = draft;
                }
            }
            Err(error) => ui::show_error(error, window, cx),
        }
    }
    pub(super) fn import_paths(
        &mut self,
        paths: Vec<std::path::PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.import_task.is_some() {
            return;
        }
        self.import_task = Some(cx.spawn_in(window, async move |owner, cx| {
            let results = cx
                .background_spawn(async move {
                    paths
                        .into_iter()
                        .map(store::read_document)
                        .collect::<Vec<_>>()
                })
                .await;
            let _ = cx.update(|window, cx| {
                let _ = owner.update(cx, |this, cx| {
                    let mut documents = this.documents.clone();
                    for result in results {
                        match result {
                            Ok((name, content, js)) if this.overrides || !js => {
                                let kind = if !this.overrides {
                                    super::model::DocumentKind::Profile
                                } else if js {
                                    super::model::DocumentKind::JavaScript
                                } else {
                                    super::model::DocumentKind::Yaml
                                };
                                let mut document = Document::new(this.next_id, kind);
                                this.next_id += 1;
                                document.name = name;
                                document.content = content;
                                documents.push(document);
                            }
                            Ok(_) => ui::show_error("JavaScript 请在覆写页导入".into(), window, cx),
                            Err(error) => ui::show_error(error, window, cx),
                        }
                    }
                    let active = this.desired_profile;
                    if let Err(error) =
                        this.persist_documents(documents, active, window, cx, |_, _, _| {})
                    {
                        ui::show_error(error, window, cx);
                    }
                    cx.notify();
                });
            });
            let _ = owner.update(cx, |this, cx| {
                if let Some(task) = this.import_task.take() {
                    task.detach();
                }
                cx.notify();
            });
        }));
    }
    pub(super) fn batch_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let removed: Vec<_> = self
            .documents
            .iter()
            .filter(|doc| self.selected.contains(&doc.id))
            .cloned()
            .collect();
        let remaining: Vec<_> = self
            .documents
            .iter()
            .filter(|doc| !self.selected.contains(&doc.id))
            .cloned()
            .collect();
        let active = self
            .desired_profile
            .filter(|id| remaining.iter().any(|doc| doc.id == *id));
        let result =
            self.persist_documents(remaining, active, window, cx, move |this, window, cx| {
                this.selected.clear();
                let owner = cx.entity();
                crate::ui::notifications::push(
                    window,
                    gpui_kit::component::notification::Notification::new()
                        .message(format!("已移除 {} 项配置", removed.len()))
                        .action(move |_, _, _| {
                            let owner = owner.clone();
                            let removed = removed.clone();
                            Button::new("undo-batch-delete")
                                .icon(gpui_kit::assets::IconName::RotateCcw)
                                .with_size(control::STANDARD)
                                .label("撤销")
                                .on_click(move |_, w, cx| {
                                    owner.update(cx, |this, cx| {
                                        this.restore_deleted(
                                            removed.iter().map(|doc| doc.id).collect(),
                                            w,
                                            cx,
                                        );
                                    });
                                })
                        }),
                    cx,
                );
                cx.notify();
            });
        if let Err(error) = result {
            ui::show_error(error, window, cx);
        }
    }
    pub(super) fn share_qr(document: &Document, window: &mut Window, cx: &mut App) {
        let code = match qrcode::QrCode::new(document.url.as_bytes()) {
            Ok(code) => code,
            Err(error) => {
                ui::show_error(error.to_string(), window, cx);
                return;
            }
        };
        // QR black/white encode data rather than theme colors.
        let svg = code
            .render::<qrcode::render::svg::Color>()
            .min_dimensions(512, 512)
            .dark_color(qrcode::render::svg::Color("black"))
            .light_color(qrcode::render::svg::Color("white"))
            .build();
        let image = Arc::new(Image::from_bytes(ImageFormat::Svg, svg.into_bytes()));
        let url = document.url.clone();
        let title = format!("分享订阅 · {}", document.name);
        window.open_dialog(cx, move |dialog, window, cx| {
            let url = url.clone();
            let image = image.clone();
            ui::modal(dialog, title.clone(), layout::DIALOG_WIDTH, window, cx)
                .content(ui::modal_content(
                    "subscription-qr-body",
                    crate::ui::theme::layout::DIALOG_FORM_HEIGHT,
                    move |_, cx| {
                        v_flex()
                            .gap(space::LG)
                            .items_center()
                            .child(img(image.clone()).size(layout::QR_SIZE))
                            .child(
                                ui::muted("二维码包含完整订阅地址，请仅向信任的人展示。", cx)
                                    .text_size(text::CAPTION),
                            )
                            .into_any_element()
                    },
                ))
                .footer(
                    h_flex()
                        .justify_end()
                        .gap(space::SM)
                        .child(
                            Button::new("copy-subscription-url")
                                .icon(gpui_kit::assets::IconName::Copy)
                                .with_size(control::STANDARD)
                                .label("复制链接")
                                .on_click(move |_, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(url.clone()))
                                }),
                        )
                        .child(
                            Button::new("close-subscription-qr")
                                .icon(gpui_kit::assets::IconName::X)
                                .with_size(control::STANDARD)
                                .label("关闭")
                                .on_click(|_, w, cx| w.close_dialog(cx)),
                        ),
                )
        });
    }
}
