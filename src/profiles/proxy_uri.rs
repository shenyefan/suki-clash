//! Proxy share links are parsed once at the import boundary, never written to logs.
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD},
};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

fn decoded(text: &str) -> Result<String, String> {
    [STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD]
        .iter()
        .find_map(|engine| engine.decode(text.trim()).ok())
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .ok_or_else(|| "Base64 内容无效".into())
}
fn component(text: &str) -> Result<String, String> {
    percent_encoding::percent_decode_str(text)
        .decode_utf8()
        .map(|text| text.into_owned())
        .map_err(|error| crate::logging::diagnostic_error("链接文字编码无效".into(), error))
}
pub(super) fn parse_batch(text: &str) -> Result<Vec<serde_yaml_ng::Value>, String> {
    if text.len() > 2 * 1024 * 1024 {
        return Err("导入内容超过 2 MiB".into());
    }
    let text = if text.trim().contains("://") {
        text.to_owned()
    } else {
        decoded(text)?
    };
    let mut values = Vec::new();
    let mut names = BTreeMap::new();
    let mut errors = Vec::new();
    for (index, line) in text
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
    {
        if index >= 10000 {
            return Err("单次最多导入 10000 行".into());
        }
        match parse(line.trim()) {
            Ok(value) => {
                let name = value["name"].as_str().ok_or("代理名称缺失")?;
                match names.get(name) {
                    Some(previous) if previous != &value => errors.push(format!(
                        "第 {} 行：同名代理内容不同，请修改名称：{name}",
                        index + 1
                    )),
                    Some(_) => {}
                    None => {
                        names.insert(name.to_owned(), value.clone());
                        values.push(serde_yaml_ng::to_value(value).map_err(|error| {
                            crate::logging::diagnostic_error("代理转换失败", error)
                        })?);
                    }
                }
            }
            Err(error) => errors.push(format!("第 {} 行：{error}", index + 1)),
        }
    }
    if !errors.is_empty() {
        return Err(errors.join("；"));
    }
    if values.is_empty() {
        return Err("未找到可导入的代理链接".into());
    }
    Ok(values)
}
fn parse(text: &str) -> Result<Value, String> {
    let (scheme, raw) = text.split_once("://").ok_or("代理链接缺少协议")?;
    if scheme == "vmess" {
        return vmess(raw);
    }
    if scheme == "ssr" {
        return ssr(raw);
    }
    let kind = match scheme {
        "ss" => "ss",
        "vless" => "vless",
        "trojan" => "trojan",
        "hy2" | "hysteria2" => "hysteria2",
        "hy" | "hysteria" => "hysteria",
        "tuic" => "tuic",
        "anytls" => "anytls",
        "wg" | "wireguard" => "wireguard",
        "http" | "https" => "http",
        "socks" | "socks5" => "socks5",
        _ => return Err("不支持的代理协议".into()),
    };
    // Parse every protocol with the same maintained URL parser; no hand-written authority parser.
    let url = url::Url::parse(text)
        .map_err(|error| crate::logging::diagnostic_error("代理链接格式无效", error))?;
    let server = url
        .host_str()
        .filter(|host| !host.is_empty())
        .ok_or("代理服务器缺失")?
        .trim_matches(['[', ']']);
    let port = url
        .port()
        .or(match scheme {
            "https" => Some(443),
            "http" => Some(80),
            _ => None,
        })
        .unwrap_or(443);
    if port == 0 {
        return Err("代理端口需在 1–65535 之间".into());
    }
    let name = url
        .fragment()
        .map(component)
        .transpose()?
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| format!("{} {server}:{port}", kind.to_uppercase()));
    let mut map = json!({"name":name,"type":kind,"server":server,"port":port})
        .as_object()
        .cloned()
        .ok_or("代理对象无效")?;
    let user = component(url.username())?;
    let password = url.password().map(component).transpose()?;
    match kind {
        "ss" => {
            let auth = if let Some(password) = password {
                format!("{user}:{password}")
            } else {
                decoded(&user)?
            };
            let (cipher, password) = auth.split_once(':').ok_or("SS 加密方式或密码缺失")?;
            map.insert("cipher".into(), cipher.into());
            map.insert("password".into(), password.into());
        }
        "vless" => {
            if user.is_empty() {
                return Err("VLESS UUID 缺失".into());
            }
            map.insert("uuid".into(), user.into());
        }
        "tuic" => {
            map.insert("uuid".into(), user.into());
            map.insert("password".into(), password.ok_or("TUIC 密码缺失")?.into());
        }
        "wireguard" => {
            map.insert("private-key".into(), user.into());
            map.insert("udp".into(), true.into());
        }
        "http" | "socks5" => {
            if !user.is_empty() {
                map.insert("username".into(), user.into());
                if let Some(password) = password {
                    map.insert("password".into(), password.into());
                }
            }
            if scheme == "https" {
                map.insert("tls".into(), true.into());
            }
        }
        "anytls" => {
            let auth = password.unwrap_or(user);
            if auth.is_empty() {
                return Err("AnyTLS 密码缺失".into());
            }
            map.insert("password".into(), auth.into());
        }
        "hysteria" => {
            if !user.is_empty() {
                map.insert(
                    "auth-str".into(),
                    password
                        .map(|password| format!("{user}:{password}"))
                        .unwrap_or(user)
                        .into(),
                );
            }
        }
        _ => {
            let auth = password
                .map(|password| format!("{user}:{password}"))
                .unwrap_or(user);
            if auth.is_empty() {
                return Err("代理密码缺失".into());
            }
            map.insert("password".into(), auth.into());
        }
    }
    let params = url
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<std::collections::BTreeMap<_, _>>();
    for (key, value) in &params {
        let target = match key.as_str() {
            "sni" | "peer" => {
                if kind == "vless" {
                    "servername"
                } else {
                    "sni"
                }
            }
            "fp" if kind == "trojan" => "fingerprint",
            "fp" => "client-fingerprint",
            "hpkp" | "pinSHA256" => "fingerprint",
            "fastopen" => "tfo",
            "upmbps" => "up",
            "downmbps" => "down",
            "obfsParam" if kind == "hysteria" => "obfs",
            "mport" => "ports",
            "publickey" => "public-key",
            "auth" if kind == "hysteria" => "auth-str",
            other => other,
        };
        match key.as_str() {
            "type" | "security" | "pbk" | "sid" | "host" | "path" | "serviceName" | "plugin"
            | "address" | "headerType" | "mode" => {}
            "ip" if kind == "wireguard" => {}
            "insecure" | "allowInsecure" | "allow-insecure" | "skip-cert-verify" => {
                map.insert("skip-cert-verify".into(), boolean(value)?.into());
            }
            "tls"
            | "udp"
            | "tfo"
            | "fastopen"
            | "fast-open"
            | "disable-sni"
            | "reduce-rtt"
            | "remote-dns-resolve"
            | "disable-mtu-discovery" => {
                map.insert(target.into(), boolean(value)?.into());
            }
            "mtu"
            | "recv-window"
            | "recv-window-conn"
            | "heartbeat-interval"
            | "request-timeout"
            | "max-open-streams"
            | "max-udp-relay-packet-size"
            | "idle-session-check-interval"
            | "idle-session-timeout"
            | "min-idle-session" => {
                map.insert(
                    target.into(),
                    value
                        .parse::<u32>()
                        .map_err(|error| {
                            crate::logging::diagnostic_error(format!("{key}需为非负整数"), error)
                        })?
                        .into(),
                );
            }
            "alpn" | "dns" | "allowed-ips" => {
                map.insert(
                    target.into(),
                    Value::Array(value.split(',').map(|entry| entry.trim().into()).collect()),
                );
            }
            "reserved" => {
                let entries = value
                    .split(',')
                    .map(|entry| entry.trim().parse::<u8>().map(Value::from))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| {
                        crate::logging::diagnostic_error("WireGuard reserved需为3个字节", error)
                    })?;
                if entries.len() != 3 {
                    return Err("WireGuard reserved需为3个字节".into());
                }
                map.insert(target.into(), entries.into());
            }
            "sni"
            | "token"
            | "ip"
            | "ip-version"
            | "hpkp"
            | "pinSHA256"
            | "upmbps"
            | "downmbps"
            | "obfsParam"
            | "ca"
            | "ca-str"
            | "peer"
            | "fp"
            | "fingerprint"
            | "client-fingerprint"
            | "flow"
            | "encryption"
            | "obfs"
            | "obfs-password"
            | "mport"
            | "publickey"
            | "public-key"
            | "pre-shared-key"
            | "dialer-proxy"
            | "congestion-controller"
            | "udp-relay-mode"
            | "auth"
            | "up"
            | "down"
            | "protocol" => {
                map.insert(target.into(), value.clone().into());
            }
            _ => {}
        }
    }
    if let Some(security) = params.get("security") {
        if !matches!(security.as_str(), "none" | "") {
            map.insert("tls".into(), true.into());
        }
        if security == "reality" {
            let mut opts = Map::new();
            for (key, target) in [("pbk", "public-key"), ("sid", "short-id")] {
                if let Some(value) = params.get(key) {
                    opts.insert(target.into(), value.clone().into());
                }
            }
            map.insert("reality-opts".into(), opts.into());
        }
    }
    if kind == "wireguard"
        && let Some(address) = params.get("address").or_else(|| params.get("ip"))
    {
        for address in address.split(',') {
            let ip = address
                .split('/')
                .next()
                .unwrap_or("")
                .parse::<std::net::IpAddr>()
                .map_err(|error| crate::logging::diagnostic_error("WireGuard 地址无效", error))?;
            map.insert(
                if ip.is_ipv4() { "ip" } else { "ipv6" }.into(),
                ip.to_string().into(),
            );
        }
    }
    let network = if params
        .get("headerType")
        .is_some_and(|value| value == "http")
    {
        Some("http")
    } else {
        params.get("type").map(String::as_str)
    };
    transport(&mut map, network, &params)?;
    if kind == "anytls" {
        map.entry("udp").or_insert(true.into());
    }
    if kind == "hysteria" {
        map.entry("protocol").or_insert("udp".into());
    }
    if kind == "vless"
        && params
            .get("encryption")
            .is_some_and(|value| value == "none")
    {
        map.insert("encryption".into(), "".into());
    }
    if kind == "vless"
        && map.get("tls") == Some(&Value::Bool(true))
        && !map.contains_key("servername")
        && let Some(host) = params.get("host")
    {
        map.insert("servername".into(), host.clone().into());
    }
    if kind == "trojan"
        && let Some(encryption) = params.get("encryption")
    {
        let parts = encryption.split(';').collect::<Vec<_>>();
        if parts.len() == 3 {
            map.remove("encryption");
            map.insert(
                "ss-opts".into(),
                json!({"enabled":true,"method":parts[1],"password":parts[2]}),
            );
        }
    }
    if kind == "ss"
        && let Some(plugin) = params.get("plugin")
    {
        let mut parts = plugin.split(';');
        let name = parts.next().unwrap_or("");
        let target = match name {
            "obfs-local" | "simple-obfs" => "obfs",
            "v2ray-plugin" => "v2ray-plugin",
            _ => return Err("不支持的 SS 插件".into()),
        };
        map.insert("plugin".into(), target.into());
        let mut opts = Map::new();
        for part in parts {
            let (key, value) = part.split_once('=').unwrap_or((part, "true"));
            let key = match key {
                "obfs" => "mode",
                "obfs-host" => "host",
                other => other,
            };
            opts.insert(
                key.into(),
                if value == "true" {
                    true.into()
                } else {
                    value.into()
                },
            );
        }
        map.insert("plugin-opts".into(), opts.into());
    }
    Ok(map.into())
}
fn boolean(value: &str) -> Result<bool, String> {
    match value.to_ascii_lowercase().as_str() {
        "" | "1" | "true" => Ok(true),
        "0" | "false" => Ok(false),
        _ => Err("布尔参数无效".into()),
    }
}
fn transport(
    map: &mut Map<String, Value>,
    network: Option<&str>,
    params: &std::collections::BTreeMap<String, String>,
) -> Result<(), String> {
    if let Some(network) = network.filter(|network| !matches!(*network, "tcp" | "none" | "")) {
        let target = match network {
            "websocket" | "httpupgrade" => "ws",
            other => other,
        };
        let mut opts = Map::new();
        match target {
            "ws" => {
                if let Some(path) = params.get("path") {
                    opts.insert("path".into(), path.clone().into());
                }
                if let Some(host) = params.get("host") {
                    opts.insert("headers".into(), json!({"Host":host}));
                }
                if network == "httpupgrade" {
                    opts.insert("v2ray-http-upgrade".into(), true.into());
                    opts.insert("v2ray-http-upgrade-fast-open".into(), true.into());
                }
            }
            "grpc" => {
                if let Some(path) = params.get("serviceName").or_else(|| params.get("path")) {
                    opts.insert("grpc-service-name".into(), path.clone().into());
                }
            }
            "h2" => {
                if let Some(path) = params.get("path") {
                    opts.insert("path".into(), path.clone().into());
                }
                if let Some(host) = params.get("host") {
                    opts.insert("host".into(), json!([host]));
                }
            }
            "http" => {
                if let Some(path) = params.get("path") {
                    opts.insert("path".into(), json!([path]));
                }
                if let Some(host) = params.get("host") {
                    opts.insert("headers".into(), json!({"Host":[host]}));
                }
            }
            "xhttp" => {
                for key in ["host", "path", "mode"] {
                    if let Some(value) = params.get(key) {
                        opts.insert(key.into(), value.clone().into());
                    }
                }
            }
            _ => return Err("不支持的传输协议".into()),
        }
        map.insert("network".into(), target.into());
        if !opts.is_empty() {
            map.insert(format!("{target}-opts"), opts.into());
        }
    }
    Ok(())
}
fn vmess(raw: &str) -> Result<Value, String> {
    let text = decoded(raw)?;
    let object: Value = serde_json::from_str(&text)
        .map_err(|error| crate::logging::diagnostic_error("VMess 分享内容需为JSON对象", error))?;
    let text = |key| {
        object
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned()
    };
    let server = text("add");
    let uuid = text("id");
    if server.is_empty() || uuid.is_empty() {
        return Err("VMess 服务器或UUID缺失".into());
    }
    let port = object
        .get("port")
        .and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok()))
        .and_then(|port| u16::try_from(port).ok())
        .filter(|port| *port > 0)
        .ok_or("VMess 端口无效")?;
    let mut map=json!({"name":if text("ps").is_empty(){format!("VMess {server}:{port}")}else{text("ps")},"type":"vmess","server":server,"port":port,"uuid":uuid,"cipher":if text("scy").is_empty(){"auto".into()}else{text("scy")},"alterId":object.get("aid").and_then(|value|value.as_u64().or_else(||value.as_str()?.parse().ok())).unwrap_or(0)}).as_object().cloned().ok_or("VMess 对象无效")?;
    if text("tls") == "tls" {
        map.insert("tls".into(), true.into());
    }
    if !text("sni").is_empty() {
        map.insert("servername".into(), text("sni").into());
    }
    let params = [("path".into(), text("path")), ("host".into(), text("host"))]
        .into_iter()
        .collect();
    transport(&mut map, Some(&text("net")), &params)?;
    Ok(map.into())
}
fn ssr(raw: &str) -> Result<Value, String> {
    let text = decoded(raw)?;
    let (main, query) = text.split_once("/?").unwrap_or((&text, ""));
    let parts = main.rsplitn(6, ':').collect::<Vec<_>>();
    if parts.len() != 6 {
        return Err("SSR 分享格式无效".into());
    }
    let port = parts[4]
        .parse::<u16>()
        .ok()
        .filter(|port| *port > 0)
        .ok_or("SSR 端口无效")?;
    let mut map=json!({"name":parts[5],"type":"ssr","server":parts[5].trim_matches(['[',']']),"port":port,"protocol":parts[3],"cipher":parts[2],"obfs":parts[1],"password":decoded(parts[0])?}).as_object().cloned().ok_or("SSR 对象无效")?;
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        let target = match key.as_ref() {
            "remarks" => "name",
            "protoparam" => "protocol-param",
            "obfsparam" => "obfs-param",
            _ => continue,
        };
        map.insert(target.into(), decoded(&value)?.into());
    }
    Ok(map.into())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn batch_is_atomic_named_deduplicated_and_preserves_credentials() {
        let batch=parse_batch("trojan://%20secret%20@example.test:443#one\nvless://uuid@example.test:8443?security=reality&pbk=public&sid=abcd&type=grpc&serviceName=test#two\ntrojan://%20secret%20@example.test:443#one").unwrap();
        assert_eq!(batch.len(), 2);
        assert!(
            parse_batch(
                "trojan://first@example.test:443#same\ntrojan://second@example.test:443#same"
            )
            .unwrap_err()
            .contains("同名代理内容不同")
        );
        assert_eq!(batch[0]["password"], " secret ");
        assert_eq!(batch[1]["reality-opts"]["public-key"], "public");
        assert_eq!(batch[1]["grpc-opts"]["grpc-service-name"], "test");
        let error = parse_batch("trojan://password@example.test:443#ok\ninvalid").unwrap_err();
        assert!(error.contains("第 2 行"));
        assert!(!error.contains("password"));
        assert_eq!(
            parse_batch(&STANDARD.encode("socks5://user:pass@example.test:1080#one")).unwrap()[0]["password"],
            "pass"
        );
    }
    #[test]
    fn standard_protocols_keep_numeric_and_transport_fields() {
        let encoded=STANDARD.encode(r#"{"add":"example.test","port":"443","id":"uuid","ps":"vm","net":"ws","host":"cdn.test","path":"/ws","tls":"tls"}"#);
        let values=parse_batch(&format!("vmess://{encoded}\nss://{}@example.test:8388#ss\nwg://key@example.test:443?publickey=pub&address=10.0.0.1/32&reserved=1,2,3#wg\nhy2://auth@example.test:443?insecure=1#hy",STANDARD_NO_PAD.encode("aes-128-gcm:secret"))).unwrap();
        assert_eq!(values[0]["ws-opts"]["headers"]["Host"], "cdn.test");
        assert_eq!(values[1]["cipher"], "aes-128-gcm");
        assert_eq!(values[2]["reserved"][2], 3);
        assert_eq!(values[3]["skip-cert-verify"], true);
    }
}
