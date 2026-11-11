//! Compact tray popovers. GPUI owns list input, virtualization and popup geometry.
use super::menu::{self, Page, Row, Target};
use crate::{
    core, integration,
    ui::theme::{layout, space, stroke, text},
};
use gpui_kit::{
    assets::IconName,
    base::{Align, Placement, Positioner},
    component::{
        ActiveTheme, Icon, IndexPath, h_flex,
        list::{List, ListDelegate, ListEvent, ListItem, ListState},
    },
    prelude::FluentBuilder as _,
    *,
};
use std::{cell::Cell, collections::BTreeMap, rc::Rc};

struct Delegate {
    owner: WeakEntity<Panel>,
    depth: usize,
    rows: Vec<Row>,
    detail_width: Pixels,
}
impl ListDelegate for Delegate {
    type Item = ListItem;
    fn items_count(&self, _: usize, _: &App) -> usize {
        self.rows.len()
    }
    fn render_item(
        &mut self,
        ix: IndexPath,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        let row = self.rows.get(ix.row)?;
        let submenu = matches!(row.target, Target::Page(_));
        let owner = self.owner.clone();
        let measure = owner.clone();
        let depth = self.depth;
        let id = row.id.clone();
        let rem = cx.theme().font_size;
        let mut surface = ListItem::new(SharedString::from(id.clone()))
            .h(layout::TRAY_ROW_HEIGHT.to_pixels(rem).ceil())
            .rounded(
                (cx.theme().radius - space::XS.to_pixels(rem).ceil() - stroke::HAIRLINE)
                    .max(px(0.)),
            )
            .px(space::SM)
            .py_0()
            .relative()
            .text_size(text::BODY)
            .disabled(!row.enabled)
            .accessibility_label(if row.detail.is_empty() {
                row.label.clone()
            } else {
                format!("{}，{}", row.label, row.detail)
            })
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .gap_2()
                    .when_some(row.image.clone(), |line, path| {
                        line.child(
                            img(path)
                                .size(layout::ICON)
                                .object_fit(ObjectFit::Contain)
                                .flex_shrink_0(),
                        )
                    })
                    .when_some(row.icon, |line, icon| {
                        line.child(Icon::new(icon).size(layout::ICON).flex_shrink_0())
                    })
                    .child(div().flex_1().min_w_0().truncate().child(row.label.clone()))
                    .when(!row.detail.is_empty(), |line| {
                        line.child(
                            div()
                                .max_w(self.detail_width)
                                .min_w_0()
                                .truncate()
                                .text_color(cx.theme().muted_foreground)
                                .child(row.detail.clone()),
                        )
                    })
                    .child(div().w(layout::ICON).flex_shrink_0().children(if submenu {
                        Some(Icon::new(IconName::ChevronRight).size(layout::ICON_SM))
                    } else if row.checked {
                        Some(Icon::new(IconName::Check).size(layout::ICON_SM))
                    } else {
                        None
                    })),
            )
            .child(
                canvas(
                    move |bounds, _, cx| {
                        let _ = measure.update(cx, |panel, _| {
                            if let Some(layer) = panel.layers.get_mut(depth) {
                                layer.rows_bounds.insert(id.clone(), bounds);
                            }
                        });
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full()
                .top_0()
                .left_0(),
            );
        if row.enabled {
            surface = surface.on_mouse_enter(move |_, window, cx| {
                let _ = owner.update(cx, |panel, cx| panel.hover(depth, ix, window, cx));
            });
        }
        Some(surface)
    }
    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        div()
            .px(space::SM)
            .text_size(text::BODY)
            .text_color(cx.theme().muted_foreground)
            .child("暂无项目")
    }
    fn set_selected_index(
        &mut self,
        _: Option<IndexPath>,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) {
    }
}
struct Layer {
    page: Page,
    list: Entity<ListState<Delegate>>,
    _subscription: Subscription,
    anchor: Bounds<Pixels>,
    width: Pixels,
    bounds: Rc<Cell<Bounds<Pixels>>>,
    rows_bounds: BTreeMap<String, Bounds<Pixels>>,
    hovered: Option<String>,
}
pub(super) struct Panel {
    main: AnyWindowHandle,
    layers: Vec<Layer>,
    _activation: Subscription,
}
impl Panel {
    pub(super) fn new(
        main: AnyWindowHandle,
        anchor: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let activation = cx.observe_window_activation(window, |_, window, _| {
            if !window.is_window_active() {
                window.remove_window();
            }
        });
        let mut panel = Self {
            main,
            layers: vec![],
            _activation: activation,
        };
        let anchor = Bounds::new(
            anchor.origin - window.window_bounds().get_bounds().origin,
            anchor.size,
        );
        panel.push(Page::Root, anchor, false, window, cx);
        panel
    }
    fn projection(page: &Page, cx: &App) -> Vec<Row> {
        let controller = core::controller(cx);
        let state = controller.read(cx);
        menu::rows(
            page,
            &state.runtime(),
            state.snapshot(),
            state.is_busy(),
            &integration::read("settings", cx),
            cx,
        )
    }
    fn push(
        &mut self,
        page: Page,
        anchor: Bounds<Pixels>,
        keyboard: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rows = Self::projection(&page, cx);
        let font_size = text::BODY.to_pixels(cx.theme().font_size);
        let measure = |value: &str| {
            let value = value.replace(['\r', '\n'], " ");
            let run = window.text_style().to_run(value.len());
            window
                .text_system()
                .shape_line(value.into(), font_size, &[run], None)
                .width
        };
        let rem = cx.theme().font_size;
        let max_width = layout::TRAY_MAX_WIDTH
            .to_pixels(rem)
            .min(window.bounds().size.width - space::LG.to_pixels(rem));
        let detail_width = rows
            .iter()
            .map(|r| measure(&r.detail))
            .max()
            .unwrap_or_default()
            .min(max_width * 0.45);
        let label_width = rows
            .iter()
            .map(|r| measure(&r.label))
            .max()
            .unwrap_or_default();
        let width =
            (label_width + detail_width + space::REGION.to_pixels(rem) + space::LG.to_pixels(rem))
                .max(layout::TRAY_MIN_WIDTH.to_pixels(rem))
                .min(max_width);
        let depth = self.layers.len();
        let delegate = Delegate {
            owner: cx.weak_entity(),
            depth,
            rows,
            detail_width,
        };
        let list = cx.new(|cx| ListState::new(delegate, window, cx).searchable(false));
        let subscription =
            cx.subscribe_in(
                &list,
                window,
                move |this, _, event, window, cx| match event {
                    ListEvent::Confirm(ix) => this.activate(depth, *ix, true, window, cx),
                    ListEvent::Cancel => window.remove_window(),
                    ListEvent::Select(_) => {
                        this.layers.truncate(depth + 1);
                        cx.notify();
                    }
                },
            );
        self.layers.push(Layer {
            page,
            list: list.clone(),
            _subscription: subscription,
            anchor,
            width,
            bounds: Default::default(),
            rows_bounds: Default::default(),
            hovered: None,
        });
        if keyboard || depth == 0 {
            list.update(cx, |list, cx| list.focus(window, cx));
        }
        cx.notify();
    }
    fn hover(&mut self, depth: usize, ix: IndexPath, window: &mut Window, cx: &mut Context<Self>) {
        let Some(layer) = self.layers.get(depth) else {
            return;
        };
        let Some(row) = layer.list.read(cx).delegate().rows.get(ix.row) else {
            return;
        };
        if layer.hovered.as_deref() == Some(row.id.as_str()) {
            return;
        }
        self.layers[depth].hovered = Some(row.id.clone());
        let list = self.layers[depth].list.clone();
        list.update(cx, |list, cx| {
            list.set_selected_index(Some(ix), window, cx);
            cx.notify();
        });
        self.layers.truncate(depth + 1);
        self.activate(depth, ix, false, window, cx);
    }
    fn activate(
        &mut self,
        depth: usize,
        ix: IndexPath,
        keyboard: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(layer) = self.layers.get(depth) else {
            return;
        };
        let Some(row) = layer
            .list
            .read(cx)
            .delegate()
            .rows
            .get(ix.row)
            .filter(|r| r.enabled)
            .cloned()
        else {
            return;
        };
        match row.target {
            Target::Page(page) => {
                if self
                    .layers
                    .get(depth + 1)
                    .is_some_and(|child| child.page == page)
                {
                    if keyboard {
                        self.layers[depth + 1]
                            .list
                            .update(cx, |list, cx| list.focus(window, cx));
                    }
                    return;
                }
                if let Some(mut anchor) = layer.rows_bounds.get(&row.id).copied() {
                    let parent = layer.bounds.get();
                    anchor.origin.x = parent.origin.x;
                    anchor.size.width = parent.size.width;
                    self.layers.truncate(depth + 1);
                    self.push(page, anchor, keyboard, window, cx);
                }
            }
            Target::Command(command) if keyboard => {
                let main = self.main;
                window.remove_window();
                cx.defer(move |cx| super::dispatch(&command, main, cx));
            }
            _ => {
                cx.notify();
            }
        }
    }
    fn right(&mut self, _: &OpenSubmenu, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self
            .layers
            .iter()
            .enumerate()
            .find(|(_, layer)| layer.list.focus_handle(cx).contains_focused(window, cx))
            .and_then(|(depth, layer)| layer.list.read(cx).selected_index().map(|ix| (depth, ix)));
        if let Some((depth, ix)) = selected
            && self.layers[depth]
                .list
                .read(cx)
                .delegate()
                .rows
                .get(ix.row)
                .is_some_and(|row| matches!(row.target, Target::Page(_)))
        {
            self.activate(depth, ix, true, window, cx);
        }
    }
    fn left(&mut self, _: &CloseSubmenu, window: &mut Window, cx: &mut Context<Self>) {
        let depth = self
            .layers
            .iter()
            .position(|layer| layer.list.focus_handle(cx).contains_focused(window, cx));
        if let Some(depth) = depth.filter(|depth| *depth > 0) {
            self.layers.truncate(depth);
            self.layers[depth - 1].hovered = None;
            self.layers[depth - 1]
                .list
                .update(cx, |list, cx| list.focus(window, cx));
            cx.notify();
        }
    }
    fn outside(&mut self, event: &MouseDownEvent, window: &mut Window, _: &mut Context<Self>) {
        if !self
            .layers
            .iter()
            .any(|layer| layer.bounds.get().contains(&event.position))
        {
            window.remove_window();
        }
    }
    pub(super) fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for layer in &self.layers {
            let rows = Self::projection(&layer.page, cx);
            layer.list.update(cx, |list, cx| {
                let id = list
                    .selected_index()
                    .and_then(|ix| list.delegate().rows.get(ix.row))
                    .map(|r| r.id.clone());
                let selected = id.and_then(|id| rows.iter().position(|r| r.id == id));
                list.delegate_mut().rows = rows;
                list.set_selected_index(selected.map(IndexPath::new), window, cx);
                cx.notify();
            });
        }
        // Existing surfaces retain their width during live refresh.
        cx.notify();
    }
}
actions!(tray_popup, [OpenSubmenu, CloseSubmenu]);
pub(super) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("right", OpenSubmenu, Some("TrayPopup")),
        KeyBinding::new("left", CloseSubmenu, Some("TrayPopup")),
    ]);
}
impl Render for Panel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rem = cx.theme().font_size;
        let gap = space::XS.to_pixels(rem);
        let mut root = div()
            .size_full()
            .key_context("TrayPopup")
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::left))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::outside))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::outside));
        for (depth, layer) in self.layers.iter().enumerate() {
            let measure = layer.bounds.clone();
            let padding = gap.ceil();
            let inset = padding + stroke::HAIRLINE;
            let surface = div()
                .id(("tray-menu", depth))
                .w(layer.width)
                .p(padding)
                .bg(crate::ui::theme::tray_tint(cx))
                .text_color(cx.theme().popover_foreground)
                .border(stroke::HAIRLINE)
                .border_color(cx.theme().border)
                .rounded(cx.theme().radius)
                .shadow_sm()
                .occlude()
                .child(
                    List::new(&layer.list)
                        .w_full()
                        .h_auto()
                        .max_h(window.viewport_size().height - (gap + inset) * 2.)
                        .scroll_bounce(false),
                );
            let positioner = if depth == 0 {
                Positioner::side(layer.anchor)
                    .placement(Placement::Bottom)
                    .align(Align::End)
                    .offset(gap)
            } else {
                Positioner::side(layer.anchor)
                    .placement(Placement::Right)
                    .align(Align::Start)
                    .offset(gap)
            };
            root = root.child(
                deferred(
                    positioner
                        .margin(gap)
                        .occlude()
                        .on_position(move |position| {
                            measure.set(position.bounds);
                        })
                        .child(surface),
                )
                .with_priority(base::POPUP_PRIORITY + depth),
            );
        }
        root
    }
}

