//! All settings destinations use the same retained controls and page frame.
use super::SettingsPage;
use gpui_kit::{Context, IntoElement, Render, Window};
impl Render for SettingsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.runtime_view(window, cx)
    }
}
