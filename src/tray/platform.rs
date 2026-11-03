//! Native status icon and attached menu; the OS owns popup tracking and material.
use tray_icon::{Icon, TrayIcon, TrayIconBuilder, TrayIconEvent};

pub(super) struct Platform {
    icon: TrayIcon,
    monochrome: Icon,
    color: Icon,
    system_monochrome: Icon,
    system_color: Icon,
    tun_monochrome: Icon,
    tun_color: Icon,
    icon_attempt: Attempt<(bool, IconState)>,
    tooltip_attempt: Attempt<String>,
    title: Option<String>,
    custom: std::collections::BTreeMap<String, Icon>,
}
impl Platform {
    pub(super) fn new() -> Result<Self, String> {
        let image = image::load_from_memory(include_bytes!("../../assets/logo.png"))
            .map_err(|error| error.to_string())?
            .resize(32, 32, image::imageops::FilterType::Lanczos3)
            .into_rgba8();
        let make = |mono: bool, state: IconState| {
            let pixels = default_image(&image, mono, state);
            Icon::from_rgba(pixels.into_raw(), 32, 32).map_err(|error| error.to_string())
        };
        let monochrome = make(true, IconState::Common)?;
        let color = make(false, IconState::Common)?;
        let system_monochrome = make(true, IconState::System)?;
        let system_color = make(false, IconState::System)?;
        let tun_monochrome = make(true, IconState::Tun)?;
        let tun_color = make(false, IconState::Tun)?;
        let icon = TrayIconBuilder::new()
            .with_id("suki-clash")
            .with_tooltip("Suki Clash")
            .with_icon_templated(monochrome.clone())
            .with_menu_on_left_click(false)
            .with_menu_on_right_click(true)
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            icon,
            monochrome,
            color,
            system_monochrome,
            system_color,
            tun_monochrome,
            tun_color,
            icon_attempt: Attempt::default(),
            tooltip_attempt: Attempt::default(),
            title: None,
            custom: Default::default(),
        })
    }
    pub(super) fn events(sender: async_channel::Sender<TrayIconEvent>) {
        TrayIconEvent::set_event_handler(Some(move |event| {
            let _ = sender.try_send(event);
        }));
    }
    pub(super) fn update(
        &mut self,
        mono: bool,
        system: bool,
        tun: bool,
        tooltip: &str,
        speed: Option<&str>,
    ) -> Result<(), String> {
        let state = IconState::resolve(system, tun);
        let mut errors = Vec::new();
        if self.icon_attempt.changed((mono, state)) {
            let custom = self.custom.get(state.key());
            let image = custom.unwrap_or(match (mono, state) {
                (true, IconState::Common) => &self.monochrome,
                (false, IconState::Common) => &self.color,
                (true, IconState::System) => &self.system_monochrome,
                (false, IconState::System) => &self.system_color,
                (true, IconState::Tun) => &self.tun_monochrome,
                (false, IconState::Tun) => &self.tun_color,
            });
            let result = if mono && cfg!(target_os = "macos") {
                self.icon.set_icon_templated(Some(image.clone()))
            } else {
                self.icon.set_icon(Some(image.clone()))
            };
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

    pub(super) fn custom(&mut self, icons: std::collections::BTreeMap<String, Icon>) {
        self.custom = icons;
        self.icon_attempt.clear();
    }
    pub(super) fn menu(&self, menu: tray_icon::menu::Menu, left: bool) {
        self.icon.set_menu(Some(Box::new(menu)));
        self.icon.set_show_menu_on_left_click(left);
        self.icon.set_show_menu_on_right_click(true);
    }
}

fn default_image(image: &image::RgbaImage, mono: bool, state: IconState) -> image::RgbaImage {
    let mut pixels = image.clone();
    if mono {
        for pixel in pixels.pixels_mut() {
            pixel.0[..3].fill(0);
        }
    }
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
                    let cutout = if state == IconState::Tun {
                        (25..28).contains(&x) && (23..29).contains(&y)
                    } else {
                        distance < 16
                    };
                    pixels.put_pixel(
                        x,
                        y,
                        if mono {
                            image::Rgba([0, 0, 0, if cutout { 255 } else { 0 }])
                        } else {
                            image::Rgba([48, 185, 97, 255])
                        },
                    );
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
    fn key(self) -> &'static str {
        match self {
            Self::Common => "common",
            Self::System => "system",
            Self::Tun => "tun",
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
    fn native_default_tun_and_system_badges_have_distinct_pixels_in_both_styles() {
        let source = image::RgbaImage::from_pixel(32, 32, image::Rgba([100, 150, 200, 255]));
        for mono in [true, false] {
            let common = default_image(&source, mono, IconState::Common);
            let system = default_image(&source, mono, IconState::System);
            let tun = default_image(&source, mono, IconState::Tun);
            assert_ne!(common, system);
            assert_ne!(system, tun);
            assert_ne!(common, tun);
        }
    }
    #[test]
    fn default_and_custom_states_share_tun_priority() {
        assert_eq!(IconState::resolve(false, false).key(), "common");
        assert_eq!(IconState::resolve(true, false).key(), "system");
        assert_eq!(IconState::resolve(false, true).key(), "tun");
        assert_eq!(IconState::resolve(true, true).key(), "tun");
    }
}
