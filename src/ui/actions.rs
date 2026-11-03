//! Menu entries are text-only unless their command explicitly supplies an icon.
use gpui_kit::{SharedString, component::menu::PopupMenuItem};

pub(crate) fn menu(label: impl Into<SharedString>) -> PopupMenuItem {
    PopupMenuItem::new(label)
}
