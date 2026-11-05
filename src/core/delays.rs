//! A separate measurement lane. Lifecycle commands never wait for latency batches.
use super::monitor::Target;
use crate::{
    proxies::runtime::{MeasurementError, Test},
    storage::AppPaths,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use tokio::{
    sync::{Semaphore, watch},
    task::JoinSet,
};
type Key = (String, String);
type Measurements = BTreeMap<Key, Result<u32, MeasurementError>>;
#[derive(Clone, Default)]
pub(super) struct State {
    pub(super) generation: u64,
    pub(super) epoch: u64,
    pub(super) results: Measurements,
    pub(super) testing: BTreeSet<Key>,
    pub(super) batches: BTreeSet<Key>,
    pub(super) error: Option<(u64, String)>,
    pub(super) completed: std::collections::VecDeque<(u64, usize, usize)>,
}
pub(super) struct Request {
    pub(super) id: u64,
    pub(super) test: Test,
}
struct Measurement {
    group: String,
    node: String,
    key: String,
    provider: Option<String>,
    url: String,
}
pub(super) struct Plan {
    requests: Vec<Measurement>,
    timeout: u64,
    concurrency: usize,
}
pub(super) async fn prepare(
    paths: &AppPaths,
    target: &Target,
    test: &Test,
) -> Result<Plan, String> {
    if (test.generation, test.epoch) != (target.generation, target.epoch) {
        return Err("内核配置已变更，请重新测速".into());
    }
    let proxies = target.client.proxies(&target.order).await?;
    let repository = crate::storage::Repository::open(paths.clone())
        .map_err(|error| crate::logging::diagnostic_error("无法读取测速设置", error))?;
    let settings =
        crate::preferences::values(&repository, crate::preferences::SettingsKind::General)
            .map_err(|error| crate::logging::diagnostic_error("无法读取测速设置", error))?;
    let default = settings
        .get("DelayTestUrl")
        .map(String::as_str)
        .unwrap_or("https://www.gstatic.com/generate_204");
    let timeout = settings["DelayTestTimeout"]
        .parse::<u64>()
        .map_err(|error| {
            crate::logging::diagnostic_error("测速超时需在 100–60000 毫秒之间", error)
        })?;
    if !(100..=60000).contains(&timeout) {
        return Err("测速超时需在 100–60000 毫秒之间".into());
    }
    let concurrency = settings["DelayTestConcurrency"]
        .parse::<usize>()
        .map_err(|error| crate::logging::diagnostic_error("测速并发数需在 1–10 之间", error))?;
    if !(1..=10).contains(&concurrency) {
        return Err("测速并发数需在 1–10 之间".into());
    }
    let preferences = crate::proxies::preferences::Preferences::load(paths)?
        .profile(crate::configuration::selected_profile(paths)?);
    let groups = if let Some(group) = &test.group {
        if !proxies
            .entries
            .get(group)
            .is_some_and(|proxy| proxy.all.is_some())
        {
            return Err("测速代理组不存在".into());
        }
        vec![group.clone()]
    } else {
        proxies.order.clone()
    };
    let mut requests = Vec::new();
    let mut identities = BTreeSet::new();
    for group in groups {
        if proxies.entries[&group].hidden {
            continue;
        }
        let url = test
            .urls
            .get(&group)
            .or_else(|| preferences.test_urls.get(&group))
            .map(String::as_str)
            .filter(|url| !url.trim().is_empty())
            .unwrap_or_else(|| proxies.test_url(&group, default))
            .to_owned();
        if !crate::profiles::model::valid_remote_url(&url) {
            return Err("代理组测速地址无效".into());
        }
        for member in proxies.members.get(&group).into_iter().flatten() {
            let Some(proxy) = proxies.member_proxy(member) else {
                continue;
            };
            let identity = format!(
                "{}:{}",
                member.record_id.as_deref().unwrap_or(&member.name),
                member.occurrence
            );
            if test
                .node
                .as_ref()
                .is_some_and(|node| node != &member.name && node != &identity)
                || !proxies.testable(&group, &member.name)
            {
                continue;
            }
            let key = proxies.measurement_key(&group, member);
            if !identities.insert((group.clone(), key.clone())) {
                continue;
            }
            requests.push(Measurement {
                group: group.clone(),
                node: member.name.clone(),
                key,
                provider: proxy.provider.clone(),
                url: url.clone(),
            });
            if requests.len() > 20000 {
                return Err("单次测速节点超过上限".into());
            }
        }
    }
    if requests.is_empty() {
        return Err("没有可测速的节点".into());
    }
    Ok(Plan {
        requests,
        timeout,
        concurrency,
    })
}
struct Batch {
    scope: (u64, u64),
    request_id: u64,
    keys: Vec<Key>,
    results: Measurements,
}
struct Progress {
    scope: (u64, u64),
    request_id: u64,
    key: Key,
    result: Result<u32, MeasurementError>,
}
async fn measure(
    target: Target,
    plan: Plan,
    limit: Arc<Semaphore>,
    request_id: u64,
    progress: async_channel::Sender<Progress>,
) -> Measurements {
    let expected = plan
        .requests
        .iter()
        .map(|request| (request.group.clone(), request.key.clone()))
        .collect::<Vec<_>>();
    let mut pending = plan.requests.into_iter();
    let mut tasks = JoinSet::new();
    let mut results = BTreeMap::new();
    loop {
        while tasks.len() < plan.concurrency {
            let Some(Measurement {
                group,
                node,
                key,
                provider,
                url,
            }) = pending.next()
            else {
                break;
            };
            let client = target.client.clone();
            let limit = limit.clone();
            let timeout = plan.timeout;
            tasks.spawn(async move {
                let result = match limit.acquire_owned().await {
                    Ok(_permit) => {
                        client
                            .node_delay(&node, provider.as_deref(), &url, timeout)
                            .await
                    }
                    Err(error) => Err(crate::logging::diagnostic_error("测速已取消".into(), error)),
                };
                ((group, key), result)
            });
        }
        if tasks.is_empty() {
            break;
        }
        if let Some(Ok((key, result))) = tasks.join_next().await {
            if progress
                .send(Progress {
                    scope: (target.generation, target.epoch),
                    request_id,
                    key: key.clone(),
                    result: result.clone(),
                })
                .await
                .is_err()
            {
                return results;
            }
            results.insert(key, result);
        }
    }
    for key in expected {
        results
            .entry(key)
            .or_insert_with(|| Err("测速任务异常结束".into()));
    }
    results
}
pub(super) async fn run(
    paths: AppPaths,
    mut targets: watch::Receiver<Option<Target>>,
    requests: async_channel::Receiver<Request>,
    updates: watch::Sender<State>,
    mut shutdown: watch::Receiver<bool>,
) {
    let limit = Arc::new(Semaphore::new(10));
    let mut tasks: JoinSet<Batch> = JoinSet::new();
    let (progress, results) = async_channel::bounded::<Progress>(64);
    let mut reserved = BTreeMap::<Key, u64>::new();
    let mut state = State::default();
    let mut sequence = 0u64;
    let mut measured_at = BTreeMap::<Key, std::time::Instant>::new();
    let mut expiry = tokio::time::interval(std::time::Duration::from_secs(30));
    expiry.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut automatic = tokio::time::interval(std::time::Duration::from_secs(5));
    automatic.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last_auto: Option<std::time::Instant> = None;
    let mut automatic_request = None;
    loop {
        if *shutdown.borrow() {
            break;
        }
        let target = targets.borrow_and_update().clone();
        let scope = target
            .as_ref()
            .map(|target| (target.generation, target.epoch))
            .unwrap_or_default();
        if (state.generation, state.epoch) != scope {
            tasks.abort_all();
            while tasks.join_next().await.is_some() {}
            last_auto = None;
            automatic_request = None;
            measured_at.clear();
            reserved.clear();
            state = State {
                generation: scope.0,
                epoch: scope.1,
                ..Default::default()
            };
            updates.send_replace(state.clone());
        }
        tokio::select! {biased;
            _=shutdown.changed()=>break,
            _=expiry.tick()=>{
                measured_at.retain(|key,time| {let keep=time.elapsed()<std::time::Duration::from_secs(1800);if !keep{state.results.remove(key);}keep});
                updates.send_replace(state.clone());
            },
            _=automatic.tick()=> {
                let Some(current) = &target else { continue; };
                let settings = crate::storage::Repository::open(paths.clone()).and_then(|repository| crate::preferences::values(&repository, crate::preferences::SettingsKind::General));
                let settings = match settings { Ok(settings) => settings, Err(error) => { state.error=Some((u64::MAX,error.to_string())); updates.send_replace(state.clone()); continue; } };
                if settings.get("EnableAutoDelay").is_none_or(|value| value != "true") { last_auto = None; continue; }
                let interval = settings.get("AutoDelayInterval").and_then(|value| value.parse::<u64>().ok()).unwrap_or(5).max(1);
                if last_auto.is_some_and(|last| last.elapsed() < std::time::Duration::from_secs(interval * 60)) { continue; }
                last_auto = Some(std::time::Instant::now());
                let observed = tokio::select! { biased; _=shutdown.changed()=>break, changed=targets.changed()=>{if changed.is_err(){break;}continue;}, result=async {
                    let config = current.client.json("/configs").await?;
                    let mode = match config.get("mode").and_then(serde_json::Value::as_str) { Some("global") => crate::session::ProxyMode::Global, Some("direct") => crate::session::ProxyMode::Direct, _ => crate::session::ProxyMode::Rule };
                    let proxies = current.client.proxies(&current.order).await?;
                    let profile = crate::proxies::preferences::Preferences::load(&paths)?.profile(crate::configuration::selected_profile(&paths)?);
                    let test = profile.group(&proxies, mode).and_then(|group| {
                        let node = proxies.selected(group)?;
                        proxies.testable(group, node).then(|| Test { urls: profile.test_urls.clone(), group: Some(group.into()), node: Some(node.into()), generation: current.generation, epoch: current.epoch })
                    });
                    Ok::<_, String>(test)
                }=>result };
                match observed { Ok(Some(test)) => automatic_request = Some(Request { id: u64::MAX, test }), Ok(None) => {}, Err(error) => { state.error=Some((u64::MAX,error)); updates.send_replace(state.clone()); } }
            },
            changed=targets.changed()=>{if changed.is_err(){break;}},
            update=results.recv()=> {
                let Ok(update)=update else {break;};
                if update.scope!=(state.generation,state.epoch) || reserved.get(&update.key)!=Some(&update.request_id) {continue;}
                state.testing.remove(&update.key);
                measured_at.insert(update.key.clone(),std::time::Instant::now());
                state.results.insert(update.key,update.result);
                updates.send_replace(state.clone());
            },
            result=tasks.join_next(),if !tasks.is_empty()=> {
                if let Some(Ok(Batch{scope,request_id,keys,results}))=result {
                    if scope!=(state.generation,state.epoch){continue;}
                    for key in keys {if reserved.get(&key)==Some(&request_id){reserved.remove(&key);state.testing.remove(&key);state.batches.remove(&key);}}
                    let failures=results.values().filter(|result|!matches!(result,Ok(delay) if *delay>0)).count();
                    sequence=sequence.saturating_add(1);
                    state.completed.push_back((sequence,results.len(),failures));
                    while state.completed.len()>64 {state.completed.pop_front();}
                    if state.results.len().saturating_add(results.len())>100000 {state.results.clear();measured_at.clear();}
                    for key in results.keys(){measured_at.insert(key.clone(),std::time::Instant::now());}
                    state.results.extend(results);updates.send_replace(state.clone());
                } else {
                    if let Some(Err(error)) = result { log::error!("测速任务异常退出：{error}"); }
                    tasks.abort_all();reserved.clear();state.testing.clear();state.batches.clear();state.error=Some((0,"测速任务异常结束".into()));updates.send_replace(state.clone());
                }
            },
            request=async { if let Some(request) = automatic_request.take() { Ok(request) } else { requests.recv().await } },if tasks.len()<32=>{
                let Ok(request)=request else{break;};
                log::info!("测速批次开始；请求={}；内核代次={}；配置修订={}", request.id, scope.0, scope.1);
                let result=if let Some(target)=&target {
                    tokio::select! {biased;_=shutdown.changed()=>break,changed=targets.changed()=>{if changed.is_err(){break;}continue;},result=prepare(&paths,target,&request.test)=>result}
                }else{Err("内核尚未就绪".into())};
                match result {
                    Ok(mut plan)=>{
                        plan.requests.retain(|request|!reserved.contains_key(&(request.group.clone(),request.key.clone())));
                        if plan.requests.is_empty(){log::info!("测速批次合并到现有任务；请求={}", request.id);continue;}
                        let keys=plan.requests.iter().map(|request|(request.group.clone(),request.key.clone())).collect::<Vec<_>>();
                        state.testing.extend(keys.iter().cloned());state.batches.extend(keys.iter().cloned());reserved.extend(keys.iter().cloned().map(|key|(key,request.id)));state.error=None;updates.send_replace(state.clone());
                        if let Some(target)=target {let limit=limit.clone();let progress=progress.clone();tasks.spawn(async move {Batch{scope,request_id:request.id,keys,results:measure(target,plan,limit,request.id,progress).await}});}
                    },
                    Err(error)=>{log::error!("测速批次拒绝；请求={}；{error}", request.id);state.error=Some((request.id,error));updates.send_replace(state.clone());}
                }
            }
        }
    }
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn automatic_measurement_respects_interval_and_cancels_old_generation() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::time::Duration;

        let root = tempfile::Builder::new()
            .prefix("suki-auto-delay-")
            .tempdir_in("/tmp")
            .unwrap();
        let paths = AppPaths::isolated(root.path());
        crate::storage::Repository::open(paths.clone()).unwrap();
        let mut settings = crate::preferences::defaults(crate::preferences::SettingsKind::General);
        settings.insert("EnableAutoDelay".into(), "true".into());
        settings.insert("AutoDelayInterval".into(), "1".into());
        settings.insert("DelayTestTimeout".into(), "60000".into());
        crate::preferences::app::write_values(&paths, "general", &settings).unwrap();

        let endpoint = root.path().join("api.sock");
        let listener = tokio::net::UnixListener::bind(&endpoint).unwrap();
        let observations = Arc::new(AtomicUsize::new(0));
        let measurements = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(tokio::sync::Notify::new());
        let cancelled = Arc::new(tokio::sync::Notify::new());
        let calls = observations.clone();
        let probes = measurements.clone();
        let begin = started.clone();
        let closed = cancelled.clone();
        let server = tokio::spawn(async move {
            let mut tasks = JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let (mut socket, _) = accepted.unwrap();
                        let calls = calls.clone();
                        let probes = probes.clone();
                        let begin = begin.clone();
                        let closed = closed.clone();
                        tasks.spawn(async move {
                            let mut bytes = [0; 8192];
                            let n = socket.read(&mut bytes).await.unwrap();
                            let request = String::from_utf8_lossy(&bytes[..n]);
                            let body = if request.starts_with("GET /configs ") {
                                calls.fetch_add(1, Ordering::SeqCst);
                                r#"{"mode":"rule"}"#
                            } else if request.starts_with("GET /proxies ") {
                                r#"{"proxies":{"g":{"name":"g","type":"Selector","all":["node"],"now":"node"},"node":{"name":"node","type":"Socks5"}}}"#
                            } else if request.starts_with("GET /providers/proxies ") {
                                r#"{"providers":{}}"#
                            } else {
                                assert!(request.starts_with("GET /proxies/node/delay?"));
                                if probes.fetch_add(1, Ordering::SeqCst) == 0 {
                                    begin.notify_one();
                                    assert_eq!(socket.read(&mut bytes).await.unwrap(), 0);
                                    closed.notify_one();
                                    return;
                                }
                                r#"{"delay":11}"#
                            };
                            socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
                        });
                    },
                    _ = tasks.join_next(), if !tasks.is_empty() => {},
                }
            }
        });
        let target = Target {
            logs: None,
            client: super::super::client::Client::new(
                super::super::client::Endpoint::Unix(endpoint),
                "test".into(),
            ),
            http: Default::default(),
            generation: 7,
            epoch: 9,
            order: vec!["g".into()],
            proxy_port: 0,
            proxy_auth: None,
            dns_mode: None,
            proxy_error: None,
        };
        let (targets, changes) = watch::channel(Some(target.clone()));
        let (sender, requests) = async_channel::bounded(2);
        let (updates, mut events) = watch::channel(State::default());
        let (shutdown, cancellation) = watch::channel(false);
        let worker = tokio::spawn(run(paths.clone(), changes, requests, updates, cancellation));
        tokio::time::timeout(Duration::from_secs(3), started.notified())
            .await
            .unwrap();
        assert_eq!(observations.load(Ordering::SeqCst), 1);
        // The scheduler polls every five seconds, but the saved interval is one minute.
        tokio::time::sleep(Duration::from_millis(5200)).await;
        assert_eq!(observations.load(Ordering::SeqCst), 1);
        assert_eq!(measurements.load(Ordering::SeqCst), 1);
        targets.send_replace(None);
        tokio::time::timeout(Duration::from_secs(3), cancelled.notified())
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if events.borrow().generation == 0 && events.borrow().testing.is_empty() {
                    break;
                }
                events.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        assert!(events.borrow().results.is_empty());
        assert!(events.borrow().completed.is_empty());

        settings.insert("EnableAutoDelay".into(), "false".into());
        crate::preferences::app::write_values(&paths, "general", &settings).unwrap();
        targets.send_replace(Some(Target {
            generation: 8,
            epoch: 10,
            ..target
        }));
        sender
            .send(Request {
                id: 2,
                test: Test {
                    generation: 8,
                    epoch: 10,
                    group: Some("g".into()),
                    node: Some("node".into()),
                    urls: Default::default(),
                },
            })
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if events.borrow().generation == 8 && !events.borrow().completed.is_empty() {
                    break;
                }
                events.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        let final_state = events.borrow().clone();
        assert_eq!(final_state.epoch, 10);
        assert_eq!(final_state.results[&("g".into(), "node".into())], Ok(11));
        assert!(final_state.testing.is_empty());
        assert!(final_state.batches.is_empty());
        assert_eq!(final_state.completed.len(), 1);
        assert_eq!(measurements.load(Ordering::SeqCst), 2);
        shutdown.send_replace(true);
        tokio::time::timeout(Duration::from_secs(3), worker)
            .await
            .unwrap()
            .unwrap();
        server.abort();
        let _ = server.await;
    }

    #[tokio::test]
    async fn fast_result_is_published_while_slow_node_is_still_pending() {
        let root = tempfile::Builder::new()
            .prefix("suki-delay-")
            .tempdir_in("/tmp")
            .unwrap();
        let paths = AppPaths::isolated(root.path());
        crate::storage::Repository::open(paths.clone()).unwrap();
        let endpoint = root.path().join("api.sock");
        let listener = tokio::net::UnixListener::bind(&endpoint).unwrap();
        let release = Arc::new(tokio::sync::Notify::new());
        let gate = release.clone();
        let server = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    accepted=listener.accept()=> {
                        let (mut stream,_)=accepted.unwrap();let gate=gate.clone();
                        connections.spawn(async move {
                            let mut bytes=[0;8192];let n=stream.read(&mut bytes).await.unwrap();
                            let request=String::from_utf8_lossy(&bytes[..n]);
                            let body=if request.starts_with("GET /proxies ") {r#"{"proxies":{"g":{"name":"g","type":"Selector","all":["fast","fast","slow"],"now":"fast"},"h":{"name":"h","type":"Selector","all":["shared","shared","shared"]},"fast":{"name":"fast","type":"Socks5"},"slow":{"name":"slow","type":"Socks5"}}}"#}
                            else if request.starts_with("GET /providers/proxies ") {r#"{"providers":{"a":{"proxies":[{"name":"shared","type":"Socks5"}]},"b":{"proxies":[{"name":"shared","type":"Socks5"}]}}}"#}
                            else if request.starts_with("GET /proxies/slow/") {gate.notified().await;r#"{"delay":451}"#}
                            else {r#"{"delay":37}"#};
                            stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
                        });
                    },
                    _=connections.join_next(),if !connections.is_empty()=>{}
                }
            }
        });
        let target = Target {
            logs: None,
            client: super::super::client::Client::new(
                super::super::client::Endpoint::Unix(endpoint),
                "test".into(),
            )
            .with_proxy_config(
                &serde_yaml_ng::from_str("proxy-groups:\n- name: h\n  use: [a, b]\n").unwrap(),
            ),
            http: Default::default(),
            generation: 7,
            epoch: 9,
            order: vec!["g".into(), "h".into()],
            proxy_port: 0,
            proxy_auth: None,
            dns_mode: None,
            proxy_error: None,
        };
        let mut preferences = crate::proxies::preferences::Preferences::default();
        preferences
            .update(
                None,
                crate::proxies::preferences::Change::TestUrl {
                    group: "g".into(),
                    url: "https://persisted.test/test".into(),
                },
            )
            .unwrap();
        preferences.save(&paths).unwrap();
        let test = Test {
            urls: Default::default(),
            group: None,
            node: None,
            generation: 7,
            epoch: 9,
        };
        let plan = prepare(&paths, &target, &test).await.unwrap();
        assert_eq!(
            plan.requests.len(),
            4,
            "repeated identical sources share a request"
        );
        assert_eq!(
            plan.requests
                .iter()
                .filter(|request| request.group == "h")
                .map(|request| request.provider.as_deref().unwrap())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["a", "b"])
        );
        assert!(
            plan.requests
                .iter()
                .filter(|request| request.group == "g")
                .all(|request| request.url == "https://persisted.test/test")
        );
        let mut settings = crate::preferences::defaults(crate::preferences::SettingsKind::General);
        settings.insert("DelayTestTimeout".into(), "100".into());
        settings.insert("DelayTestConcurrency".into(), "1".into());
        crate::preferences::app::write_values(&paths, "general", &settings).unwrap();
        let plan = prepare(&paths, &target, &test).await.unwrap();
        assert_eq!((plan.timeout, plan.concurrency), (100, 1));
        settings.insert("DelayTestConcurrency".into(), "11".into());
        crate::preferences::app::write_values(&paths, "general", &settings).unwrap();
        assert!(prepare(&paths, &target, &test).await.is_err());
        crate::preferences::app::write_values(
            &paths,
            "general",
            &crate::preferences::defaults(crate::preferences::SettingsKind::General),
        )
        .unwrap();
        let (_targets, changes) = watch::channel(Some(target));
        let (sender, requests) = async_channel::bounded(2);
        let (updates, mut events) = watch::channel(State::default());
        let (shutdown, cancellation) = watch::channel(false);
        let worker = tokio::spawn(run(paths, changes, requests, updates, cancellation));
        sender
            .send(Request {
                id: 1,
                test: Test {
                    urls: Default::default(),
                    group: Some("g".into()),
                    node: None,
                    generation: 7,
                    epoch: 9,
                },
            })
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                events.changed().await.unwrap();
                assert!(
                    events.borrow().error.is_none(),
                    "{:?}",
                    events.borrow().error
                );
                if events
                    .borrow()
                    .results
                    .contains_key(&("g".into(), "c:4:fast".into()))
                {
                    break;
                }
            }
        })
        .await
        .unwrap();
        let partial = events.borrow().clone();
        assert_eq!(partial.results[&("g".into(), "c:4:fast".into())], Ok(37));
        assert!(!partial.testing.contains(&("g".into(), "c:4:fast".into())));
        assert!(partial.testing.contains(&("g".into(), "slow".into())));
        assert_eq!(partial.batches.len(), 2);
        assert!(partial.completed.is_empty());
        release.notify_one();
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                events.changed().await.unwrap();
                if !events.borrow().completed.is_empty() {
                    break;
                }
            }
        })
        .await
        .unwrap();
        assert!(events.borrow().batches.is_empty());
        assert_eq!(
            events.borrow().results[&("g".into(), "slow".into())],
            Ok(451)
        );
        shutdown.send_replace(true);
        worker.await.unwrap();
        server.abort();
        let _ = server.await;
    }
}
