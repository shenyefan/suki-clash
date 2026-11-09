//! Application-lifetime sampler. One recorder and one bounded foreground feed.
use super::client::Client;
use crate::{
    overview::statistics::{Connections, Recorder, Summary},
    proxies::runtime::Proxies,
    session::ProxyMode,
    storage::AppPaths,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::watch;
#[derive(Clone)]
pub(super) struct Target {
    pub(super) client: Client,
    pub(super) logs: Option<super::log_stream::Source>,
    pub(super) http: crate::network::Http,
    pub(super) generation: u64,
    pub(super) epoch: u64,
    pub(super) order: Vec<String>,
    pub(super) proxy_port: u16,
    pub(super) proxy_auth: Option<String>,
    pub(super) dns_mode: Option<String>,
    pub(super) proxy_error: Option<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct IpInfo {
    pub(crate) ip: String,
    pub(crate) country: String,
    pub(crate) country_code: String,
    pub(crate) region: String,
    pub(crate) city: String,
    pub(crate) timezone: String,
    pub(crate) organization: String,
    pub(crate) asn_organization: String,
    pub(crate) asn: Option<u32>,
    pub(crate) latitude_microdegrees: Option<i32>,
    pub(crate) longitude_microdegrees: Option<i32>,
}
impl IpInfo {
    pub(crate) fn flag(&self) -> String {
        let code = self.country_code.to_ascii_uppercase();
        if code.len() != 2 || !code.bytes().all(|byte| byte.is_ascii_uppercase()) {
            return String::new();
        }
        code.chars()
            .filter_map(|letter| char::from_u32(127397 + letter as u32))
            .collect()
    }
    pub(crate) fn location(&self) -> String {
        [&self.city, &self.region]
            .into_iter()
            .filter(|part| !part.is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join(" · ")
    }
}
/// Mihomo's /memory snapshot reports the core's allocation in bytes.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
pub(crate) struct MemoryUsage {
    pub(crate) inuse: u64,
    pub(crate) oslimit: Option<u64>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Runtime {
    pub(crate) generation: u64,
    pub(crate) epoch: u64,
    pub(crate) connected: bool,
    pub(crate) connections_live: bool,
    pub(crate) traffic_live: bool,
    pub(crate) memory: Option<MemoryUsage>,
    pub(crate) memory_live: bool,
    pub(crate) memory_error: Option<String>,
    pub(crate) connections: Arc<Connections>,
    pub(crate) connections_at: Option<Instant>,
    pub(crate) stream_error: Option<String>,
    pub(crate) icons: super::icons::State,
    pub(crate) delay_error: Option<(u64, String)>,
    pub(crate) delay_logs: std::collections::VecDeque<(u64, usize, usize)>,
    pub(crate) mode: Option<ProxyMode>,
    pub(crate) dns_mode: Option<String>,
    pub(crate) proxy_port: Option<u16>,
    pub(crate) proxies: Proxies,
    pub(crate) statistics: Summary,
    pub(crate) history: crate::overview::history::Snapshot,
    pub(crate) history_error: Option<String>,
    pub(crate) latency: Option<u32>,
    pub(crate) external_ip: Option<String>,
    pub(crate) ip_info: Option<IpInfo>,
    pub(crate) error: Option<String>,
    pub(crate) probe_error: Option<String>,
}
impl Runtime {
    fn apply_memory(&mut self, state: super::streams::State) {
        if (state.generation, state.epoch) != (self.generation, self.epoch) {
            return;
        }
        if let Some(packet) = state.packet {
            if (packet.generation, packet.epoch) != (self.generation, self.epoch) {
                return;
            }
            match serde_json::from_slice::<MemoryUsage>(&packet.bytes) {
                Ok(memory) => {
                    self.memory = Some(memory);
                    self.memory_live = true;
                    self.memory_error = None;
                }
                Err(error) => {
                    if self.memory_error.is_none() {
                        log::warn!("解析内核内存流失败（{error}）");
                    }
                    self.memory_live = false;
                    self.memory_error = Some("内存流响应无效".into());
                }
            }
        } else if let Some(error) = state.error {
            self.memory_live = false;
            self.memory_error = Some(error);
        }
    }
    fn apply_probe(&mut self, result: Probe) {
        if let Some(latency) = result.latency {
            self.latency = Some(latency);
        }
        if let Some(info) = result.ip_info {
            self.external_ip = Some(info.ip.clone());
            self.ip_info = Some(info);
        }
        self.probe_error = result.error;
    }
}
fn send(
    value: &Runtime,
    updates: &async_channel::Sender<Arc<Runtime>>,
    observer: &async_channel::Receiver<Arc<Runtime>>,
) {
    if let Err(async_channel::TrySendError::Full(value)) = updates.try_send(Arc::new(value.clone()))
    {
        let _ = observer.try_recv();
        let _ = updates.try_send(value);
    }
}
#[derive(Clone, Default)]
pub(super) struct ProbeControl {
    pub(super) revision: u64,
    pub(super) proxies: u64,
}
pub(super) async fn run(
    paths: AppPaths,
    mut target: watch::Receiver<Option<Target>>,
    updates: async_channel::Sender<Arc<Runtime>>,
    observer: async_channel::Receiver<Arc<Runtime>>,
    mut shutdown: watch::Receiver<bool>,
    tests: Option<async_channel::Receiver<super::delays::Request>>,
    mut refresh: Option<watch::Receiver<ProbeControl>>,
) {
    let (delay_updates, mut delay_events) = watch::channel(super::delays::State::default());
    let delay_task = tests.map(|requests| {
        tokio::spawn(super::delays::run(
            paths.clone(),
            target.clone(),
            requests,
            delay_updates,
            shutdown.clone(),
        ))
    });
    let (icon_requests, requests) = watch::channel(None);
    let (icons, mut icon_events) = watch::channel(super::icons::State::default());
    let icon_task = tokio::spawn(super::icons::run(
        paths.clone(),
        requests,
        icons,
        shutdown.clone(),
    ));
    let mut history = crate::overview::history::Ledger::open(paths.traffic_record()).await;
    let mut persist = tokio::time::interval(Duration::from_secs(30));
    persist.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut recorder = Recorder::default();
    let mut runtime = Runtime {
        history: history.snapshot(chrono::Local::now()),
        history_error: history.error(),
        ..Default::default()
    };
    let mut connections = super::streams::Feed::start(
        "/connections?interval=1000",
        target.clone(),
        shutdown.clone(),
    );
    let mut traffic = super::streams::Feed::start("/traffic", target.clone(), shutdown.clone());
    let mut memory = super::streams::Feed::start("/memory", target.clone(), shutdown.clone());
    let mut timer = tokio::time::interval(Duration::from_secs(1));
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last_control: Option<Instant> = None;
    let mut control_failures = 0u8;
    let mut probe_requested = true;
    let mut probe_revision = refresh
        .as_ref()
        .map_or(0, |control| control.borrow().revision);
    let mut control_revision = refresh
        .as_ref()
        .map_or(0, |control| control.borrow().proxies);
    let mut probe: Option<tokio::task::JoinHandle<Probe>> = None;
    let mut scope = None;
    let mut flush = tokio::time::interval(Duration::from_millis(200));
    flush.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut dirty = false;
    let mut connection_error = None;
    let mut traffic_error = None;
    send(&runtime, &updates, &observer);
    loop {
        if *shutdown.borrow() {
            break;
        }
        let next_scope = target
            .borrow()
            .as_ref()
            .map(|current| (current.generation, current.epoch));
        if scope != next_scope {
            if let Some(task) = probe.take() {
                task.abort();
                let _ = task.await;
            }
            icon_requests.send_replace(None);
            recorder.set_scope(next_scope);
            history.gap();
            history.set_proxies(&Proxies::default());
            scope = next_scope;
            last_control = None;
            control_failures = 0;
            probe_requested = true;
            connection_error = None;
            traffic_error = None;
            runtime = Runtime {
                latency: runtime.latency,
                external_ip: runtime.external_ip.clone(),
                ip_info: runtime.ip_info.clone(),
                history: history.snapshot(chrono::Local::now()),
                history_error: history.error(),
                statistics: Summary {
                    rankings: recorder.rankings(),
                    ..Default::default()
                },
                ..Default::default()
            };
            if let Some((generation, epoch)) = scope {
                runtime.generation = generation;
                runtime.epoch = epoch;
            }
            send(&runtime, &updates, &observer);
        }
        tokio::select! {
                    _=shutdown.changed()=>break,
                    _=persist.tick()=> {
                        history.flush(chrono::Local::now()).await;
                        runtime.history = history.snapshot(chrono::Local::now());
                        runtime.history_error = history.error();
                        dirty=true;
                        continue;
                    },
                    _=async { match &mut refresh { Some(refresh) => { let _ = refresh.changed().await; }, None => std::future::pending::<()>().await } },if refresh.is_some()=> {
                        if let Some(receiver) = &mut refresh {
                            if receiver.has_changed().is_err() { refresh=None; }
                            else {
                                let request = receiver.borrow_and_update().clone();
                                if probe_revision != request.revision { probe_requested=true; probe_revision=request.revision; }
                                if control_revision != request.proxies { last_control=None; control_failures=0; control_revision=request.proxies; }
                                send(&runtime,&updates,&observer);
                            }
                        }
                        continue;
                    },
                    changed=delay_events.changed(),if delay_task.is_some()=> {
                        if changed.is_err(){break;}
                        let state=delay_events.borrow_and_update().clone();
                        if scope==Some((state.generation,state.epoch)) {runtime.proxies.measurements=state.results;runtime.proxies.testing=state.testing;runtime.proxies.batches=state.batches;runtime.delay_error=state.error;runtime.delay_logs=state.completed;send(&runtime,&updates,&observer);dirty=false;}
                        continue;
                    },
                    changed=icon_events.changed()=> {
                        if changed.is_err(){break;}
                        let state=icon_events.borrow_and_update().clone();
                        if scope==Some((state.generation,state.epoch)) {runtime.icons=state;dirty=true;}
                        continue;
                    },
                    _=flush.tick()=>{if dirty {send(&runtime,&updates,&observer);dirty=false;}continue;},
                    changed=target.changed()=>{if changed.is_err(){break;}continue;},
                    changed=connections.events.changed()=> {
                        if changed.is_err(){break;}
                        let state=connections.events.borrow_and_update().clone();
                        if scope!=Some((state.generation,state.epoch)) {continue;}
                        if let Some(packet)=state.packet {
                            if scope!=Some((packet.generation,packet.epoch)){continue;}
                            match decode_connections(&packet.bytes) {
                                Ok(current)=> {
                                    history.sample(chrono::Local::now(), packet.at, &current, recorder.previous_connections());
                                    runtime.history = history.snapshot(chrono::Local::now());
                                    runtime.statistics.connections=Some(current.connections.len());
                                    runtime.statistics.rankings=recorder.sample_connections(&current);

                                    runtime.connections=Arc::new(current);runtime.connections_at=Some(packet.at);runtime.connections_live=true;connection_error=None;
                                },
                                Err(error)=> {connection_error=Some(error);runtime.connections_live=false;runtime.statistics.connections=None;recorder.connection_gap();history.gap();}
                            }
                        } else if let Some(error)=state.error {
                            runtime.connections_live=false;runtime.statistics.connections=None;connection_error=Some(error);recorder.connection_gap();history.gap();
                        }
                    },
                    changed=traffic.events.changed()=> {
                        if changed.is_err(){break;}
                        let state=traffic.events.borrow_and_update().clone();
                        if scope!=Some((state.generation,state.epoch)){continue;}
                        if let Some(packet)=state.packet {
                            #[derive(serde::Deserialize)] struct Traffic {up:u64,down:u64}
                            match serde_json::from_slice::<Traffic>(&packet.bytes) {
                                Ok(rate)=> {runtime.statistics.upload_rate=Some(rate.up);runtime.statistics.download_rate=Some(rate.down);runtime.statistics.rates=recorder.record_rate(chrono::Local::now(),rate.up,rate.down);runtime.traffic_live=true;traffic_error=None;},
                                Err(error)=>{if traffic_error.is_none(){log::warn!("解析内核流量流失败（{error}）");}traffic_error=Some("流量流响应无效".into());runtime.traffic_live=false;runtime.statistics.upload_rate=None;runtime.statistics.download_rate=None;runtime.statistics.rates.clear();recorder.gap();}
                            }
                        } else if let Some(error)=state.error {recorder.gap();runtime.statistics.rates.clear();runtime.traffic_live=false;runtime.statistics.upload_rate=None;runtime.statistics.download_rate=None;traffic_error=Some(error);}
                    },
                    changed=memory.events.changed()=> {
                        if changed.is_err(){break;}
                        runtime.apply_memory(memory.events.borrow_and_update().clone());
                    },
                    _=timer.tick()=> {
                        let Some(current)=target.borrow().clone() else {continue;};
                        if control_failures < 3 && last_control.is_none_or(|last|last.elapsed()>=Duration::from_secs(5)) {
                            last_control=Some(Instant::now());
                            let observed=tokio::select! {biased;_=shutdown.changed()=>break,changed=target.changed()=>{if changed.is_err(){break;}continue;},result=read_control(&current)=>result};
                            match observed {
                                Ok((mode,dns,mut proxies,port))=>{
                                    control_failures=0;
                                    history.set_proxies(&proxies);
                                    let delays=delay_events.borrow();
                                    if scope==Some((delays.generation,delays.epoch)){proxies.measurements=delays.results.clone();proxies.testing=delays.testing.clone();proxies.batches=delays.batches.clone();}
        let sources=proxies.entries.values().filter(|proxy|!proxy.hidden).map(|proxy|proxy.icon.trim().to_owned()).filter(|source|!source.is_empty() && source.len()<=3*1024*1024).take(128).collect();
                                    let request=Arc::new(super::icons::Request{target:current.clone(),sources});
                                    icon_requests.send_if_modified(|old|{if old.as_ref().is_some_and(|old|old.target.generation==request.target.generation && old.target.epoch==request.target.epoch && old.sources==request.sources){false}else{*old=Some(request);true}});
                                    runtime.connected=true;runtime.mode=Some(mode);runtime.dns_mode=dns.or_else(||current.dns_mode.clone());runtime.error=proxies.provider_error.clone();runtime.proxies=proxies;runtime.proxy_port=port;},
                                Err(error)=>{control_failures+=1;runtime.connected=false;runtime.error=Some(if control_failures>=3 {format!("{error}；已停止自动重试")}else{error});}
                            }
                        }
                        if let Some(task)=probe.as_ref() && task.is_finished() && let Some(task)=probe.take() {
                            match task.await {
                                Ok(result) if scope==Some((result.generation,result.epoch)) => {
                                    if result.error.is_none() { log::debug!("出口 IP 检测完成"); }
                                    runtime.apply_probe(result)
                                },
                                Err(error) => runtime.probe_error=Some(format!("出口 IP 查询任务异常：{error}")),
                                _ => {},
                            }
                        }
                        if !cfg!(test) && runtime.connected && runtime.proxy_port.is_some() && probe.is_none() && probe_requested {
                            probe_requested=false;
                            let mut observed=current;observed.proxy_port=runtime.proxy_port.unwrap_or(0);probe=Some(tokio::spawn(probe_exit(paths.clone(),observed,target.clone(),shutdown.clone())));
                        }
                    }
                }
        let errors = connection_error
            .clone()
            .map(|error| ("连接流", error))
            .into_iter()
            .chain(traffic_error.clone().map(|error| ("流量流", error)))
            .chain(runtime.memory_error.clone().map(|error| ("内存流", error)));
        let error = crate::failure::scoped(errors);
        runtime.stream_error = (!error.is_empty()).then_some(error);
        dirty = true;
    }
    if let Some(task) = probe {
        let _ = task.await;
    }
    if let Some(task) = delay_task {
        let _ = task.await;
    }
    drop(icon_requests);
    let _ = icon_task.await;
    connections.finish().await;
    traffic.finish().await;
    memory.finish().await;
    history.flush(chrono::Local::now()).await;
    if let Some(error) = history.error() {
        log::error!(target: "statistics", "{error}");
    }
    updates.close();
}
fn decode_connections(bytes: &[u8]) -> Result<Connections, String> {
    let connections: Connections = serde_json::from_slice(bytes)
        .map_err(|error| crate::logging::diagnostic_error("连接流响应无效", error))?;
    if connections.connections.len() > 20000 {
        return Err("连接统计超过上限".into());
    }
    let mut ids = std::collections::BTreeSet::new();
    if connections
        .connections
        .iter()
        .any(|connection| connection.id.is_empty() || !ids.insert(&connection.id))
    {
        return Err("连接身份重复或无效".into());
    }
    Ok(connections)
}
async fn read_control(
    target: &Target,
) -> Result<(ProxyMode, Option<String>, Proxies, Option<u16>), String> {
    let (config, proxies) = tokio::try_join!(
        target.client.json("/configs"),
        target.client.proxies(&target.order)
    )?;
    let mode = serde_json::from_value(config.get("mode").cloned().ok_or("内核缺少模式信息")?)
        .map_err(|error| crate::logging::rejected("内核模式无效", error))?;
    let dns = config
        .get("dns")
        .and_then(|dns| dns.get("enhanced-mode"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let port = ["mixed-port", "port"].iter().find_map(|key| {
        config
            .get(*key)
            .and_then(serde_json::Value::as_u64)
            .and_then(|port| u16::try_from(port).ok())
            .filter(|port| *port > 0)
    });
    Ok((mode, dns, proxies, port))
}
#[cfg(all(test, unix))]
pub(super) async fn sample(
    target: &Target,
) -> Result<(Connections, ProxyMode, Option<String>, Proxies, Option<u16>), String> {
    let (raw, (mode, dns, proxies, port)) =
        tokio::try_join!(target.client.json("/connections"), read_control(target))?;
    let bytes = serde_json::to_vec(&raw)
        .map_err(|error| crate::logging::diagnostic_error("连接响应无效", error))?;
    Ok((decode_connections(&bytes)?, mode, dns, proxies, port))
}
struct Probe {
    generation: u64,
    epoch: u64,
    latency: Option<u32>,
    ip: Option<String>,
    ip_info: Option<IpInfo>,
    error: Option<String>,
}
async fn probe_exit(
    paths: AppPaths,
    target: Target,
    changes: watch::Receiver<Option<Target>>,
    shutdown: watch::Receiver<bool>,
) -> Probe {
    probe_services(
        paths,
        target,
        changes,
        shutdown,
        &["https://api.ip.sb/geoip", "https://ipwho.is/"],
    )
    .await
}
async fn probe_services(
    paths: AppPaths,
    target: Target,
    changes: watch::Receiver<Option<Target>>,
    shutdown: watch::Receiver<bool>,
    ip_urls: &[&str],
) -> Probe {
    let mut result = Probe {
        generation: target.generation,
        epoch: target.epoch,
        latency: None,
        ip: None,
        ip_info: None,
        error: None,
    };
    if *shutdown.borrow()
        || changes.borrow().as_ref().is_none_or(|current| {
            current.generation != target.generation || current.epoch != target.epoch
        })
    {
        return result;
    }
    if let Some(error) = &target.proxy_error {
        result.error = Some(error.clone());
        return result;
    }
    let settings = crate::storage::Repository::open(paths.clone()).and_then(|repository| {
        crate::preferences::values(&repository, crate::preferences::SettingsKind::General)
    });
    let settings = match settings {
        Ok(settings) => settings,
        Err(error) => {
            result.error = Some(error.to_string());
            return result;
        }
    };
    let url = settings
        .get("DelayTestUrl")
        .map(String::as_str)
        .unwrap_or("https://www.gstatic.com/generate_204");
    let timeout = match settings["DelayTestTimeout"].parse::<u64>() {
        Ok(value) if (100..=60000).contains(&value) => Duration::from_millis(value),
        _ => {
            result.error = Some("测速超时需在 100–60000 毫秒之间".into());
            return result;
        }
    };
    let latency = probe_http(
        &paths,
        &target,
        url,
        timeout,
        changes.clone(),
        shutdown.clone(),
    );
    let ip = async {
        let mut errors = Vec::new();
        for url in ip_urls {
            match probe_http(
                &paths,
                &target,
                url,
                timeout,
                changes.clone(),
                shutdown.clone(),
            )
            .await
            {
                Ok(Some((bytes, _))) => match parse_ip_info(&bytes) {
                    Ok(info) => return Ok(Some(info)),
                    Err(error) => errors.push(error),
                },
                Ok(None) => return Ok(None),
                Err(error) => errors.push(error),
            }
        }
        Err(crate::failure::join(&errors))
    };
    let (latency, ip) = tokio::join!(latency, ip);
    let mut errors = Vec::new();
    match latency {
        Ok(Some((_, elapsed))) => result.latency = Some(elapsed),
        Ok(None) => {}
        Err(error) => errors.push(("出口请求", error)),
    }
    match ip {
        Ok(ip) => {
            result.ip = ip.as_ref().map(|info| info.ip.clone());
            result.ip_info = ip;
        }
        Err(error) => errors.push(("出口 IP 检测", error)),
    }
    if !errors.is_empty() {
        result.error = Some(crate::failure::scoped(errors));
    }
    result
}
fn parse_ip(bytes: &[u8]) -> Result<String, String> {
    let response: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|error| crate::logging::diagnostic_error("IP 服务响应格式无效", error))?;
    response
        .get("ip")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| value.parse::<std::net::IpAddr>().ok())
        .map(|ip| ip.to_string())
        .ok_or_else(|| "IP 服务未返回有效地址".into())
}
fn parse_ip_info(bytes: &[u8]) -> Result<IpInfo, String> {
    let response: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|error| crate::logging::diagnostic_error("IP 服务响应格式无效", error))?;
    let ip = parse_ip(bytes)?;
    let text = |key: &str| {
        response
            .get(key)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let connection = response.get("connection");
    let nested = |key: &str| {
        connection
            .and_then(|value| value.get(key))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let coordinate = |key: &str, maximum: f64| {
        response
            .get(key)
            .and_then(serde_json::Value::as_f64)
            .filter(|value| value.is_finite() && value.abs() <= maximum)
            .map(|value| (value * 1_000_000.).round() as i32)
    };
    Ok(IpInfo {
        ip,
        country: text("country"),
        country_code: text("country_code"),
        region: text("region"),
        city: text("city"),
        timezone: response
            .get("timezone")
            .and_then(|value| {
                value
                    .as_str()
                    .or_else(|| value.get("id").and_then(serde_json::Value::as_str))
            })
            .unwrap_or_default()
            .to_owned(),
        organization: if connection.is_some() {
            nested("isp")
        } else {
            text("organization")
        },
        asn_organization: if connection.is_some() {
            nested("org")
        } else {
            text("asn_organization")
        },
        asn: response
            .get("asn")
            .or_else(|| connection.and_then(|value| value.get("asn")))
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| value.try_into().ok()),
        latitude_microdegrees: coordinate("latitude", 90.),
        longitude_microdegrees: coordinate("longitude", 180.),
    })
}
async fn probe_http(
    paths: &AppPaths,
    target: &Target,
    url: &str,
    timeout: Duration,
    mut changes: watch::Receiver<Option<Target>>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<Option<(Vec<u8>, u32)>, String> {
    if *shutdown.borrow()
        || changes.borrow().as_ref().is_none_or(|current| {
            current.generation != target.generation || current.epoch != target.epoch
        })
    {
        return Ok(None);
    }
    let transport = super::download::SubscriptionTransport::Proxy {
        url: format!("http://127.0.0.1:{}", target.proxy_port),
        authentication: target.proxy_auth.clone(),
    };
    let started = Instant::now();
    let mut transfer =
        super::download::Transfer::probe(&target.http, paths, url, timeout, &transport)?;
    let outcome = tokio::select! {biased;_=shutdown.changed()=>None,_=changes.changed()=>None,result=transfer.finish(|_,_|{})=>Some(result)};
    outcome.transpose().map(|bytes| {
        bytes.map(|bytes| {
            (
                bytes,
                started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32,
            )
        })
    })
}

#[cfg(all(test, unix))]
mod tests {
    #[test]
    fn ip_metadata_keeps_timezone_and_failed_refresh_keeps_last_success() {
        let info = super::parse_ip_info(
            br#"{"ip":"203.0.113.9","timezone":"Asia/Shanghai","country_code":"CN"}"#,
        )
        .unwrap();
        assert_eq!(info.timezone, "Asia/Shanghai");
        let nested = super::parse_ip_info(
            br#"{"ip":"203.0.113.10","timezone":{"id":"Europe/Paris"},"connection":{"asn":64500}}"#,
        )
        .unwrap();
        assert_eq!(nested.timezone, "Europe/Paris");
        assert_eq!(nested.asn, Some(64500));
        let mut runtime = super::Runtime::default();
        runtime.apply_probe(super::Probe {
            generation: 1,
            epoch: 1,
            latency: Some(23),
            ip: Some(info.ip.clone()),
            ip_info: Some(info.clone()),
            error: None,
        });
        runtime.apply_probe(super::Probe {
            generation: 1,
            epoch: 1,
            latency: None,
            ip: None,
            ip_info: None,
            error: Some("IP 服务返回 HTTP 503".into()),
        });
        assert_eq!(runtime.ip_info, Some(info));
        assert_eq!(runtime.external_ip.as_deref(), Some("203.0.113.9"));
        assert_eq!(runtime.latency, Some(23));
        assert_eq!(runtime.probe_error.as_deref(), Some("IP 服务返回 HTTP 503"));
        runtime.apply_probe(super::Probe {
            generation: 1,
            epoch: 1,
            latency: Some(42),
            ip: Some(nested.ip.clone()),
            ip_info: Some(nested.clone()),
            error: None,
        });
        assert_eq!(runtime.ip_info, Some(nested));
        assert_eq!(runtime.external_ip.as_deref(), Some("203.0.113.10"));
        assert_eq!(runtime.latency, Some(42));
        assert!(runtime.probe_error.is_none());
    }

    use super::*;
    use crate::core::client::Endpoint;
    #[tokio::test]
    async fn changing_target_cancels_sampling_and_shutdown_does_not_wait_for_http_timeout() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        let socket = root.path().join("monitor.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        let (targets, target) = watch::channel(Some(Target {
            logs: None,
            http: crate::network::Http::default(),
            client: Client::new(Endpoint::Unix(socket), String::new()),
            generation: 1,
            epoch: 1,
            order: vec![],
            proxy_port: 0,
            proxy_auth: None,
            dns_mode: None,
            proxy_error: None,
        }));
        let (sender, events) = async_channel::bounded(1);
        let observer = events.clone();
        let (shutdown, cancellation) = watch::channel(false);
        let task = tokio::spawn(run(
            paths,
            target,
            sender,
            observer,
            cancellation,
            None,
            None,
        ));
        let (stream, _) = tokio::time::timeout(Duration::from_secs(1), listener.accept())
            .await
            .unwrap()
            .unwrap();
        targets.send_replace(None);
        tokio::time::timeout(Duration::from_millis(500), async {
            loop {
                let event = events.recv().await.unwrap();
                if event.generation == 0 && !event.connected && event.error.is_none() {
                    break;
                }
            }
        })
        .await
        .unwrap();
        shutdown.send_replace(true);
        tokio::time::timeout(Duration::from_millis(500), task)
            .await
            .unwrap()
            .unwrap();
        drop(stream);
    }
    #[tokio::test]
    async fn exit_probe_accepts_no_content_and_retries_ip_service_through_the_same_proxy() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        let repository = crate::storage::Repository::open(paths.clone()).unwrap();
        let mut settings = crate::preferences::defaults(crate::preferences::SettingsKind::General);
        settings.insert(
            "DelayTestUrl".into(),
            "http://latency.invalid/generate_204".into(),
        );
        repository
            .save(
                &crate::storage::Namespace::parse("settings").unwrap(),
                &settings,
            )
            .unwrap();
        let proxy = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = proxy.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let mut requests = Vec::new();
            for _ in 0..3 {
                let (mut stream, _) = proxy.accept().await.unwrap();
                let mut request = Vec::new();
                let mut buffer = [0u8; 1024];
                while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    let read = stream.read(&mut buffer).await.unwrap();
                    assert!(read > 0);
                    request.extend_from_slice(&buffer[..read]);
                    assert!(request.len() < 8192);
                }
                let request = String::from_utf8(request).unwrap();
                if request.starts_with("GET http://latency.invalid/") {
                    stream
                        .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
                        .await
                        .unwrap();
                } else if request.starts_with("GET http://first.invalid/") {
                    stream.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
                } else {
                    assert!(request.starts_with("GET http://second.invalid/"));
                    let body = br#"{"ip":"203.0.113.42"}"#;
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    stream.write_all(response.as_bytes()).await.unwrap();
                    stream.write_all(body).await.unwrap();
                }
                requests.push(request);
            }
            requests
        });
        let target = Target {
            logs: None,
            http: crate::network::Http::default(),
            client: Client::new(
                Endpoint::Unix(root.path().join("unused.sock")),
                String::new(),
            ),
            generation: 1,
            epoch: 1,
            order: vec![],
            proxy_port: port,
            proxy_auth: None,
            dns_mode: None,
            proxy_error: None,
        };
        let (_targets, changes) = watch::channel(Some(target.clone()));
        let (_shutdown, cancellation) = watch::channel(false);
        let outcome = probe_services(
            paths,
            target,
            changes,
            cancellation,
            &["http://first.invalid/ip", "http://second.invalid/ip"],
        )
        .await;
        assert!(outcome.latency.is_some());
        assert_eq!(outcome.ip.as_deref(), Some("203.0.113.42"));
        assert_eq!(outcome.error, None);
        assert_eq!(server.await.unwrap().len(), 3);
    }
    #[tokio::test]
    async fn shutdown_requested_before_spawn_is_observed() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        let (_targets, target) = watch::channel(None);
        let (updates, events) = async_channel::bounded(1);
        let (_shutdown, cancellation) = watch::channel(true);
        tokio::time::timeout(
            Duration::from_millis(500),
            run(
                paths,
                target,
                updates,
                events.clone(),
                cancellation,
                None,
                None,
            ),
        )
        .await
        .unwrap();
        assert!(events.is_closed());
    }
}

