//! Retained native toast content shared by long-running download workflows.
use super::theme::{space, text};
use gpui_kit::{
    component::{Sizable, notification::Notification, progress::Progress, v_flex},
    *,
};

pub(crate) struct DownloadToast {
    label: String,
    received: u64,
    total: Option<u64>,
    notification: Option<WeakEntity<Notification>>,
}
impl DownloadToast {
    pub(crate) fn show(
        label: &str,
        received: u64,
        total: Option<u64>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        let view = cx.new(|_| Self {
            label: label.into(),
            received,
            total,
            notification: None,
        });
        let content = view.clone();
        let notification = super::notifications::progress(
            window,
            Notification::new()
                .id1::<Self>(("download", view.entity_id()))
                .content(move |_, _, _| content.clone().into_any_element()),
            cx,
        );
        view.update(cx, |toast, _| {
            toast.notification = notification.map(|note| note.downgrade())
        });
        view
    }
    pub(crate) fn dismiss(toast: &Entity<Self>, window: &mut Window, cx: &mut App) {
        let notification = toast.read(cx).notification.clone();
        if let Some(note) = notification.and_then(|note| note.upgrade()) {
            note.update(cx, |note, cx| note.dismiss(window, cx));
        }
    }
    pub(crate) fn update_progress(
        &mut self,
        label: &str,
        received: u64,
        total: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        self.label = label.into();
        self.received = received;
        self.total = total;
        cx.notify();
    }
}
impl Render for DownloadToast {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let total = self
            .total
            .filter(|total| *total > 0 && *total >= self.received);
        let percent = total.map(|total| (self.received as f64 / total as f64 * 100.) as f32);
        v_flex()
            .w_full()
            .gap(space::SM)
            .child(self.label.clone())
            .child(
                Progress::new("download-progress")
                    .small()
                    .loading(percent.is_none())
                    .value(percent.unwrap_or(0.))
                    .accessibility_label(format!("{}进度", self.label)),
            )
            .child(
                super::muted(
                    match (percent, total) {
                        (Some(percent), Some(total)) => {
                            format!(
                                "{percent:.0}% · {} / {}",
                                super::file_size(self.received),
                                super::file_size(total)
                            )
                        }
                        _ if self.received > 0 => {
                            format!("已接收 {}", super::file_size(self.received))
                        }
                        _ => "处理中".into(),
                    },
                    cx,
                )
                .text_size(text::CAPTION),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::DownloadToast;
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{
        AppContext, Context, IntoElement, Render, TestAppContext, Window, WindowOptions, div,
    };
    use std::time::Duration;
    struct Host;
    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }
    #[gpui_kit::test]
    fn progress_toast_survives_autohide_and_updates_without_replacing_notification(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            super::super::notifications::init(cx);
            cx.set_reduce_motion(true);
        });
        let (window, _) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| Host)).unwrap()
        });
        let (toast, note) = window
            .update(cx, |_, window, cx| {
                let toast = DownloadToast::show("下载内核", 0, None, window, cx);
                let note = super::super::notifications::current(window, cx)[0].clone();
                window.render_frame(cx);
                (toast, note)
            })
            .unwrap();
        cx.background_executor
            .advance_clock(Duration::from_secs(10));
        cx.run_until_parked();
        window
            .update(cx, |_, window, cx| {
                assert_eq!(super::super::notifications::current(window, cx).len(), 1);
                toast.update(cx, |toast, cx| {
                    toast.update_progress("下载内核", 512, Some(1024), cx)
                });
                window.render_frame(cx);
                assert_eq!(super::super::notifications::current(window, cx)[0], note);
                assert_eq!(toast.read(cx).received, 512);
                DownloadToast::dismiss(&toast, window, cx);
            })
            .unwrap();
        cx.background_executor.advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        window
            .update(cx, |_, window, cx| {
                assert!(super::super::notifications::current(window, cx).is_empty());
            })
            .unwrap();
    }
}

#[cfg(test)]
mod size_tests {
    #[test]
    fn byte_counts_use_readable_units() {
        for (bytes, expected) in [
            (0, "0 B"),
            (1023, "1023 B"),
            (1024, "1.0 KB"),
            (1536, "1.5 KB"),
            (1048576, "1.0 MB"),
            (1073741824, "1.0 GB"),
        ] {
            assert_eq!(super::super::file_size(bytes), expected);
        }
        assert!(!super::super::file_size(u64::MAX).is_empty());
    }
}
