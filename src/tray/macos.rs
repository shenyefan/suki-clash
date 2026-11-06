//! Native attributed menu titles. AppKit supplies font metrics and menu material.
use objc2::{AnyThread, runtime::AnyObject};
use objc2_app_kit::{
    NSAttributedStringNSStringDrawing, NSColor, NSFont, NSFontAttributeName,
    NSForegroundColorAttributeName, NSMutableParagraphStyle, NSParagraphStyleAttributeName,
    NSTextAlignment, NSTextTab,
};
use objc2_foundation::{
    NSArray, NSAttributedString, NSDictionary, NSMutableAttributedString, NSRange, NSString,
};
pub(super) fn width<'a>(groups: impl Iterator<Item = (&'a str, &'a str)>) -> f64 {
    let font = NSFont::menuFontOfSize(0.);
    let attributes =
        unsafe { NSDictionary::from_slices(&[NSFontAttributeName], &[&*font as &AnyObject]) };
    let measure = |text: &str| unsafe {
        NSAttributedString::initWithString_attributes(
            NSAttributedString::alloc(),
            &NSString::from_str(text),
            Some(&attributes),
        )
        .size()
        .width
    };
    let mut leading = 0f64;
    let mut trailing = 0f64;
    for (name, node) in groups {
        leading = leading.max(measure(name));
        trailing = trailing.max(measure(node));
    }
    // One em of separation; the width is native text geometry, not a UI pixel patch.
    leading + trailing + font.pointSize() * 2.
}
pub(super) fn title(menu: &tray_icon::menu::Submenu, name: &str, node: &str, width: f64) {
    let font = NSFont::menuFontOfSize(0.);
    let paragraph = NSMutableParagraphStyle::new();
    let tab = unsafe {
        NSTextTab::initWithTextAlignment_location_options(
            NSTextTab::alloc(),
            NSTextAlignment::Right,
            width,
            &NSDictionary::new(),
        )
    };
    paragraph.setTabStops(Some(&NSArray::from_retained_slice(&[tab])));
    let attributes = unsafe {
        NSDictionary::from_slices(
            &[NSFontAttributeName, NSParagraphStyleAttributeName],
            &[&*font as &AnyObject, &*paragraph as &AnyObject],
        )
    };
    let text = NSString::from_str(&format!("{name}\t{node}"));
    let title = unsafe {
        NSMutableAttributedString::initWithString_attributes(
            NSMutableAttributedString::alloc(),
            &text,
            Some(&attributes),
        )
    };
    let range = NSRange::new(name.encode_utf16().count() + 1, node.encode_utf16().count());
    unsafe {
        title.addAttribute_value_range(
            NSForegroundColorAttributeName,
            &NSColor::secondaryLabelColor(),
            range,
        );
    }
    menu.set_attributed_title(Some(&title));
}

/// AppKit menus inherit application appearance, including native submenus.
pub(super) fn theme(dark: bool) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{
        NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication,
    };
    let Some(main) = MainThreadMarker::new() else {
        return;
    };
    let name = unsafe {
        if dark {
            NSAppearanceNameDarkAqua
        } else {
            NSAppearanceNameAqua
        }
    };
    let appearance = NSAppearance::appearanceNamed(name);
    NSApplication::sharedApplication(main).setAppearance(appearance.as_deref());
}
