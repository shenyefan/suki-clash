//! Native status icon. GPUI owns the popup; no OS menu is attached.
#[cfg(target_os = "linux")]
use super::linux::{Icon, Native as TrayIcon};
use tray_icon::TrayIconEvent;
#[cfg(not(target_os = "linux"))]
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

pub(super) enum Event {
    Icon(TrayIconEvent),
    #[cfg(target_os = "linux")]
    Error(String),
}
pub(super) struct Platform {
    icon: TrayIcon,
    common: Icon,
    system: Icon,
    tun: Icon,
    icon_attempt: Attempt<IconState>,
    tooltip_attempt: Attempt<String>,
    title: Option<String>,
}
impl Platform {
    pub(super) fn new(
        cx: &gpui_kit::App,
        sender: async_channel::Sender<Event>,
    ) -> Result<Self, String> {
        let image = image::load_from_memory(include_bytes!("../../assets/logo.png"))
            .map_err(crate::logging::error_message)?
            .resize(32, 32, image::imageops::FilterType::Lanczos3)
            .into_rgba8();
        let make = |state: IconState| {
            let pixels = default_image(&image, state);
            Icon::from_rgba(pixels.into_raw(), 32, 32).map_err(crate::logging::error_message)
        };
        let common = make(IconState::Common)?;
        let system = make(IconState::System)?;
        let tun = make(IconState::Tun)?;
        #[cfg(not(target_os = "linux"))]
        let builder = TrayIconBuilder::new()
            .with_id("suki-clash")
            .with_tooltip("Suki Clash");
        #[cfg(not(target_os = "linux"))]
        let icon = builder
            .with_icon(common.clone())
            .with_menu_on_left_click(false)
            .with_menu_on_right_click(false)
            .build()
            .map_err(crate::logging::error_message)?;
        #[cfg(not(target_os = "linux"))]
        {
            Self::events(sender);
            let _ = cx;
        }
        #[cfg(target_os = "linux")]
        let icon = TrayIcon::new(common.clone(), sender, cx);
        Ok(Self {
            icon,
            common,
            system,
            tun,
            icon_attempt: Attempt::default(),
            tooltip_attempt: Attempt::default(),
            title: None,
        })
    }
    #[cfg(not(target_os = "linux"))]
    fn events(sender: async_channel::Sender<Event>) {
        TrayIconEvent::set_event_handler(Some(move |event| {
            let _ = sender.try_send(Event::Icon(event));
        }));
    }
    pub(super) fn update(
        &mut self,
        system: bool,
        tun: bool,
        tooltip: &str,
        speed: Option<&str>,
    ) -> Result<(), String> {
        let state = IconState::resolve(system, tun);
        let mut errors = Vec::new();
        if self.icon_attempt.changed(state) {
            let image = match state {
                IconState::Common => &self.common,
                IconState::System => &self.system,
                IconState::Tun => &self.tun,
            };
            let result = self.icon.set_icon(Some(image.clone()));
            if let Err(error) = result {
                errors.push(format!("托盘图标：{error}"));
            }
        }
        if self.tooltip_attempt.changed(tooltip.to_owned())
            && let Err(error) = self.icon.set_tooltip(Some(tooltip))
        {
            errors.push(format!("托盘提示：{error}"));
        }
        if self.title.as_deref() != speed {
            self.icon.set_title(speed);
            self.title = speed.map(str::to_owned);
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("；"))
        }
    }
    pub(super) fn retry(&mut self) {
        self.icon_attempt.clear();
        self.tooltip_attempt.clear();
    }
}

fn default_image(image: &image::RgbaImage, state: IconState) -> image::RgbaImage {
    let mut pixels = image.clone();
    // Native status icons use a circular proxy badge and rectangular TUN badge.
    if state != IconState::Common {
        for y in 20..32 {
            for x in 20..32 {
                let distance = (x as i32 - 26).pow(2) + (y as i32 - 26).pow(2);
                let badge = match state {
                    IconState::Tun => (22..31).contains(&x) && (21..31).contains(&y),
                    _ => distance < 36,
                };
                if badge {
                    pixels.put_pixel(x, y, image::Rgba([48, 185, 97, 255]));
                }
            }
        }
    }
    pixels
}

/// The last attempted target also gates failures: sampling must not retry an unchanged OS write.
struct Attempt<T>(Option<T>);
impl<T> Default for Attempt<T> {
    fn default() -> Self {
        Self(None)
    }
}
impl<T: PartialEq> Attempt<T> {
    fn changed(&mut self, target: T) -> bool {
        if self.0.as_ref() == Some(&target) {
            return false;
        }
        self.0 = Some(target);
        true
    }
    fn clear(&mut self) {
        self.0 = None;
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum IconState {
    Common,
    System,
    Tun,
}
impl IconState {
    fn resolve(system: bool, tun: bool) -> Self {
        if tun {
            Self::Tun
        } else if system {
            Self::System
        } else {
            Self::Common
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identical_failed_targets_pause_until_change_or_explicit_retry() {
        let mut attempts = Attempt::default();
        assert!(attempts.changed("target"));
        for _ in 0..100 {
            assert!(!attempts.changed("target"));
        }
        assert!(attempts.changed("changed"));
        assert!(!attempts.changed("changed"));
        attempts.clear();
        assert!(attempts.changed("changed"));
    }
    #[test]
    fn native_default_tun_and_system_badges_have_distinct_pixels_() {
        let source = image::RgbaImage::from_pixel(32, 32, image::Rgba([100, 150, 200, 255]));
        let common = default_image(&source, IconState::Common);
        let system = default_image(&source, IconState::System);
        let tun = default_image(&source, IconState::Tun);
        assert_ne!(common, system);
        assert_ne!(system, tun);
        assert_ne!(common, tun);
    }
    #[test]
    fn native_states_use_tun_priority() {
        assert!(matches!(
            IconState::resolve(false, false),
            IconState::Common
        ));
        assert!(matches!(IconState::resolve(true, false), IconState::System));
        assert!(matches!(IconState::resolve(false, true), IconState::Tun));
        assert!(matches!(IconState::resolve(true, true), IconState::Tun));
    }
}
