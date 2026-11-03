//! Tray projection of the existing runtime. No second selection or network owner.
use crate::proxies::runtime::{Proxies, ProxyMode};

pub(super) fn groups(proxies: &Proxies, mode: Option<ProxyMode>) -> Vec<String> {
    if mode == Some(ProxyMode::Direct) {
        return Vec::new();
    }
    proxies
        .order
        .iter()
        .filter(|name| {
            proxies.entries.get(*name).is_some_and(|group| {
                !group.hidden
                    && group.all.is_some()
                    && if mode == Some(ProxyMode::Global) {
                        name.as_str() == "GLOBAL"
                    } else {
                        name.as_str() != "GLOBAL"
                    }
            })
        })
        .cloned()
        .collect()
}

pub(super) fn mode_label(mode: Option<ProxyMode>) -> &'static str {
    match mode {
        Some(ProxyMode::Rule) => "规则",
        Some(ProxyMode::Global) => "全局",
        Some(ProxyMode::Direct) => "直连",
        None => "未连接",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tray_filters_hidden_and_global_without_reordering_groups() {
        let p = Proxies::parse(br#"{"proxies":{"GLOBAL":{"name":"GLOBAL","type":"Selector","all":[]},"b":{"name":"b","type":"Selector","all":[]},"a":{"name":"a","type":"URLTest","all":[]},"hidden":{"name":"hidden","type":"Selector","all":[],"hidden":true}}}"#, &["b".into(),"a".into()]).unwrap();
        assert_eq!(groups(&p, Some(ProxyMode::Rule)), ["b", "a"]);
        assert_eq!(groups(&p, Some(ProxyMode::Global)), ["GLOBAL"]);
        assert!(groups(&p, Some(ProxyMode::Direct)).is_empty());
    }
}