#[cfg(test)]
mod memory_tests {
    use super::*;

    fn packet(generation: u64, epoch: u64, body: &'static [u8]) -> super::super::streams::State {
        super::super::streams::State {
            generation,
            epoch,
            packet: Some(Arc::new(super::super::streams::Packet {
                generation,
                epoch,
                bytes: hyper::body::Bytes::from_static(body),
                at: Instant::now(),
            })),
            error: None,
        }
    }

    #[test]
    fn memory_preserves_last_sample_on_failure_and_rejects_other_runtime_scopes() {
        let mut runtime = Runtime {
            generation: 2,
            epoch: 3,
            ..Default::default()
        };
        runtime.apply_memory(packet(2, 3, br#"{"inuse":4096,"oslimit":8192}"#));
        assert_eq!(
            runtime.memory,
            Some(MemoryUsage {
                inuse: 4096,
                oslimit: Some(8192)
            })
        );
        assert!(runtime.memory_live);
        runtime.apply_memory(packet(1, 3, br#"{"inuse":1}"#));
        runtime.apply_memory(packet(2, 2, br#"{"inuse":1}"#));
        assert_eq!(runtime.memory.as_ref().unwrap().inuse, 4096);
        runtime.apply_memory(packet(2, 3, br#"{"inuse":"invalid"}"#));
        assert!(!runtime.memory_live);
        assert_eq!(runtime.memory.as_ref().unwrap().inuse, 4096);
        assert!(runtime.memory_error.is_some());
        runtime.apply_memory(super::super::streams::State {
            generation: 2,
            epoch: 3,
            packet: None,
            error: Some("连接中断".into()),
        });
        assert_eq!(runtime.memory_error.as_deref(), Some("连接中断"));
        runtime.apply_memory(packet(2, 3, br#"{"inuse":0}"#));
        assert_eq!(
            runtime.memory,
            Some(MemoryUsage {
                inuse: 0,
                oslimit: None
            })
        );
        assert!(runtime.memory_live);
        assert!(runtime.memory_error.is_none());
    }

    #[test]
    fn memory_requires_a_nonnegative_integral_native_measurement() {
        for body in [
            br#"{}"#.as_slice(),
            br#"{"inuse":null}"#,
            br#"{"inuse":-1}"#,
            br#"{"inuse":1.5}"#,
        ] {
            let mut runtime = Runtime {
                generation: 1,
                epoch: 1,
                ..Default::default()
            };
            runtime.apply_memory(packet(1, 1, body));
            assert!(runtime.memory.is_none());
            assert!(!runtime.memory_live);
            assert!(runtime.memory_error.is_some());
        }
    }
}
