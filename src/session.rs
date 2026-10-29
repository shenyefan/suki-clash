pub(crate) use crate::proxies::runtime::ProxyMode;
/// Read-only projection of observed runtime state shared by the feature views.
#[derive(Default)]
pub(crate) struct Session {
    tun: Option<bool>,
    core: crate::core::Snapshot,
    core_busy: bool,
    _core_subscription: Option<gpui_kit::Subscription>,
    runtime: std::sync::Arc<crate::core::Runtime>,
}
impl Session {
    #[cfg(test)]
    pub(crate) fn fixture(core: crate::core::Snapshot, runtime: crate::core::Runtime) -> Self {
        Self {
            core,
            runtime: std::sync::Arc::new(runtime),
            ..Default::default()
        }
    }

    pub(crate) fn new(cx: &mut gpui_kit::Context<Self>) -> Self {
        let controller = crate::core::controller(cx);
        let core = controller.read(cx).snapshot().clone();
        let core_busy = controller.read(cx).is_busy();
        let runtime = controller.read(cx).runtime();
        let subscription = cx.observe(&controller, |this, controller, cx| {
            let observed = controller.read(cx).snapshot();
            if observed.generation() >= this.core.generation() {
                this.core = observed.clone();
                this.runtime = controller.read(cx).runtime();
                this.core_busy = controller.read(cx).is_busy();
                cx.notify();
            }
        });
        Self {
            core,
            core_busy,
            runtime,
            _core_subscription: Some(subscription),
            ..Self::default()
        }
    }
    pub(crate) fn runtime(&self) -> std::sync::Arc<crate::core::Runtime> {
        self.runtime.clone()
    }
    pub(crate) fn core(&self) -> &crate::core::Snapshot {
        &self.core
    }
    pub(crate) fn core_busy(&self) -> bool {
        self.core_busy
    }
    pub(crate) fn system_proxy(&self) -> Option<bool> {
        self.core.system_proxy().enabled()
    }
    pub(crate) fn tun(&self) -> Option<bool> {
        self.tun
    }
    pub(crate) fn is_running(&self) -> bool {
        self.core.phase() == crate::core::Phase::Ready
    }
    pub(crate) fn mode(&self) -> Option<ProxyMode> {
        if self.is_running() && self.runtime.connected {
            self.runtime.mode
        } else {
            None
        }
    }
    pub(crate) fn active_profile_id(&self) -> Option<u64> {
        self.core.profile().map(|p| p.id())
    }
    pub(crate) fn profile_name(&self) -> Option<&str> {
        self.core.profile().map(|p| p.name())
    }
    pub(crate) fn selected_node(&self, group: &str) -> Option<&str> {
        self.runtime.proxies.selected(group)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{
        AppContext, Bounds, TestAppContext, WindowBounds, WindowOptions, px, size,
        test::TestWindowExt,
    };
    #[gpui_kit::test]
    fn dashboard_renders_first_live_sample_and_period_controls_without_empty_plots(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_kit::init);
        let (window, _) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        Default::default(),
                        size(px(1200.), px(900.)),
                    ))),
                    ..Default::default()
                },
                cx,
                |_, cx| {
                    let runtime = crate::core::Runtime {
                        connected: true,
                        statistics: crate::overview::statistics::Summary {
                            upload_rate: Some(1024),
                            download_rate: Some(2048),
                            rates: vec![crate::overview::statistics::Point {
                                time: "12:00:00".into(),
                                upload: 1024,
                                download: 2048,
                            }],
                            ..Default::default()
                        },
                        ..Default::default()
                    };
                    let session = cx.new(|_| Session {
                        runtime: std::sync::Arc::new(runtime),
                        ..Default::default()
                    });
                    cx.new(|cx| crate::overview::Overview::new(session, cx))
                },
            )
            .unwrap()
        });
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            for id in ["upload-rate-plot", "download-rate-plot"] {
                let bounds = window.find(id).bounds();
                assert!(bounds.size.width > px(0.) && bounds.size.height > px(0.));
                assert!(bounds.right() <= window.viewport_size().width);
            }
            assert!(window.try_find("upload-empty-plot").is_none());
            assert!(window.try_find("download-empty-plot").is_none());
        })
        .unwrap();
    }
}
