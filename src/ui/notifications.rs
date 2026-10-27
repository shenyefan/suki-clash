//! One native notification list per window, painted above modal popups.
use gpui_kit::{
    base::{POPUP_PRIORITY, Root, RootPlugin},
    component::notification::{Notification, NotificationList},
    *,
};
struct ToastLayer {
    list: Entity<NotificationList>,
    _list_subscription: Subscription,
}
impl ToastLayer {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let list = cx.new(|cx| NotificationList::new(window, cx));
        let subscription = cx.observe(&list, |_, _, cx| cx.notify());
        Self {
            list,
            _list_subscription: subscription,
        }
    }
}
impl RootPlugin for ToastLayer {}
impl Render for ToastLayer {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        deferred(div().absolute().inset_0().child(self.list.clone()))
            .with_priority(POPUP_PRIORITY + 10)
    }
}
pub(crate) fn init(cx: &mut App) {
    Root::register_plugin(cx, ToastLayer::new);
}
fn list(window: &Window, cx: &App) -> Option<Entity<NotificationList>> {
    Root::read(window, cx)
        .plugin::<ToastLayer>()
        .map(|layer| layer.read(cx).list.clone())
}
pub(crate) fn push(
    window: &mut Window,
    note: impl Into<Notification>,
    cx: &mut App,
) -> Option<Entity<Notification>> {
    let list = list(window, cx)?;
    let before = list.read(cx).notifications();
    list.update(cx, |list, cx| list.push(note, window, cx));
    list.read(cx)
        .notifications()
        .into_iter()
        .find(|note| !before.contains(note))
}
#[cfg(test)]
pub(crate) fn current(window: &Window, cx: &App) -> Vec<Entity<Notification>> {
    list(window, cx)
        .map(|list| list.read(cx).notifications())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{init, push};
    use gpui_kit::component::notification::Notification;
    use gpui_kit::component::{Theme, WindowExt, button::Button};
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{
        Anchor, AppContext, Bounds, Context, IntoElement, ParentElement, Render, Styled,
        TestAppContext, Window, WindowBounds, WindowOptions, div, px, size,
    };
    use std::{cell::Cell, rc::Rc};
    struct Host;
    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }
    #[gpui_kit::test]
    fn top_center_toast_remains_clickable_above_an_open_dialog(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            init(cx);
            cx.set_reduce_motion(true);
            Theme::update(cx, |theme| theme.notification.placement = Anchor::TopCenter);
        });
        let (handle, _) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        Default::default(),
                        size(px(1100.), px(800.)),
                    ))),
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(|_| Host),
            )
            .unwrap()
        });
        let clicks = Rc::new(Cell::new(0));
        let count = clicks.clone();
        cx.update_window(handle, |_, window, cx| {
            window.open_dialog(cx, |dialog, _, _| {
                dialog.title("编辑订阅").child("保留弹窗")
            });
            push(
                window,
                Notification::new().autohide(false).content(move |_, _, _| {
                    let count = count.clone();
                    Button::new("toast-layer-action")
                        .outline()
                        .w_full()
                        .label("通知动作")
                        .on_click(move |_, _, _| count.set(count.get() + 1))
                        .into_any_element()
                }),
                cx,
            );
        })
        .unwrap();
        cx.run_until_parked();
        cx.background_executor
            .advance_clock(std::time::Duration::from_secs(1));
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let bounds = window.find("toast-layer-action").bounds();
            assert!((bounds.center().x - px(550.)).abs() < px(2.));
            assert!(bounds.top() < px(100.));
            window.click("toast-layer-action", cx);
        })
        .unwrap();
        cx.run_until_parked();
        assert_eq!(clicks.get(), 1);
    }
}
