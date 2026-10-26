mod view;

use crate::session::Session;
use gpui_kit::*;

/// Dashboard controls own only their view state; runtime settings live in Session.
pub(crate) struct Overview {
    session: Entity<Session>,
    period: usize,
    breakdown: usize,
    proxy_only: bool,
    _subscription: Subscription,
}
impl Overview {
    pub(crate) fn new(session: Entity<Session>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&session, |_, _, cx| cx.notify());
        Self {
            session,
            period: 0,
            breakdown: 0,
            proxy_only: false,
            _subscription: subscription,
        }
    }
}
