//! A separate measurement lane. Lifecycle commands never wait for latency batches.
use super::monitor::Target;
use crate::{proxies::runtime::Test, storage::AppPaths};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use tokio::{
    sync::{Semaphore, watch},
    task::JoinSet,
};
type Key = (String, String);
type Measurements = BTreeMap<Key, Result<u32, String>>;
#[derive(Clone, Default)]
pub(super) struct State {
    pub(super) generation: u64,
    pub(super) epoch: u64,
    pub(super) results: Measurements,
    pub(super) testing: BTreeSet<Key>,
    pub(super) error: Option<(u64, String)>,
    pub(super) completed: std::collections::VecDeque<(u64, usize, usize)>,
}
pub(super) struct Request {
    pub(super) id: u64,
    pub(super) test: Test,
}
pub(super) struct Plan {
    pub(super) requests: Vec<(String, String, Option<String>, String)>,
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
    let repository =
        crate::storage::Repository::open(paths.clone()).map_err(|_| "无法读取测速设置")?;
    let settings =
        crate::preferences::values(&repository, crate::preferences::SettingsKind::General)
            .map_err(|_| "无法读取测速设置")?;
    let default = settings
        .get("DelayTestUrl")
        .map(String::as_str)
        .unwrap_or("https://www.gstatic.com/generate_204");
    let timeout = settings
        .get("DelayTestTimeout")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(5000)
        .clamp(100, 60000);
    let concurrency = settings
        .get("DelayTestConcurrency")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(8)
        .clamp(1, 10);
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
    for group in groups {
        if proxies.entries[&group].hidden {
            continue;
        }
        let url = proxies.test_url(&group, default).to_owned();
        if !crate::profiles::model::valid_remote_url(&url) {
            return Err("代理组测速地址无效".into());
        }
        for name in proxies.entries[&group].all.as_deref().unwrap_or_default() {
            if test.node.as_ref().is_some_and(|node| node != name)
                || !proxies.testable(&group, name)
            {
                continue;
            }
            requests.push((
                group.clone(),
                name.clone(),
                proxies.entries[name].provider.clone(),
                url.clone(),
            ));
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
pub(super) async fn measure(target: Target, plan: Plan, limit: Arc<Semaphore>) -> Measurements {
    let expected = plan
        .requests
        .iter()
        .map(|(group, node, _, _)| (group.clone(), node.clone()))
        .collect::<Vec<_>>();
    let mut pending = plan.requests.into_iter();
    let mut tasks = JoinSet::new();
    let mut results = BTreeMap::new();
    loop {
        while tasks.len() < plan.concurrency {
            let Some((group, node, provider, url)) = pending.next() else {
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
                    Err(_) => Err("测速已取消".into()),
                };
                ((group, node), result)
            });
        }
        if tasks.is_empty() {
            break;
        }
        if let Some(Ok((key, result))) = tasks.join_next().await {
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
    let mut tasks: JoinSet<((u64, u64), Vec<Key>, Measurements)> = JoinSet::new();
    let mut state = State::default();
    let mut sequence = 0u64;
    let mut measured_at = BTreeMap::<Key, std::time::Instant>::new();
    let mut expiry = tokio::time::interval(std::time::Duration::from_secs(30));
    expiry.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
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
            measured_at.clear();
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
            changed=targets.changed()=>{if changed.is_err(){break;}},
            result=tasks.join_next(),if !tasks.is_empty()=> {
                if let Some(Ok((scope,keys,results)))=result {
                    if scope!=(state.generation,state.epoch){continue;}
                    for key in keys {state.testing.remove(&key);}
                    let failures=results.values().filter(|result|!matches!(result,Ok(delay) if *delay>0)).count();
                    sequence=sequence.saturating_add(1);
                    state.completed.push_back((sequence,results.len(),failures));
                    while state.completed.len()>64 {state.completed.pop_front();}
                    if state.results.len().saturating_add(results.len())>100000 {state.results.clear();measured_at.clear();}
                    for key in results.keys(){measured_at.insert(key.clone(),std::time::Instant::now());}
                    state.results.extend(results);updates.send_replace(state.clone());
                } else {
                    tasks.abort_all();state.testing.clear();state.error=Some((0,"测速任务异常结束".into()));updates.send_replace(state.clone());
                }
            },
            request=requests.recv(),if tasks.len()<32=>{
                let Ok(request)=request else{break;};
                let result=if let Some(target)=&target {
                    tokio::select! {biased;_=shutdown.changed()=>break,changed=targets.changed()=>{if changed.is_err(){break;}continue;},result=prepare(&paths,target,&request.test)=>result}
                }else{Err("内核尚未就绪".into())};
                match result {
                    Ok(mut plan)=>{
                        plan.requests.retain(|(group,node,_,_)|!state.testing.contains(&(group.clone(),node.clone())));
                        if plan.requests.is_empty(){continue;}
                        let keys=plan.requests.iter().map(|(group,node,_,_)|(group.clone(),node.clone())).collect::<Vec<_>>();
                        state.testing.extend(keys.iter().cloned());state.error=None;updates.send_replace(state.clone());
                        if let Some(target)=target {let limit=limit.clone();tasks.spawn(async move {(scope,keys,measure(target,plan,limit).await)});}
                    },
                    Err(error)=>{state.error=Some((request.id,error));updates.send_replace(state.clone());}
                }
            }
        }
    }
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
}
