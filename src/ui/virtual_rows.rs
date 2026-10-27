//! Lazy rows with one retained native scroll owner.
use gpui_kit::{
    component::{VirtualListScrollHandle, scroll::Scrollbar, v_virtual_list},
    *,
};
use std::rc::Rc;

pub(crate) fn rows<V: Render>(
    owner: Entity<V>,
    id: impl Into<ElementId>,
    heights: Vec<Pixels>,
    handle: &VirtualListScrollHandle,
    render: impl Fn(&mut V, usize, &mut Window, &mut Context<V>) -> AnyElement + 'static,
) -> AnyElement {
    let sizes: Rc<Vec<Size<Pixels>>> = Rc::new(
        heights
            .into_iter()
            .map(|height| size(px(0.), height))
            .collect(),
    );
    let sizes_for_rows = sizes.clone();
    div()
        .relative()
        .size_full()
        .min_h(px(0.))
        .child(
            v_virtual_list(owner, id, sizes, move |view, range, window, cx| {
                range
                    .map(|index| {
                        div()
                            .w_full()
                            .h(sizes_for_rows[index].height)
                            .flex_none()
                            .overflow_hidden()
                            .child(render(view, index, window, cx))
                    })
                    .collect::<Vec<_>>()
            })
            .track_scroll(handle),
        )
        .child(Scrollbar::vertical(handle))
        .into_any_element()
}

/// A bounded data viewport inside a settings form; the list owns its scrolling.
pub(crate) fn panel<V: Render>(
    owner: Entity<V>,
    id: impl Into<ElementId>,
    heights: Vec<Pixels>,
    handle: &VirtualListScrollHandle,
    render: impl Fn(&mut V, usize, &mut Window, &mut Context<V>) -> AnyElement + 'static,
) -> AnyElement {
    let height: Pixels = heights
        .iter()
        .take(super::theme::list::MAX_VISIBLE_ROWS)
        .copied()
        .sum();
    div()
        .w_full()
        .h(height)
        .flex_none()
        .child(rows(owner, id, heights, handle, render))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::rows;
    use gpui_kit::{
        AppContext, Bounds, Context, IntoElement, ParentElement, Render, ScrollStrategy,
        TestAppContext, Window, WindowBounds, WindowOptions, component::VirtualListScrollHandle,
        div, px, size, test::TestWindowExt,
    };
    struct Harness {
        scroll: VirtualListScrollHandle,
        seen: std::collections::BTreeSet<usize>,
    }
    impl Render for Harness {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            rows(
                cx.entity(),
                "large-rows",
                vec![px(30.); 10000],
                &self.scroll,
                |this, index, _, _| {
                    this.seen.insert(index);
                    div().child(index.to_string()).into_any_element()
                },
            )
        }
    }
    #[gpui_kit::test]
    fn large_dataset_renders_only_visible_rows_and_scrolls_to_model_index(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (window, owner) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        Default::default(),
                        size(px(800.), px(600.)),
                    ))),
                    ..Default::default()
                },
                cx,
                |_, cx| {
                    cx.new(|_| Harness {
                        scroll: VirtualListScrollHandle::new(),
                        seen: Default::default(),
                    })
                },
            )
            .unwrap()
        });
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert!(owner.read(cx).seen.len() < 100);
            assert!(!owner.read(cx).seen.contains(&5000));
            owner.update(cx, |view, cx| {
                view.seen.clear();
                view.scroll.scroll_to_item(5000, ScrollStrategy::Top);
                cx.notify();
            });
            window.render_frame(cx);
            window.render_frame(cx);
            assert!(owner.read(cx).seen.contains(&5000));
            assert!(owner.read(cx).seen.len() < 100);
        })
        .unwrap();
    }
}
