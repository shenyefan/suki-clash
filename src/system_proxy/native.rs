use super::{Backend, Target, types::OsSnapshot};
#[cfg(target_os = "linux")]
#[path = "linux.rs"]
mod platform;
#[cfg(target_os = "macos")]
#[path = "macos.rs"]
mod platform;
#[cfg(windows)]
#[path = "windows.rs"]
mod platform;
#[derive(Default)]
pub(super) struct Native {
    #[cfg(target_os = "macos")]
    adapter: platform::Adapter,
}
impl Backend for Native {
    type Checkpoint = platform::Checkpoint;
    fn capture(&mut self) -> Result<(OsSnapshot, Self::Checkpoint), String> {
        #[cfg(target_os = "macos")]
        {
            self.adapter.capture()
        }
        #[cfg(any(target_os = "linux", windows))]
        {
            platform::capture()
        }
    }
    fn read(&self) -> Result<OsSnapshot, String> {
        platform::read()
    }
    fn write(&mut self, target: &Target) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            self.adapter.write(target)
        }
        #[cfg(any(target_os = "linux", windows))]
        {
            platform::write(target)
        }
    }
    fn restore(&mut self, checkpoint: &Self::Checkpoint) -> Result<(), String> {
        platform::restore(checkpoint)
    }
    fn clear(&mut self, target: &Target) -> Result<OsSnapshot, String> {
        #[cfg(target_os = "macos")]
        {
            self.adapter.clear(Some(target))
        }
        #[cfg(any(target_os = "linux", windows))]
        {
            super::clear_current(self, target)
        }
    }
}
