//! Protocol snapshot types adapted from sysproxy-rs (MIT), revision
//! c37a60e1fc9079cc0f074200e2296afd1705208c. See licenses/sysproxy-MIT.txt.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ManualProxy {
    pub host: String,
    pub bypass: String,
    pub port: u16,
    pub enable: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PacProxy {
    pub url: String,
    pub enable: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
    pub enable: bool,
    pub switched_on: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OsSnapshot {
    pub socks: Endpoint,
    pub http: Endpoint,
    pub https: Endpoint,
    pub auto: PacProxy,
    pub auto_switched_on: bool,
    pub bypass: String,
    pub auto_detect: bool,
}
impl OsSnapshot {
    pub fn is_all_disabled(&self) -> bool {
        !self.socks.switched_on
            && !self.http.switched_on
            && !self.https.switched_on
            && !self.auto_switched_on
            && !self.auto_detect
    }
    pub fn matches_pac(&self, target: &PacProxy) -> bool {
        target.enable
            && self.auto_switched_on
            && self.auto.enable
            && self.auto.url == target.url
            && !self.auto_detect
            && !self.socks.switched_on
            && !self.http.switched_on
            && !self.https.switched_on
    }
}
