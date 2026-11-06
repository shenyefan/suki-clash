//! Runtime rule positions are scoped to one core/configuration generation.
use serde::Deserialize;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub(crate) generation: u64,
    pub(crate) epoch: u64,
    pub(crate) entries: Vec<Rule>,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub(crate) struct Rule {
    #[serde(rename = "type")]
    pub(crate) kind: String,
    #[serde(default)]
    pub(crate) payload: String,
    pub(crate) proxy: String,
}
impl Snapshot {
    pub(crate) fn parse(bytes: &[u8], generation: u64, epoch: u64) -> Result<Self, String> {
        #[derive(Deserialize)]
        struct Response {
            rules: Vec<Rule>,
        }
        let response: Response = serde_json::from_slice(bytes)
            .map_err(|error| crate::logging::diagnostic_error("内核规则响应无效", error))?;
        if response.rules.len() > 100_000 {
            return Err("内核规则数量超过上限".into());
        }
        Ok(Self {
            generation,
            epoch,
            entries: response.rules,
        })
    }
    pub(super) fn rows(&self) -> Vec<super::model::Record> {
        self.entries
            .iter()
            .enumerate()
            .map(|(index, rule)| {
                let kind = rule_kind(&rule.kind);
                super::model::Record {
                    id: index as u64 + 1,
                    connection: None,
                    cells: vec![rule.payload.clone(), kind.clone(), rule.proxy.clone()],
                    numbers: vec![],
                    active: true,
                    enabled: true,
                    details: format!(
                        "顺序\t{}\n类型\t{kind}\n匹配内容\t{}\n策略\t{}",
                        index + 1,
                        rule.payload,
                        rule.proxy
                    ),
                }
            })
            .collect()
    }
}
fn rule_kind(kind: &str) -> String {
    // Mihomo's API uses CamelCase rule type names; source YAML uses hyphens.
    match kind {
        "Match" => "MATCH",
        "Domain" => "DOMAIN",
        "DomainSuffix" => "DOMAIN-SUFFIX",
        "DomainKeyword" => "DOMAIN-KEYWORD",
        "DomainRegex" => "DOMAIN-REGEX",
        "GeoIP" => "GEOIP",
        "GeoSite" => "GEOSITE",
        "IPCIDR" => "IP-CIDR",
        "IPCIDR6" => "IP-CIDR6",
        "IPASN" => "IP-ASN",
        "SrcIPCIDR" => "SRC-IP-CIDR",
        "SrcIPASN" => "SRC-IP-ASN",
        "SrcGeoIP" => "SRC-GEOIP",
        "SrcPort" => "SRC-PORT",
        "DstPort" => "DST-PORT",
        "ProcessName" => "PROCESS-NAME",
        "ProcessPath" => "PROCESS-PATH",
        "ProcessNameRegex" => "PROCESS-NAME-REGEX",
        "ProcessPathRegex" => "PROCESS-PATH-REGEX",
        "RuleSet" => "RULE-SET",
        "Network" => "NETWORK",
        "InType" => "IN-TYPE",
        "InUser" => "IN-USER",
        "InName" => "IN-NAME",
        "DSCP" => "DSCP",
        "AND" => "AND",
        "OR" => "OR",
        "NOT" => "NOT",
        "SubRules" => "SUB-RULE",
        _ => return kind.to_owned(),
    }
    .into()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rules_preserve_order_duplicates_unknown_types_and_validate_response() {
        let snapshot=Snapshot::parse(br#"{"rules":[{"type":"DomainSuffix","payload":"example.com","proxy":"A"},{"type":"FutureRule","payload":"x","proxy":"B"},{"type":"Match","proxy":"DIRECT"}]}"#,2,9).unwrap();
        let rows = snapshot.rows();
        assert_eq!(rows[0].cells, vec!["example.com", "DOMAIN-SUFFIX", "A"]);
        assert_eq!(rows[1].cells[1], "FutureRule");
        assert_eq!(super::super::table::rule_text(&rows[2]), "MATCH,DIRECT");
        assert!(Snapshot::parse(br#"{}"#, 2, 9).is_err());
    }
}
