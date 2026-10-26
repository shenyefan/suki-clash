//! Shared, interruptible motion. Framework sampling also honors reduced motion.
use super::theme::space;
use gpui_kit::{
    base::motion::{MotionReveal, Presence, PresenceSample, Transition},
    component::ActiveTheme,
    prelude::FluentBuilder as _,
    *,
};

pub(crate) fn presence(
    id: &'static str,
    visible: bool,
    window: &mut Window,
    cx: &mut App,
) -> PresenceSample {
    let motion = cx.theme().motion_tokens();
    Presence::new(id, visible)
        .transition(Transition::new(motion.duration_normal).easing(motion.easing_move.clone()))
        .sample(window, cx)
}

pub(crate) fn drawer(
    sample: PresenceSample,
    content: impl IntoElement,
    window: &Window,
) -> impl IntoElement {
    #[cfg(test)]
    use gpui_kit::test::TestSupportExt as _;
    div()
        .id("detail-drawer")
        .map(|drawer| {
            #[cfg(test)]
            let drawer = drawer.test_support();
            drawer
        })
        // Collapse the workspace's additional gap along with the drawer.
        .mt(space::LG.to_pixels(window.rem_size()) * (sample.progress - 1.))
        .flex_shrink_0()
        .min_h(space::NONE)
        .overflow_hidden()
        .child(MotionReveal::new(
            "detail-reveal",
            sample.progress,
            content.into_any_element(),
        ))
}

#[cfg(test)]
mod tests {
    use super::{drawer, presence};
    use gpui_kit::base::motion::PresenceSample;
    use gpui_kit::{Context, IntoElement, Render, Styled, TestAppContext, Window, div, px, size};
    use std::{cell::Cell, rc::Rc, time::Duration};

    struct Host {
        visible: Rc<Cell<bool>>,
        sample: Rc<Cell<Option<PresenceSample>>>,
    }
    impl Render for Host {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let sample = presence("drawer-regression", self.visible.get(), window, cx);
            self.sample.set(Some(sample));
            drawer(sample, div().h(px(225.)), window)
        }
    }

    #[gpui_kit::test]
    fn drawer_reverses_continuously_and_reduced_motion_snaps(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_reduce_motion(false);
        });
        let visible = Rc::new(Cell::new(false));
        let sample = Rc::new(Cell::new(None));
        let window = cx.open_window(size(px(640.), px(480.)), {
            let visible = visible.clone();
            let sample = sample.clone();
            move |_, _| Host { visible, sample }
        });
        cx.run_until_parked();
        assert_eq!(sample.get().unwrap().progress, 0.);
        visible.set(true);
        window.update(cx, |_, w, _| w.refresh()).unwrap();
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_millis(70));
        window.update(cx, |_, w, _| w.refresh()).unwrap();
        cx.run_until_parked();
        let opening = sample.get().unwrap().progress;
        assert!(opening > 0. && opening < 1.);
        visible.set(false);
        window.update(cx, |_, w, _| w.refresh()).unwrap();
        cx.run_until_parked();
        assert!((sample.get().unwrap().progress - opening).abs() < 0.001);
        cx.update(|cx| cx.set_reduce_motion(true));
        window.update(cx, |_, w, _| w.refresh()).unwrap();
        cx.run_until_parked();
        assert!(!sample.get().unwrap().should_render());
        visible.set(true);
        window.update(cx, |_, w, _| w.refresh()).unwrap();
        cx.run_until_parked();
        assert_eq!(sample.get().unwrap().progress, 1.);
    }
}
