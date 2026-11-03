pub(crate) mod statistics;
mod view;

use crate::session::Session;
use gpui_kit::*;

/// Dashboard controls own only their view state; runtime settings live in Session.
pub(crate) struct Overview {
    session: Entity<Session>,
    period: usize,
    breakdown: usize,
    proxy_only: bool,
    show_ip: bool,
    ranking_scroll: gpui_kit::component::VirtualListScrollHandle,
    _subscription: Subscription,
    system: Option<crate::desktop::system::Info>,
    started: std::time::Instant,
    _system_task: Task<()>,
}
impl Overview {
    pub(crate) fn new(session: Entity<Session>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&session, |_, _, cx| cx.notify());
        let system_task = cx.spawn(async move |owner, cx| {
            let info = cx
                .background_spawn(async { crate::desktop::system::Info::collect() })
                .await;
            if owner
                .update(cx, |this, cx| {
                    this.system = Some(info);
                    cx.notify();
                })
                .is_err()
            {
                return;
            }
            let mut ticks = 0;
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(1))
                    .await;
                ticks += 1;
                let refreshed = if ticks % 30 == 0 {
                    Some(
                        cx.background_spawn(async { crate::desktop::system::Info::collect() })
                            .await,
                    )
                } else {
                    None
                };
                if owner
                    .update(cx, |this, cx| {
                        if let Some(info) = refreshed {
                            this.system = Some(info);
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            session,
            period: 0,
            breakdown: 0,
            proxy_only: false,
            show_ip: false,
            ranking_scroll: gpui_kit::component::VirtualListScrollHandle::new(),
            _subscription: subscription,
            system: None,
            started: std::time::Instant::now(),
            _system_task: system_task,
        }
    }
}