#[cfg(test)]
mod tests {
    use super::{Page, Panel};
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{
        AppContext, Bounds, TestAppContext, WindowBounds, WindowOptions, point, px, size,
    };
    #[gpui_kit::test]
    fn compact_popover_opens_a_side_layer_and_keeps_parent_size(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            super::init(cx);
            crate::integration::init(
                crate::integration::Service::unavailable(None, "isolated tray test".into()),
                None,
                cx,
            );
        });
        let (handle, panel) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        Default::default(),
                        size(px(1000.), px(700.)),
                    ))),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    let main = window.window_handle();
                    cx.new(|cx| {
                        Panel::new(
                            main,
                            Bounds::new(point(px(900.), px(0.)), size(px(24.), px(24.))),
                            window,
                            cx,
                        )
                    })
                },
            )
            .unwrap()
        });
        cx.run_until_parked();
        let original = cx
            .update_window(handle, |_, window, cx| {
                window.render_frame(cx);
                assert!(window.try_find("tray-back").is_none());
                assert!(panel.read(cx).layers[0].bounds.get().size.height < px(400.));
                let original = panel.read(cx).layers[0].bounds.get().size;
                window.click("more", cx);
                original
            })
            .unwrap();
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(panel.read(cx).layers.len(), 2);
            let layers = &panel.read(cx).layers;
            assert_eq!(layers[0].bounds.get().size, original);
            assert_eq!(layers[1].page, Page::More);
            assert!(
                layers[1].bounds.get().right() <= layers[0].bounds.get().left(),
                "submenu should flip left at right screen edge: root={:?}, child={:?}, anchor={:?}",
                layers[0].bounds.get(),
                layers[1].bounds.get(),
                layers[1].anchor
            );
            assert!(window.find("restart-core").visible());
            window.press("left", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            assert_eq!(panel.read(cx).layers.len(), 1);
            gpui_kit::component::Theme::change(
                gpui_kit::component::ThemeMode::Light,
                Some(window),
                cx,
            );
            panel.update(cx, |panel, cx| panel.sync(window, cx));
            window.render_frame(cx);
            assert_eq!(panel.read(cx).layers[0].bounds.get().size, original);
            assert!(window.find("more").visible());
        })
        .unwrap();
    }
}
