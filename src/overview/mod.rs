pub(crate) mod statistics;
mod view;

use crate::session::Session;
use gpui_kit::*;

/// Dashboard controls own only their view state; runtime settings live in Session.
pub(crate) struct Overview {
    session: Entity<Session>,
    breakdown: usize,
    ranking_scroll: gpui_kit::component::VirtualListScrollHandle,
    _subscription: Subscription,
}
impl Overview {
    pub(crate) fn new(session: Entity<Session>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&session, |_, _, cx| cx.notify());
        Self {
            session,
            breakdown: 0,
            ranking_scroll: gpui_kit::component::VirtualListScrollHandle::new(),
            _subscription: subscription,
        }
    }
}
