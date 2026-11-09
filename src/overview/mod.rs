pub(crate) mod history;
pub(crate) mod statistics;
mod view;

use crate::session::Session;
use gpui_kit::*;

/// Dashboard controls own only their view state; runtime settings live in Session.
pub(crate) struct Overview {
    session: Entity<Session>,
    breakdown: usize,
    proxy_only: bool,
    month: bool,
    ranking_scroll: component::VirtualListScrollHandle,
    visible: bool,
    _subscription: Subscription,
}
impl Overview {
    pub(crate) fn new(session: Entity<Session>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&session, |this, _, cx| {
            if this.visible {
                cx.notify();
            }
        });
        Self {
            session,
            breakdown: 0,
            proxy_only: false,
            month: false,
            ranking_scroll: component::VirtualListScrollHandle::new(),
            visible: true,
            _subscription: subscription,
        }
    }
    pub(crate) fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        self.visible = visible;
        if visible {
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Overview;
    use crate::overview::history::{Bytes, Hour, Snapshot};
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{AppContext, Bounds, TestAppContext, WindowBounds, WindowOptions, px, size};

    #[gpui_kit::test]
    fn hidden_graph_skips_notifications_and_notifies_once_on_return(cx: &mut TestAppContext) {
        let session =
            cx.new(|_| crate::session::Session::fixture(Default::default(), Default::default()));
        let view = cx.new(|cx| Overview::new(session.clone(), cx));
        let notifications = std::rc::Rc::new(std::cell::Cell::new(0));
        let _subscription = cx.update(|cx| {
            cx.observe(&view, {
                let notifications = notifications.clone();
                move |_, _| notifications.set(notifications.get() + 1)
            })
        });
        view.update(cx, |view, cx| view.set_visible(false, cx));
        session.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
        assert_eq!(notifications.get(), 0);
        view.update(cx, |view, cx| view.set_visible(true, cx));
        cx.run_until_parked();
        assert_eq!(notifications.get(), 1);
        session.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
        assert_eq!(notifications.get(), 2);
    }

    #[gpui_kit::test]
    fn hourly_card_fits_and_native_scope_and_period_tabs_update_the_view(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_reduce_motion(true);
        });
        let (handle, page) = cx.update(|cx| {
            let page = std::rc::Rc::new(std::cell::RefCell::new(None));
            let captured = page.clone();
            let handle = gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        Default::default(),
                        size(px(960.), px(900.)),
                    ))),
                    ..Default::default()
                },
                cx,
                move |_, cx| {
                    let hour = Hour {
                        all: Bytes {
                            upload: 100,
                            download: 200,
                        },
                        proxy: Bytes {
                            upload: 10,
                            download: 20,
                        },
                        samples: 1,
                    };
                    let mut runtime = crate::core::Runtime {
                        history: Snapshot {
                            available: true,
                            today: hour,
                            month: Hour {
                                all: Bytes {
                                    upload: 1000,
                                    download: 2000,
                                },
                                ..hour
                            },
                            hours: [hour; 24],
                        },
                        ..Default::default()
                    };
                    let info = runtime.ip_info.get_or_insert_with(Default::default);
                    info.country = "country".into();
                    info.city = "city".into();
                    info.asn_organization = "network".into();
                    info.organization = "isp".into();
                    info.timezone = "Asia/Shanghai".into();
                    info.latitude_microdegrees = Some(31230400);
                    info.longitude_microdegrees = Some(121473700);
                    let session =
                        cx.new(|_| crate::session::Session::fixture(Default::default(), runtime));
                    let view = cx.new(|cx| Overview::new(session, cx));
                    *captured.borrow_mut() = Some(view.clone());
                    view
                },
            )
            .unwrap();
            let view = page.borrow().clone().unwrap();
            (handle, view)
        });
        cx.update_window(handle.0, |_, window, cx| {
            window.render_frame(cx);
            let plot = window.find("hourly-traffic-plot").bounds();
            assert!(plot.size.width > px(0.) && plot.size.height > px(0.));
            assert!(plot.right() <= window.viewport_size().width);
            let ip = window.find("overview-ip-details").bounds();
            let coordinates = window.find("overview-ip-coordinates").bounds();
            assert!(
                coordinates.bottom() <= ip.bottom(),
                "IP fields must fit inside their card"
            );
            let proxy = window
                .within("traffic-scope")
                .find_by_label("仅代理")
                .path()
                .last()
                .unwrap()
                .clone();
            window.within("traffic-scope").click(proxy, cx);
            assert!(page.read(cx).proxy_only);
            let month = window
                .within("traffic-period")
                .find_by_label("本月")
                .path()
                .last()
                .unwrap()
                .clone();
            window.within("traffic-period").click(month, cx);
            assert!(page.read(cx).month);
            assert!(page.read(cx).proxy_only);
            let all = window
                .within("traffic-scope")
                .find_by_label("全部")
                .path()
                .last()
                .unwrap()
                .clone();
            window.within("traffic-scope").click(all, cx);
            assert!(!page.read(cx).proxy_only);
            assert!(page.read(cx).month);
        })
        .unwrap();
    }
}
