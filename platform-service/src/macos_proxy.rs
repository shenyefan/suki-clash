//! Native macOS execution shared by local and privileged proxy owners.
mod native;
use crate::proxy_owner::{Backend, Target, types::OsSnapshot};
pub struct Adapter(native::Adapter);
pub struct Checkpoint(native::Checkpoint);
impl Adapter {
    pub fn new(journal: Option<std::path::PathBuf>, interactive: bool) -> Result<Self, String> {
        native::Adapter::new(journal, interactive).map(Self)
    }
}
pub fn read() -> Result<OsSnapshot, String> {
    native::read()
}
/// Read the primary network service without acquiring write authorization.
pub fn active_service_id() -> Result<Option<String>, String> {
    native::active_service_id()
}
impl Backend for Adapter {
    type Checkpoint = Checkpoint;
    fn available(&self) -> Result<bool, String> {
        self.0.available()
    }
    fn allow_interaction(&mut self, allowed: bool) {
        self.0.allow_interaction(allowed);
    }
    fn capture(&mut self) -> Result<(OsSnapshot, Checkpoint), String> {
        self.0
            .capture()
            .map(|(state, checkpoint)| (state, Checkpoint(checkpoint)))
    }
    fn read(&self) -> Result<OsSnapshot, String> {
        read()
    }
    fn write(&mut self, target: &Target) -> Result<(), String> {
        self.0.write(target)
    }
    fn restore(&mut self, checkpoint: &Checkpoint) -> Result<(), String> {
        self.0.restore(&checkpoint.0)
    }
    fn clear(&mut self, target: &Target) -> Result<OsSnapshot, String> {
        self.0.clear(Some(target))
    }
}
