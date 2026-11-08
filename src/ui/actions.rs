//! Action entries supply semantic icons; value choices and filters remain text-only.
use gpui_kit::{SharedString, component::menu::PopupMenuItem};

pub(crate) fn menu(label: impl Into<SharedString>) -> PopupMenuItem {
    PopupMenuItem::new(label)
}
