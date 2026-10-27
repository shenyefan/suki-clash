use super::{Phase, Snapshot, launch::Launch};
use crate::storage::AppPaths;
use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::Duration,
};
use tokio::{process::Child, sync::watch};

#[derive(Clone)]
pub(crate) enum Command {
    Profiles(super::ProfileCommand),
    Overrides(super::OverrideChange),
    ScriptLogs(u64),
    CheckDocument {
        kind: crate::profiles::model::DocumentKind,
        content: String,
    },
    Start,
    Stop,
    Restart,
    Activate(u64),
    Settings {
        kind: crate::preferences::SettingsKind,
        patch: crate::storage::Values,
    },
    Tunnel(crate::configuration::tunnels::Change),
    Interfaces,
    ProxyNames,
    Proxy(crate::proxies::runtime::Action),
    Connections(crate::traffic::ConnectionAction),
    Chain(crate::proxies::chain_config::Change),
    Validate,
    FlushFakeIp,
    UpdateGeo,
    UpdateDatabase(crate::configuration::resources::Database),
    Resources,
    Rules,
    UpdateProviders {
        kind: super::ProviderKind,
        name: Option<String>,
        generation: u64,
    },
    CheckUpdate(super::ReleaseSelection),
    Install(super::ReleaseSelection),
    Versions {
        refresh: bool,
    },
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ServiceError {
    #[error("已有内核操作正在执行，请等待完成")]
    Busy,
    #[error("内核执行端已停止")]
    Closed,
    #[error("内核请求编号已耗尽")]
    RequestIdExhausted,
}

struct Reservation(Arc<AtomicBool>);
impl Drop for Reservation {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
struct Request {
    id: u64,
    command: Command,
    _reservation: Reservation,
}

pub(crate) struct Service {
    tests: async_channel::Sender<super::delays::Request>,
    sender: async_channel::Sender<Request>,
    events: async_channel::Receiver<Snapshot>,
    shutdown: watch::Sender<bool>,
    busy: Arc<AtomicBool>,
    next_id: u64,
    worker: Option<JoinHandle<Result<(), String>>>,
    runtime_events: async_channel::Receiver<std::sync::Arc<super::Runtime>>,
}
impl Service {
    #[cfg(test)]
    pub(crate) fn start(paths: AppPaths) -> io::Result<Self> {
        Self::start_with_logs(paths, crate::logging::Feed::default())
    }
    pub(crate) fn start_with_logs(paths: AppPaths, logs: crate::logging::Feed) -> io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let (sender, receiver) = async_channel::bounded(1);
        let (tests, test_requests) = async_channel::bounded(32);
        let (updates, events) = async_channel::bounded(1);
        let (shutdown, cancellation) = watch::channel(false);
        let worker_shutdown = shutdown.clone();
        let observer = events.clone();
        let (monitor_target, monitor_receiver) = watch::channel(None);
        let (runtime_updates, runtime_events) = async_channel::bounded(1);
        let runtime_observer = runtime_events.clone();
        let worker = std::thread::Builder::new()
            .name("suki-core".into())
            .spawn(move || {
                let mut actor = Actor::new(paths.clone(), updates, observer);
                actor.logs = logs;
                actor.monitor = monitor_target;
                let terminal_updates = actor.updates.clone();
                let terminal_observer = actor.observer.clone();
                runtime.block_on(async move {
                    let task = tokio::spawn(super::monitor::run(
                        paths,
                        monitor_receiver,
                        runtime_updates,
                        runtime_observer,
                        cancellation.clone(),
                        Some(test_requests),
                    ));
                    let result = actor.run(receiver, cancellation).await;
                    if let Err(error) = &result {
                        Actor::send_snapshot(
                            &Snapshot {
                                phase: Phase::Failed,
                                revision: 1,
                                error: Some(error.clone()),
                                ..Snapshot::default()
                            },
                            &terminal_updates,
                            &terminal_observer,
                        );
                    }
                    let _ = worker_shutdown.send(true);
                    task.await.map_err(|_| "实时采集任务异常退出")?;
                    result
                })
            })?;
        Ok(Self {
            tests,
            sender,
            events,
            shutdown,
            busy: Arc::new(AtomicBool::new(false)),
            next_id: 0,
            worker: Some(worker),
            runtime_events,
        })
    }

    pub(crate) fn test_delay(
        &mut self,
        test: crate::proxies::runtime::Test,
    ) -> Result<u64, String> {
        self.next_id = self.next_id.checked_add(1).ok_or("测速请求编号已耗尽")?;
        self.tests
            .try_send(super::delays::Request {
                id: self.next_id,
                test,
            })
            .map_err(|_| "测速队列不可用")?;
        Ok(self.next_id)
    }
    pub(crate) fn runtime_events(&self) -> async_channel::Receiver<std::sync::Arc<super::Runtime>> {
        self.runtime_events.clone()
    }
    pub(crate) fn events(&self) -> async_channel::Receiver<Snapshot> {
        self.events.clone()
    }

    pub(crate) fn submit(&mut self, command: Command) -> Result<u64, ServiceError> {
        if self.sender.is_closed() {
            return Err(ServiceError::Closed);
        }
        if self
            .busy
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return Err(ServiceError::Busy);
        }
        let reservation = Reservation(self.busy.clone());
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or(ServiceError::RequestIdExhausted)?;
        self.sender
            .try_send(Request {
                id: self.next_id,
                command,
                _reservation: reservation,
            })
            .map_err(|_| ServiceError::Closed)?;
        Ok(self.next_id)
    }
    pub(crate) fn shutdown(&mut self) -> Option<JoinHandle<Result<(), String>>> {
        // Close foreground delivery while its executor is still alive. Cleanup
        // returns through the join result and cannot wake a destroyed UI task.
        self.runtime_events.close();
        self.events.close();
        let _ = self.shutdown.send(true);
        self.sender.close();
        self.worker.take()
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        self.runtime_events.close();
        self.events.close();
        let _ = self.shutdown.send(true);
        self.sender.close();
    }
}

pub(super) struct Running {
    child: Child,
    pub(super) launch: Launch,
    output: Option<tokio::task::JoinHandle<Result<(), String>>>,
    pub(super) log_settings: Option<watch::Sender<super::output::LogSettings>>,
    _lifetime: Option<super::ownership::Lifetime>,
}
pub(super) struct Actor {
    logs: crate::logging::Feed,
    pub(super) paths: AppPaths,
    pub(super) snapshot: Snapshot,
    pub(super) running: Option<Running>,
    validation: Option<Running>,
    pub(super) updates: async_channel::Sender<Snapshot>,
    pub(super) observer: async_channel::Receiver<Snapshot>,
    pub(super) transfer: Option<super::download::Transfer>,
    pub(super) http: crate::network::Http,
    staging: Option<tempfile::TempDir>,
    recovery: Option<super::install::Recovery>,
    upgrade_previous: Option<Launch>,
    versions_cached: Option<std::time::Instant>,
    geo_unknown: bool,
    resources_requested: bool,
    resources_refreshed: std::time::Instant,
    pub(super) override_attempts: std::collections::BTreeMap<u64, i64>,
    pub(super) profile_attempts: std::collections::BTreeMap<u64, i64>,
    pub(super) profile_schedule_started: i64,
    pub(super) monitor: watch::Sender<Option<super::monitor::Target>>,
    pub(super) runtime_warning: Option<String>,
    pub(super) deleted_overrides: std::collections::BTreeMap<u64, super::overrides::Deleted>,
    pub(super) deleted_profiles: std::collections::BTreeMap<u64, super::profiles::DeletedProfile>,
}
impl Actor {
    fn new(
        paths: AppPaths,
        updates: async_channel::Sender<Snapshot>,
        observer: async_channel::Receiver<Snapshot>,
    ) -> Self {
        let preferences = super::Preferences::load(&paths).ok();
        Self {
            logs: crate::logging::Feed::default(),
            paths,
            snapshot: Snapshot {
                preferences,
                ..Snapshot::default()
            },
            running: None,
            validation: None,
            updates,
            observer,
            transfer: None,
            http: crate::network::Http::default(),
            staging: None,
            recovery: None,
            upgrade_previous: None,
            versions_cached: None,
            geo_unknown: false,
            resources_requested: false,
            resources_refreshed: std::time::Instant::now(),
            profile_attempts: Default::default(),
            override_attempts: Default::default(),
            profile_schedule_started: chrono::Utc::now().timestamp(),
            deleted_profiles: Default::default(),
            deleted_overrides: Default::default(),
            monitor: watch::channel(None).0,
            runtime_warning: None,
        }
    }
    pub(super) fn publish(&mut self, phase: Phase, error: Option<String>) {
        if let Ok(chain) = crate::proxies::chain_config::Config::load(&self.paths) {
            self.snapshot.chain = chain;
        }
        if let Ok(repository) = crate::storage::Repository::open(self.paths.clone())
            && let Ok(values) =
                crate::preferences::values(&repository, crate::preferences::SettingsKind::Dns)
        {
            self.snapshot.settings.insert("dns".into(), values);
        }
        self.snapshot.revision = self.snapshot.revision.saturating_add(1);
        self.snapshot.phase = phase;
        self.snapshot.error = error;
        self.snapshot.resources.refresh_files(&self.paths);
        let mut notices = Vec::new();
        if let Some(notice) = crate::bootstrap::geodata_notice(&self.paths) {
            notices.push(notice);
        }
        self.snapshot.notice = (!notices.is_empty()).then(|| notices.join("\n"));
        self.snapshot.pid = self.running.as_ref().and_then(|running| running.child.id());
        self.snapshot.profile = if phase == Phase::Ready {
            self.running
                .as_ref()
                .and_then(|running| running.launch.source.profile())
                .cloned()
        } else {
            None
        };
        if phase != Phase::Ready {
            self.snapshot.version = None;
        }
        if let Some(warning) = &self.runtime_warning {
            notices.push(warning.clone());
            self.snapshot.notice = Some(notices.join("\n"));
        }
        self.sync_monitor();
        Self::send_snapshot(&self.snapshot, &self.updates, &self.observer);
    }

    pub(super) fn send_snapshot(
        snapshot: &Snapshot,
        updates: &async_channel::Sender<Snapshot>,
        observer: &async_channel::Receiver<Snapshot>,
    ) {
        // Coalesce progress and lifecycle updates on the same bounded stream.
        if let Err(async_channel::TrySendError::Full(snapshot)) = updates.try_send(snapshot.clone())
        {
            let _ = observer.try_recv();
            let _ = updates.try_send(snapshot);
        }
    }

    pub(super) fn progress(&mut self, label: &str) {
        self.snapshot.progress = Some(super::TransferProgress::pending(label));
        self.publish(self.snapshot.phase, None);
    }

    async fn run(
        mut self,
        receiver: async_channel::Receiver<Request>,
        mut shutdown: watch::Receiver<bool>,
    ) -> Result<(), String> {
        let ownership = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(
                self.paths
                    .directory(crate::storage::DirectoryKind::Core)
                    .join(".owner.lock"),
            )
            .map_err(|_| "无法建立本应用内核目录所有权")?;
        if ownership.try_lock().is_err() {
            self.publish(
                Phase::Failed,
                Some("另一个 Suki Clash 实例正在管理此用户目录；请先退出另一个实例".into()),
            );
            self.updates.close();
            return Ok(());
        }
        let _ownership = ownership;
        let mut polling = tokio::time::interval(Duration::from_millis(250));
        let mut health = tokio::time::interval(Duration::from_secs(2));
        let mut subscription_timer = tokio::time::interval(Duration::from_secs(30));
        if let Err(error) = self.refresh_profiles() {
            self.publish(Phase::Failed, Some(error));
        }
        self.publish(Phase::Preparing, None);
        let prepared = super::install::recover_pending(&self.paths)
            .and_then(|()| {
                let repository = crate::storage::Repository::open(self.paths.clone())
                    .map_err(|error| error.to_string())?;
                crate::preferences::initialize(&repository).map_err(|error| error.to_string())
            })
            .and_then(|()| {
                let preferences = super::Preferences::load(&self.paths)?;
                let [native, _] = super::Preferences::paths(&self.paths);
                if !native.exists() {
                    preferences.save(&self.paths)?;
                }
                self.snapshot.preferences = Some(preferences);
                Ok(())
            })
            .and_then(|()| crate::bootstrap::bundle_directory())
            .and_then(|bundle| {
                crate::bootstrap::prepare(&self.paths, &bundle, || *shutdown.borrow())
            });
        match prepared {
            Ok(()) => {
                let started = tokio::select! {
                    biased;
                    _ = shutdown.changed() => Err("应用退出，启动已取消".to_owned()),
                    result = self.startup() => result,
                };
                if let Err(error) = started {
                    self.publish(Phase::Failed, Some(error));
                }
            }
            Err(error) => self.publish(Phase::Failed, Some(error)),
        }
        self.snapshot.progress = None;
        self.publish(self.snapshot.phase, self.snapshot.error.clone());
        loop {
            if *shutdown.borrow() {
                break;
            }
            tokio::select! {
                biased;
                _ = shutdown.changed() => break,
                request = receiver.recv() => {
                    let Ok(request) = request else { break; };
                    self.snapshot.request_id = Some(request.id);
                    self.snapshot.operation_active=true;
                    self.snapshot.operation_module=Some(if matches!(&request.command,Command::Profiles(_)) {"profiles"} else if matches!(&request.command,Command::Overrides(_)|Command::ScriptLogs(_)){"overrides"}else {"core"});
                    self.snapshot.information=None;
                    self.runtime_warning=None;
                    let operation = async {
                        match request.command.clone() {
                            Command::Profiles(command) => self.profiles(command).await,
                            Command::Overrides(change)=>self.overrides(change).await,
                            Command::ScriptLogs(id)=>self.script_logs(id),
                            Command::CheckDocument {kind,content}=>{
                                let result=if kind==crate::profiles::model::DocumentKind::JavaScript {crate::configuration::script::validate(&content)}else {crate::configuration::compose(content.as_bytes(),None,&Default::default()).map(|_|()).map_err(|error|error.to_string())};
                                if result.is_ok(){self.completed("配置语法有效".into());}result
                            },
                            Command::Start => self.start().await,
                            Command::Stop => self.stop().await,
                            Command::Restart => self.restart().await,
                            Command::Activate(id) => self.activate(id).await,
                            Command::Settings { kind, patch } => self.save_settings(kind, patch).await,
                            Command::Tunnel(change) => self.change_tunnel(change).await,
                            Command::Interfaces => match super::interfaces::list().await { Ok(interfaces) => { self.snapshot.interfaces = interfaces; Ok(()) }, Err(error) => Err(error) },
                            Command::Proxy(action)=>self.proxy_action(action).await,
                            Command::Chain(change)=>self.change_chain(change).await,
                            Command::Connections(action)=>self.connection_action(action).await,
                            Command::ProxyNames => match &self.running { Some(running) => match running.launch.client.proxy_names().await { Ok(names) => { self.snapshot.proxy_names = names; Ok(()) }, Err(error) => Err(error) }, None => Err("内核未运行，无法读取代理列表".into()) },
                            Command::Validate => self.validate_current().await,
                            Command::FlushFakeIp => self.flush_fake_ip().await,
                            Command::UpdateGeo => self.update_geo().await,
                            Command::UpdateDatabase(database) => self.update_database(database).await,
                            Command::Rules => self.refresh_rules().await,
                            Command::Resources => { self.resources_requested = true; self.refresh_resources().await },
                            Command::UpdateProviders { kind, name, generation } => self.update_providers(kind, name, generation).await,
                            Command::CheckUpdate(selection) => self.check_update(selection).await,
                            Command::Install(selection) => self.install(selection).await,
                            Command::Versions{refresh} => self.versions(refresh).await,
                        }
                    };
                    let result = tokio::select! {
                        biased;
                        _ = shutdown.changed() => Err("应用退出，内核操作已取消".to_owned()),
                        result = operation => result,
                    };
                    self.snapshot.operation_active=false;
                    self.snapshot.progress=None;
                    if let Err(error) = result {
                        // Preparation failure must preserve an existing ready child.
                        let phase = if self.running.is_some() && self.snapshot.phase == Phase::Ready { Phase::Ready } else { Phase::Failed };
                        self.publish(phase, Some(error));
                    } else {self.publish(self.snapshot.phase,None);}
                    drop(request);
                }
                _ = subscription_timer.tick() => {
                    let result=tokio::select! { biased; _=shutdown.changed()=>break, result=async {let profiles=self.automatic_profiles().await;let overrides=self.automatic_overrides().await;let errors=profiles.err().into_iter().chain(overrides.err()).collect::<Vec<_>>();if errors.is_empty(){Ok(())}else{Err(crate::failure::join(&errors))}}=>result };
                    if let Err(error)=result {self.publish(self.snapshot.phase,Some(error));}
                    else {self.publish(self.snapshot.phase,None);}
                },
                _ = polling.tick() => self.observe_exit().await,
                _ = health.tick() => {
                    if self.snapshot.phase == Phase::Ready && (self.snapshot.rules.generation, self.snapshot.rules.epoch) != (self.snapshot.generation, self.snapshot.monitor_epoch) {
                        let result=tokio::select! { biased; _=shutdown.changed()=>break, result=self.refresh_rules()=>result };
                        if let Err(error)=result { self.publish(self.snapshot.phase,Some(error)); }
                        else { self.publish(self.snapshot.phase,None); }
                    }
                    if let Some(running) = &self.running {
                        let result = tokio::select! {
                            biased;
                            _ = shutdown.changed() => break,
                            result = running.launch.client.version() => result,
                        };
                        self.observe_exit().await;
                        if self.resources_requested && self.resources_refreshed.elapsed() >= Duration::from_secs(10) && self.snapshot.phase == Phase::Ready {
                            let _ = self.refresh_resources().await;
                            self.publish(self.snapshot.phase, None);
                        }
                        if self.running.is_some() {
                            match result {
                                Ok(version) if self.snapshot.phase != Phase::Ready => {
                                    self.snapshot.version = Some(version);
                                    self.publish(Phase::Ready, None);
                                }
                                Err(error) if self.snapshot.phase == Phase::Ready => self.publish(Phase::Failed, Some(error)),
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
        let result = self.stop_validation().await;
        self.transfer = None;
        self.staging = None;
        let stopped = self.stop().await;
        let recovered = if stopped.is_ok()
            && let Some(recovery) = self.recovery.take()
        {
            recovery.restore()
        } else {
            Ok(())
        };
        self.upgrade_previous = None;
        let result = match (result, stopped) {
            (Err(validation), Err(child)) => Err(format!(
                "校验进程清理失败：{validation}；运行进程清理失败：{child}"
            )),
            (Err(error), _) | (_, Err(error)) => Err(error),
            _ => Ok(()),
        };
        let result = result.and(recovered);
        if let Err(error) = &result {
            self.publish(Phase::Failed, Some(format!("退出清理失败：{error}")));
            eprintln!("退出时无法确认本应用内核进程已结束：{error}");
        }
        self.updates.close();
        result
    }

    async fn refresh_rules(&mut self) -> Result<(), String> {
        let running = self
            .running
            .as_ref()
            .filter(|_| self.snapshot.phase == Phase::Ready)
            .ok_or("内核未就绪，无法读取规则")?;
        self.snapshot.rules = std::sync::Arc::new(
            running
                .launch
                .client
                .rules(self.snapshot.generation, self.snapshot.monitor_epoch)
                .await?,
        );
        Ok(())
    }

    async fn prepare(&mut self) -> Result<Launch, String> {
        let launch = Launch::prepare(&self.paths).await?;
        self.validate(launch).await
    }

    pub(super) async fn validate(&mut self, launch: Launch) -> Result<Launch, String> {
        let launch = launch;
        if self.validation.is_some() {
            return Err("上一次校验进程尚未确认退出，请先停止内核任务".into());
        }
        let mut validator = launch
            .command(true)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|error| format!("无法启动内核校验：{}", error.kind()))?;
        let captured = match (validator.stdout.take(), validator.stderr.take()) {
            (Some(stdout), Some(stderr)) => Some(super::output::capture(
                self.paths.clone(),
                self.logs.clone(),
                stdout,
                stderr,
                launch.redactions(),
                self.snapshot
                    .preferences
                    .as_ref()
                    .ok_or("内核日志设置尚未读取")?
                    .log_limits(),
            )),
            _ => None,
        };
        let (output, log_settings) = captured.map_or((None, None), |(task, settings)| {
            (Some(task), Some(settings))
        });
        self.validation = Some(Running {
            child: validator,
            launch,
            output,
            log_settings,
            _lifetime: None,
        });
        let lifetime = super::ownership::Lifetime::attach(
            &self.validation.as_ref().ok_or("校验进程已失效")?.child,
        );
        match lifetime {
            Ok(lifetime) => {
                if let Some(validation) = &mut self.validation {
                    validation._lifetime = Some(lifetime);
                }
            }
            Err(cause) => {
                self.stop_validation()
                    .await
                    .map_err(|cleanup| format!("{cause}；校验清理失败：{cleanup}"))?;
                return Err(cause);
            }
        }
        let downloading = crate::bootstrap::geodata_notice(&self.paths).is_some();
        if downloading {
            self.progress("校验配置 · 按需下载 Geo 数据");
        }
        let budget = Duration::from_secs(if downloading { 120 } else { 5 });
        let validation = self.validation.as_mut().ok_or("校验进程不存在")?;
        let result = tokio::time::timeout(budget, validation.child.wait()).await;
        if matches!(&result, Ok(Ok(_)))
            && let Some(output) = validation.output.take()
            && let Err(error) = await_output(output).await
        {
            self.validation = None;
            return Err(error);
        }
        match result {
            Ok(Ok(status)) if status.success() => self
                .validation
                .take()
                .map(|running| running.launch)
                .ok_or("校验结果已失效".into()),
            Ok(Ok(status)) => {
                self.validation = None;
                Err(format!(
                    "内核拒绝候选配置（退出状态 {status}）；保留原运行配置{}",
                    if downloading {
                        "。缺少 Geo 数据文件，自动下载可能失败；请检查网络及外部资源下载地址后重试"
                    } else {
                        ""
                    }
                ))
            }
            result => {
                let failure = if result.is_err() {
                    if downloading {
                        "内核配置校验超时；Geo 数据文件可能尚未下载完成，请检查网络及外部资源下载地址后重试"
                    } else {
                        "内核配置校验超时"
                    }
                } else {
                    "等待内核校验失败"
                };
                let cleanup = self.stop_validation().await;
                Err(match cleanup {
                    Ok(()) => failure.into(),
                    Err(error) => format!("{failure}；校验进程清理失败：{error}"),
                })
            }
        }
    }

    async fn stop_validation(&mut self) -> Result<(), String> {
        if let Some(validation) = self.validation.as_mut() {
            terminate(&mut validation.child).await?;
            let output = validation.output.take();
            self.validation = None;
            if let Some(output) = output {
                await_output(output).await?;
            }
        }
        Ok(())
    }

    async fn restart(&mut self) -> Result<(), String> {
        // Prepare before disrupting a ready child; retain its immutable config
        // and private endpoint until a replacement has actually become ready.
        let launch = self.prepare().await?;
        self.replace(launch, true).await
    }

    async fn activate(&mut self, id: u64) -> Result<(), String> {
        let source = crate::configuration::Source::from_profile(&self.paths, id)?;
        let launch = Launch::from_source(&self.paths, source).await?;
        let launch = self.validate(launch).await?;
        self.replace(launch, true).await?;
        self.refresh_profiles()?;
        self.publish(Phase::Ready, None);
        Ok(())
    }

    async fn apply(&mut self) -> Result<(), String> {
        let source = self
            .running
            .as_ref()
            .ok_or("内核尚未运行，请先激活订阅或启动内核")?
            .launch
            .source
            .clone();
        let launch = Launch::from_source(&self.paths, source).await?;
        let launch = self.validate(launch).await?;
        self.replace(launch, true).await
    }

    pub(super) fn completed(&mut self, message: String) {
        self.snapshot.information = Some(message);
        self.publish(
            if self.running.is_some() {
                Phase::Ready
            } else {
                Phase::Stopped
            },
            None,
        );
    }
    async fn validate_current(&mut self) -> Result<(), String> {
        let source = match &self.running {
            Some(running) => running.launch.source.clone(),
            None => crate::configuration::Source::load(&self.paths).await?,
        };
        let launch = Launch::from_source(&self.paths, source).await?;
        self.validate(launch).await?;
        self.completed("当前内核配置校验通过".into());
        Ok(())
    }
    async fn flush_fake_ip(&mut self) -> Result<(), String> {
        self.running
            .as_ref()
            .ok_or("内核未运行")?
            .launch
            .client
            .flush_fake_ip()
            .await?;
        self.completed("Fake-IP 缓存已清空".into());
        Ok(())
    }
    async fn update_database(
        &mut self,
        database: crate::configuration::resources::Database,
    ) -> Result<(), String> {
        use crate::configuration::resources::Database;
        use crate::storage::DirectoryKind;
        crate::bootstrap::require_core(&self.paths)?;
        let (_, key, name, file) = database.spec();
        let config: serde_yaml_ng::Value = match &self.running {
            Some(running) => {
                serde_yaml_ng::from_str(&running.launch.composed).map_err(|_| "运行配置无效")?
            }
            None => serde_yaml_ng::Value::Mapping(
                super::Preferences::load(&self.paths)?.native().clone(),
            ),
        };
        let url = config
            .get("geox-url")
            .and_then(|urls| urls.get(key))
            .and_then(serde_yaml_ng::Value::as_str)
            .unwrap_or(database.default_url())
            .to_owned();
        let bytes = self
            .fetch_resource(
                &url,
                super::download::ARCHIVE_LIMIT,
                Some(&format!("下载 {name}")),
            )
            .await?;
        self.progress(&format!("校验 {name}"));
        let rule = database.validation_rule(&bytes)?;
        let directory = tempfile::Builder::new()
            .prefix("geo-validate-")
            .tempdir_in(self.paths.directory(DirectoryKind::Cache))
            .map_err(|_| "无法创建资源校验目录")?;
        crate::storage::atomic_write(&directory.path().join(file), &bytes)
            .map_err(|_| "无法写入资源校验文件")?;
        let launch = Launch::for_database_validation(
            &self.paths,
            directory.path().to_owned(),
            matches!(database, Database::GeoIp | Database::GeoSite),
            rule,
        )
        .await?;
        drop(self.validate(launch).await?);
        let destination = self.paths.directory(DirectoryKind::Core).join(file);
        let previous = match std::fs::symlink_metadata(&destination) {
            Ok(metadata)
                if metadata.is_file()
                    && !metadata.file_type().is_symlink()
                    && metadata.len() <= super::download::ARCHIVE_LIMIT =>
            {
                Some(std::fs::read(&destination).map_err(|_| "无法保留原数据库")?)
            }
            Ok(_) => return Err("数据库路径或大小无效".into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => return Err("无法检查原数据库".into()),
        };
        let runtime = match &self.running {
            Some(running) => Some((running.launch.client.clone(), running.launch.yaml().await?)),
            None => None,
        };
        crate::storage::atomic_write(&destination, &bytes).map_err(|_| "无法替换数据库")?;
        if let Some((client, yaml)) = runtime {
            self.progress(&format!("加载 {name}"));
            if let Err(cause) = client.reload(&yaml).await {
                let files = match previous {
                    Some(bytes) => {
                        crate::storage::atomic_write(&destination, &bytes).map_err(|_| ())
                    }
                    None => std::fs::remove_file(&destination).map_err(|_| ()),
                };
                let runtime = client.reload(&yaml).await;
                return Err(format!(
                    "{name} 加载失败：{cause}；原文件恢复：{}；内核恢复：{}",
                    if files.is_ok() { "完成" } else { "失败" },
                    if runtime.is_ok() {
                        "已确认"
                    } else {
                        "失败"
                    }
                ));
            }
        }
        self.snapshot.resources.refresh_files(&self.paths);
        self.completed(format!("{name} 已更新"));
        Ok(())
    }
    async fn refresh_resources(&mut self) -> Result<(), String> {
        self.resources_refreshed = std::time::Instant::now();
        self.snapshot.resources.refresh_files(&self.paths);
        self.snapshot.resources.error = None;
        if self.snapshot.phase != Phase::Ready {
            self.snapshot.resources.proxies.clear();
            self.snapshot.resources.rules.clear();
            return Ok(());
        }
        let client = self
            .running
            .as_ref()
            .ok_or("内核未运行")?
            .launch
            .client
            .clone();
        let mut errors = Vec::new();
        for kind in [super::ProviderKind::Proxies, super::ProviderKind::Rules] {
            match client.providers(kind).await {
                Ok(providers) => self.snapshot.resources.replace_providers(kind, providers),
                Err(error) => errors.push((kind.label(), error)),
            }
        }
        if !errors.is_empty() {
            let error = crate::failure::scoped(errors);
            self.snapshot.resources.error = Some(error.clone());
            return Err(error);
        }
        Ok(())
    }
    async fn update_providers(
        &mut self,
        kind: super::ProviderKind,
        name: Option<String>,
        generation: u64,
    ) -> Result<(), String> {
        if generation != self.snapshot.generation {
            return Err("内核已变更，请刷新资源列表".into());
        }
        if self.snapshot.phase != Phase::Ready {
            return Err("内核未就绪".into());
        }
        let client = self
            .running
            .as_ref()
            .ok_or("内核未运行")?
            .launch
            .client
            .clone();
        let providers = client.providers(kind).await?;
        self.snapshot.resources.replace_providers(kind, providers);
        let selected: Vec<_> = self
            .snapshot
            .resources
            .providers(kind)
            .iter()
            .filter(|provider| name.as_ref().is_none_or(|name| name == provider.name()))
            .collect();
        if name.is_some() && selected.is_empty() {
            return Err("资源集合已变更，请刷新列表".into());
        }
        if selected.iter().any(|provider| provider.uncertain()) {
            return Err("上次更新结果待确认，请重启内核后重试".into());
        }
        let names: Vec<_> = selected
            .into_iter()
            .filter(|provider| provider.can_update())
            .map(|provider| provider.name().to_owned())
            .collect();
        if names.is_empty() {
            return Err("没有可更新的资源集合".into());
        }
        let mut failed = Vec::new();
        let mut completed = 0;
        for (index, name) in names.iter().enumerate() {
            self.progress(&format!(
                "更新{} · {name}（{}/{}）",
                kind.label(),
                index + 1,
                names.len()
            ));
            match client.update_provider(kind, name).await {
                Ok(()) => completed += 1,
                Err(error) => {
                    if !error.contains("返回 HTTP") {
                        self.snapshot.resources.mark_uncertain(kind, name);
                    }
                    failed.push((name.as_str(), error));
                }
            }
        }
        let refreshed = self.refresh_resources().await;
        if !failed.is_empty() {
            return Err(format!(
                "{}更新 {} 项，失败 {} 项：{}{}",
                kind.label(),
                completed,
                failed.len(),
                crate::failure::scoped(failed.iter().cloned()),
                if refreshed.is_err() {
                    "；列表刷新失败"
                } else {
                    ""
                }
            ));
        }
        if let Err(error) = refreshed {
            return Err(format!("{}已更新，列表刷新失败：{error}", kind.label()));
        }
        self.completed(format!("{}已更新 {} 项", kind.label(), completed));
        Ok(())
    }
    async fn update_geo(&mut self) -> Result<(), String> {
        if self.geo_unknown {
            return Err("上次 GeoData 更新结果待确认，请重启内核后再更新".into());
        }
        let client = self
            .running
            .as_ref()
            .ok_or("内核未运行")?
            .launch
            .client
            .clone();
        self.geo_unknown = true;
        self.progress("更新 Geo 数据");
        client.update_geo().await?;
        self.geo_unknown = false;
        self.snapshot.resources.refresh_files(&self.paths);
        self.completed("GeoData 已更新".into());
        Ok(())
    }
    async fn fetch(&mut self, url: &str, limit: u64) -> Result<Vec<u8>, String> {
        self.fetch_resource(url, limit, None).await
    }
    async fn fetch_resource(
        &mut self,
        url: &str,
        limit: u64,
        label: Option<&str>,
    ) -> Result<Vec<u8>, String> {
        if self.transfer.is_some() {
            return Err("上一次下载任务尚未清理".into());
        }
        self.progress(label.unwrap_or(if limit == super::download::ARCHIVE_LIMIT {
            "下载 Mihomo 内核"
        } else {
            "获取内核版本信息"
        }));
        self.transfer = Some(if label.is_some() {
            super::download::Transfer::resource(&self.http, &self.paths, url, limit)?
        } else {
            super::download::Transfer::start(&self.http, &self.paths, url, limit)?
        });
        let snapshot = &mut self.snapshot;
        let updates = &self.updates;
        let observer = &self.observer;
        let result = self
            .transfer
            .as_mut()
            .ok_or("下载任务已失效")?
            .finish(|received, total| {
                if let Some(progress) = &mut snapshot.progress {
                    progress.received = received;
                    progress.total = total;
                }
                snapshot.revision = snapshot.revision.saturating_add(1);
                Self::send_snapshot(snapshot, updates, observer);
            })
            .await;
        self.transfer = None;
        result
    }
    async fn resolve_release(
        &mut self,
        selection: &super::ReleaseSelection,
    ) -> Result<super::release::Asset, String> {
        let version = match selection {
            super::ReleaseSelection::Specific(version) => version.clone(),
            _ => String::from_utf8(
                self.fetch(selection.version_url().ok_or("发行通道无效")?, 1024)
                    .await?,
            )
            .map_err(|_| "版本号不是 UTF-8")?
            .trim()
            .to_owned(),
        };
        selection.asset(&version)
    }
    async fn check_update(&mut self, selection: super::ReleaseSelection) -> Result<(), String> {
        let asset = self.resolve_release(&selection).await?;
        let current = self.snapshot.version.as_deref();
        self.completed(if current == Some(asset.version.as_str()) {
            format!("{} 已是当前版本：{}", selection.label(), asset.version)
        } else {
            format!(
                "{} 可用版本：{}；点击切换并重启安装",
                selection.label(),
                asset.version
            )
        });
        Ok(())
    }
    async fn versions(&mut self, refresh: bool) -> Result<(), String> {
        if !refresh
            && self
                .versions_cached
                .is_some_and(|time| time.elapsed() < Duration::from_secs(300))
        {
            self.completed("版本列表已就绪".into());
            return Ok(());
        }
        let bytes = self
            .fetch(
                "https://api.github.com/repos/MetaCubeX/mihomo/tags?per_page=100",
                1024 * 1024,
            )
            .await?;
        let tags: Vec<super::release::Tag> =
            serde_json::from_slice(&bytes).map_err(|_| "官方版本列表格式无效")?;
        let mut versions = Vec::new();
        for tag in tags {
            if super::release::normalize(&tag.name).is_ok() && !versions.contains(&tag.name) {
                versions.push(tag.name);
            }
        }
        if versions.is_empty() {
            return Err("官方未返回可用版本".into());
        }
        self.snapshot.versions = versions;
        self.versions_cached = Some(std::time::Instant::now());
        self.completed("官方版本列表已更新".into());
        Ok(())
    }
    async fn install(&mut self, selection: super::ReleaseSelection) -> Result<(), String> {
        let asset = self.resolve_release(&selection).await?;
        let bytes = self
            .fetch(&asset.url, super::download::ARCHIVE_LIMIT)
            .await?;
        self.progress("解包并校验内核");
        let directory = tempfile::Builder::new()
            .prefix("core-install-")
            .tempdir_in(self.paths.directory(crate::storage::DirectoryKind::Cache))
            .map_err(|_| "无法创建内核安装暂存目录")?;
        let executable = directory.path().join(crate::bootstrap::core_name());
        self.staging = Some(directory);
        let unpacked = tokio::task::spawn_blocking(move || {
            if asset.zip {
                super::release::unpack_zip(&bytes)
            } else {
                super::release::unpack_gzip(&bytes)
            }
        })
        .await
        .map_err(|_| "内核解包任务异常退出")??;
        crate::storage::atomic_write(&executable, &unpacked).map_err(|_| "无法写入暂存内核")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
                .map_err(|_| "无法设置新内核执行权限")?;
        }
        let source = match &self.running {
            Some(running) => running.launch.source.clone(),
            None => crate::configuration::Source::load(&self.paths).await?,
        };
        let launch = Launch::for_install(&self.paths, source, executable).await?;
        let mut launch = self.validate(launch).await?;
        self.recovery = Some(super::install::Recovery::begin(&self.paths)?);
        self.upgrade_previous = self.stop_owned().await?;
        let managed = self
            .paths
            .directory(crate::storage::DirectoryKind::Core)
            .join(crate::bootstrap::core_name());
        let result = async {
            crate::storage::atomic_write(&managed, &unpacked).map_err(|_| "无法替换内核文件")?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                std::fs::set_permissions(&managed, std::fs::Permissions::from_mode(0o700))
                    .map_err(|_| "无法设置内核执行权限")?;
            }
            launch.use_executable(managed);
            self.start_prepared(launch).await?;
            if self.snapshot.version.as_deref() != Some(asset.version.as_str()) {
                return Err("新内核回读版本与下载版本不一致".into());
            }
            let previous = super::Preferences::load(&self.paths)?;
            let mut values = previous.values();
            values.insert("CoreReleaseIndex".into(), selection.label().into());
            if let super::ReleaseSelection::Specific(version) = &selection {
                values.insert("CoreSpecificVersion".into(), version.clone());
            }
            let preferences = super::Preferences::from_values(&values, Some(&previous))?;
            preferences.save(&self.paths)?;
            let running = self.running.as_ref().ok_or("新内核已退出")?;
            running.launch.source.commit(&self.paths)?;
            self.recovery.as_mut().ok_or("更新恢复记录丢失")?.commit()?;
            self.recovery = None;
            self.snapshot.preferences = Some(preferences);
            Ok::<_, String>(())
        }
        .await;
        match result {
            Ok(()) => {
                self.upgrade_previous = None;
                self.staging = None;
                self.completed(format!(
                    "{} 内核已安装并运行：{}",
                    selection.label(),
                    asset.version
                ));
                Ok(())
            }
            Err(cause) => {
                if let Err(error) = self.stop_owned().await {
                    return Err(format!("{cause}；新内核清理失败：{error}；恢复材料已保留"));
                }
                if let Some(recovery) = self.recovery.take() {
                    recovery
                        .restore()
                        .map_err(|error| format!("{cause}；旧内核文件恢复失败：{error}"))?;
                }
                self.snapshot.preferences = super::Preferences::load(&self.paths).ok();
                self.staging = None;
                if let Some(previous) = self.upgrade_previous.take() {
                    match self.start_prepared(previous).await {
                        Ok(()) => {
                            self.publish(Phase::Ready, None);
                            Err(format!("{cause}；旧内核已恢复就绪"))
                        }
                        Err(error) => Err(format!("{cause}；旧内核恢复启动失败：{error}")),
                    }
                } else {
                    Err(cause)
                }
            }
        }
    }

    async fn change_tunnel(
        &mut self,
        change: crate::configuration::tunnels::Change,
    ) -> Result<(), String> {
        let values = super::Preferences::load(&self.paths)?.values();
        let entries = crate::configuration::tunnels::parse(
            values.get("Tunnels").map(String::as_str).unwrap_or("[]"),
        )?;
        let entries = crate::configuration::tunnels::change(entries, change)?;
        let yaml = serde_yaml_ng::to_string(&entries).map_err(|_| "无法编码隧道配置")?;
        self.patch(crate::storage::Values::from([("Tunnels".into(), yaml)]))
            .await
    }
    async fn save_settings(
        &mut self,
        kind: crate::preferences::SettingsKind,
        patch: crate::storage::Values,
    ) -> Result<(), String> {
        use crate::preferences::{self, SettingsKind};
        if matches!(kind, SettingsKind::Core | SettingsKind::Resources) {
            return self.patch(patch).await;
        }
        let repository =
            crate::storage::Repository::open(self.paths.clone()).map_err(|_| "无法读取设置存储")?;
        let namespace = crate::storage::Namespace::parse(kind.id()).map_err(|_| "设置名称无效")?;
        let previous = preferences::values(&repository, kind).map_err(|error| error.to_string())?;
        let mut desired = previous.clone();
        let native_patch = patch.clone();
        desired.extend(preferences::choices::encode(patch));
        if let Some(key) = preferences::native::key(kind) {
            let root = if let Some(text) = native_patch.get(key) {
                serde_yaml_ng::from_str(text).map_err(|_| "原生配置无效".to_owned())?
            } else {
                let root = previous
                    .get(key)
                    .map(|text| {
                        serde_yaml_ng::from_str(text).map_err(|_| "原生配置无效".to_owned())
                    })
                    .unwrap_or_else(|| preferences::native::initial(kind))?;
                preferences::native::patch(kind, &root, &native_patch)?
            };
            let enabled = desired
                .get(preferences::native::flag(kind))
                .is_some_and(|value| value == "true");
            desired = preferences::native::project(kind, &root, enabled)?;
        }

        preferences::validate(kind, &desired)?;
        if matches!(kind, SettingsKind::Dns | SettingsKind::Sniffer) {
            crate::configuration::flow::validate_settings(kind, &desired)?;
            let candidate = if let Some(running) = &self.running {
                Some(
                    Launch::with_flow(
                        &self.paths,
                        running.launch.source.clone(),
                        kind,
                        desired.clone(),
                    )
                    .await?,
                )
            } else {
                None
            };
            let saved = desired.clone();
            let write_namespace = namespace.clone();
            let commit = || {
                repository
                    .save(&write_namespace, &saved)
                    .map_err(|error| error.to_string())
            };
            let result = if let Some(candidate) = candidate {
                self.apply_profile_candidate(candidate, commit).await
            } else {
                commit()
            };
            if let Err(error) = result {
                let restored = repository.save(&namespace, &previous);
                return Err(match restored {
                    Ok(()) => error,
                    Err(recovery) => format!("{error}；设置恢复失败：{recovery}"),
                });
            }
        } else {
            repository
                .save(&namespace, &desired)
                .map_err(|error| error.to_string())?;
            if kind == SettingsKind::Tun && self.running.is_some() {
                let core = super::Preferences::load(&self.paths)?;
                if let Err(error) = self.configure_hot(core.clone(), core).await {
                    let recovery = repository.save(&namespace, &previous);
                    return Err(match recovery {
                        Ok(()) => error,
                        Err(other) => format!("{error}；设置恢复失败：{other}"),
                    });
                }
            }
        }
        self.snapshot
            .settings
            .insert(kind.id().into(), desired.clone());
        if kind == SettingsKind::SysProxy
            && desired
                .get("IsSystemProxyEnabled")
                .is_some_and(|value| value == "true")
        {
            return Err("设置已保存；系统代理尚未接入".into());
        }
        Ok(())
    }
    async fn patch(&mut self, patch: crate::storage::Values) -> Result<(), String> {
        let previous = super::Preferences::load(&self.paths)?;
        let mut values = previous.values();
        if patch.contains_key("WebUiConfigured") {
            values.retain(|key, _| !key.starts_with("WebUi."));
        }
        values.extend(patch);
        self.configure(super::Preferences::from_values(&values, Some(&previous))?)
            .await
    }
    async fn configure(&mut self, draft: super::Preferences) -> Result<(), String> {
        let previous = super::Preferences::load(&self.paths)?;
        let desired = super::Preferences::from_values(&draft.values(), Some(&previous))?;
        let restart = ["external-controller", "external-controller-cors", "secret"]
            .iter()
            .any(|key| previous.native().get(*key) != desired.native().get(*key));
        if self.running.is_some() && previous.native() != desired.native() && !restart {
            return self.configure_hot(previous, desired).await;
        }
        desired.save(&self.paths)?;
        if self.running.is_some()
            && restart
            && let Err(cause) = self.apply().await
        {
            let rollback = previous.save(&self.paths);
            self.snapshot.preferences = Some(previous);
            return Err(match rollback {
                Ok(()) => format!("{cause}；已恢复旧内核设置"),
                Err(error) => format!("{cause}；设置恢复失败：{error}"),
            });
        }
        if let Some(settings) = self.running.as_ref().and_then(|r| r.log_settings.as_ref()) {
            let redactions = settings.borrow().0.clone();
            settings.send_replace((redactions, desired.log_limits()));
        }
        self.snapshot.preferences = Some(desired);
        self.publish(
            if self.running.is_some() {
                Phase::Ready
            } else {
                Phase::Stopped
            },
            None,
        );
        Ok(())
    }
    async fn configure_hot(
        &mut self,
        previous: super::Preferences,
        desired: super::Preferences,
    ) -> Result<(), String> {
        super::port::probe_tunnels(&previous, &desired)?;
        let running = self.running.as_ref().ok_or("内核尚未运行")?;
        let source = running.launch.source.clone();
        let client = running.launch.client.clone();
        let old_yaml = running.launch.yaml().await?;
        let old_expected: serde_yaml_ng::Value =
            serde_yaml_ng::from_str(&running.launch.composed).map_err(|_| "旧配置无效")?;
        let mut candidate = Launch::with_preferences(&self.paths, source, desired.clone()).await?;
        candidate
            .reuse_control(&self.running.as_ref().ok_or("内核尚未运行")?.launch)
            .await?;
        let candidate = self.validate(candidate).await?;
        let yaml = candidate.yaml().await?;
        let expected: serde_yaml_ng::Value =
            serde_yaml_ng::from_str(&candidate.composed).map_err(|_| "候选配置无效")?;
        if let Some(settings) = self.running.as_ref().and_then(|r| r.log_settings.as_ref()) {
            let mut redactions = settings.borrow().0.clone();
            redactions.extend(candidate.redactions());
            settings.send_replace((redactions, desired.log_limits()));
        }
        self.invalidate_monitor();
        let result = async {
            client.reload(&yaml).await?;
            client.verify_listeners(&expected).await?;
            desired.save(&self.paths)?;
            // Persist the source/config pair only after the API accepts the candidate.
            candidate.source.commit(&self.paths)?;
            self.running
                .as_mut()
                .ok_or("热更新期间内核已停止")?
                .launch
                .commit_hot(&candidate, &yaml)?;
            Ok::<(), String>(())
        }
        .await;
        if let Err(cause) = result {
            if let Some(settings) = self.running.as_ref().and_then(|r| r.log_settings.as_ref()) {
                let redactions = settings.borrow().0.clone();
                settings.send_replace((redactions, previous.log_limits()));
            }
            let files = previous.save(&self.paths);
            let restored = async {
                client.reload(&old_yaml).await?;
                client.verify_listeners(&old_expected).await
            }
            .await;
            let record = self
                .running
                .as_ref()
                .ok_or_else(|| "旧内核不存在".to_owned())
                .and_then(|r| r.launch.source.commit(&self.paths));
            self.snapshot.preferences = Some(previous);
            return Err(format!(
                "{cause}；旧设置恢复：{}；旧内核配置恢复：{}；来源恢复：{}",
                if files.is_ok() { "完成" } else { "失败" },
                restored.as_ref().map_or_else(|e| e.as_str(), |_| "已确认"),
                if record.is_ok() { "完成" } else { "失败" }
            ));
        }
        self.snapshot.preferences = Some(desired);
        if self.resources_requested {
            let _ = self.refresh_resources().await;
        }
        self.publish(Phase::Ready, None);
        Ok(())
    }

    async fn replace(&mut self, launch: Launch, persist: bool) -> Result<(), String> {
        let selection = launch.source.profile().map(|profile| profile.id());
        let paths = self.paths.clone();
        self.replace_committing(launch, move || {
            if persist {
                crate::configuration::select_profile(&paths, selection)
            } else {
                Ok(())
            }
        })
        .await
    }
    pub(super) async fn replace_committing(
        &mut self,
        launch: Launch,
        commit: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), String> {
        let previous = self.stop_owned().await?;
        let started = self.start_prepared(launch).await;
        let result = started.and_then(|()| commit());
        match result {
            Ok(()) => {
                self.publish(Phase::Ready, None);
                Ok(())
            }
            Err(cause) => {
                if self.running.is_some()
                    && let Err(cleanup) = self.stop_owned().await
                {
                    return Err(format!(
                        "{cause}；替换进程清理失败：{cleanup}；未重复启动旧内核"
                    ));
                }
                if self.running.is_some() {
                    return Err(format!("{cause}；替换进程未清理，未重复启动旧内核"));
                }
                if let Some(previous) = previous {
                    match self.start_prepared(previous).await {
                        Ok(()) => {
                            self.publish(Phase::Ready, None);
                            Err(format!("{cause}；已确认旧配置内核恢复就绪"))
                        }
                        Err(recovery) => Err(format!("{cause}；旧配置内核恢复失败：{recovery}")),
                    }
                } else {
                    Err(cause)
                }
            }
        }
    }

    async fn start(&mut self) -> Result<(), String> {
        if self.running.is_some() {
            if self.snapshot.phase == Phase::Ready {
                self.publish(Phase::Ready, None);
                return Ok(());
            }
            return Err("已有内核进程但尚未确认就绪，请停止后重试".into());
        }
        let repository = crate::storage::Repository::open(self.paths.clone())
            .map_err(|error| error.to_string())?;
        crate::preferences::initialize(&repository).map_err(|error| error.to_string())?;
        self.snapshot.preferences = Some(super::Preferences::load(&self.paths)?);
        self.publish(Phase::Starting, None);
        let launch = Launch::prepare_startup(&self.paths).await?;
        let launch = self.validate(launch).await?;
        self.replace(launch, true).await
    }

    /// Startup is strict: never change requested ports or substitute a profile.
    async fn startup(&mut self) -> Result<(), String> {
        let preferences = super::Preferences::load(&self.paths)?;
        if preferences
            .values()
            .get("StartCore")
            .is_some_and(|value| value == "false")
        {
            self.publish(Phase::Stopped, None);
            return Ok(());
        }
        self.start().await?;
        let repository =
            crate::storage::Repository::open(self.paths.clone()).map_err(|_| "无法读取启动设置")?;
        let proxy =
            crate::preferences::values(&repository, crate::preferences::SettingsKind::SysProxy)
                .map_err(|_| "无法读取系统代理设置")?;
        if proxy
            .get("IsSystemProxyEnabled")
            .is_some_and(|value| value == "true")
        {
            self.publish(
                Phase::Ready,
                Some("内核已启动；系统代理尚未接入，无法应用已保存设置".into()),
            );
        }
        Ok(())
    }

    async fn start_prepared(&mut self, launch: Launch) -> Result<(), String> {
        self.snapshot.proxy_names.clear();
        self.snapshot.resources = super::Resources::default();
        self.snapshot.generation = self
            .snapshot
            .generation
            .checked_add(1)
            .ok_or("内核代次已耗尽")?;
        self.publish(Phase::Starting, None);
        let limits = super::Preferences::load(&self.paths)?.log_limits();
        let mut child = launch
            .command(false)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|error| format!("无法启动 Mihomo：{}", error.kind()))?;
        let captured = match (child.stdout.take(), child.stderr.take()) {
            (Some(stdout), Some(stderr)) => Some(super::output::capture(
                self.paths.clone(),
                self.logs.clone(),
                stdout,
                stderr,
                launch.redactions(),
                limits,
            )),
            _ => None,
        };
        let (output, log_settings) = captured.map_or((None, None), |(task, settings)| {
            (Some(task), Some(settings))
        });
        self.running = Some(Running {
            child,
            launch,
            output,
            log_settings,
            _lifetime: None,
        });
        let lifetime = super::ownership::Lifetime::attach(
            &self.running.as_ref().ok_or("内核进程已失效")?.child,
        );
        match lifetime {
            Ok(lifetime) => {
                if let Some(running) = &mut self.running {
                    running._lifetime = Some(lifetime);
                }
            }
            Err(error) => {
                self.stop()
                    .await
                    .map_err(|cleanup| format!("{error}；清理失败：{cleanup}"))?;
                return Err(error);
            }
        }
        self.geo_unknown = false;
        let downloading = crate::bootstrap::geodata_notice(&self.paths).is_some();
        if downloading {
            self.progress("启动内核 · 按需下载 Geo 数据");
        }
        let budget = Duration::from_secs(if downloading { 120 } else { 8 });
        let result = tokio::time::timeout(budget, self.wait_ready())
            .await
            .map_err(|_| if downloading { "Mihomo API 未在 120 秒内就绪；Geo 数据文件可能尚未下载完成，请检查网络及外部资源下载地址后重试".to_owned() } else { "Mihomo API 未在 8 秒内就绪".to_owned() })
            .and_then(|result| result);
        match result {
            Ok(version) => {
                self.snapshot.version = Some(version);
                if let Err(error) = self
                    .restore_proxy_selections()
                    .await
                    .and(self.restore_chain_selection().await)
                {
                    self.runtime_warning = Some(format!("内核已就绪；节点选择恢复失败：{error}"));
                }
                Ok(())
            }
            Err(error) => match self.stop().await {
                Ok(()) => Err(error),
                Err(cleanup) => Err(format!("{error}；启动失败后的进程清理也失败：{cleanup}")),
            },
        }
    }

    async fn wait_ready(&mut self) -> Result<String, String> {
        loop {
            let running = self.running.as_mut().ok_or("内核进程不存在")?;
            if let Some(status) = running
                .child
                .try_wait()
                .map_err(|error| format!("无法检查内核进程：{}", error.kind()))?
            {
                return Err(format!("内核在 API 就绪前退出（{status}）"));
            }
            if let Ok(version) = running.launch.client.version().await {
                let expected: serde_yaml_ng::Value =
                    serde_yaml_ng::from_str(&running.launch.composed)
                        .map_err(|_| "无法读取候选监听设置")?;
                running.launch.client.verify_listeners(&expected).await?;
                // The API and process must belong to this generation and both still exist.
                if running
                    .child
                    .try_wait()
                    .map_err(|_| "无法确认内核存活")?
                    .is_none()
                {
                    return Ok(version);
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    async fn stop(&mut self) -> Result<(), String> {
        self.stop_validation().await?;
        self.stop_owned().await?;
        self.publish(Phase::Stopped, None);
        Ok(())
    }

    async fn stop_owned(&mut self) -> Result<Option<Launch>, String> {
        if self.running.is_some() {
            self.publish(Phase::Stopping, None);
            let running = self.running.as_mut().ok_or("内核进程不存在")?;
            // Child handle is authoritative. Never discover/kill by process name or PID alone.
            terminate(&mut running.child).await?;
            if let Some(mut output) = running.output.take() {
                match tokio::time::timeout(Duration::from_secs(2), &mut output).await {
                    Ok(Ok(Ok(()))) => {}
                    Ok(result) => eprintln!("内核进程已退出，但日志任务未完成：{result:?}"),
                    Err(_) => {
                        output.abort();
                        let _ = output.await;
                        eprintln!("内核进程已退出，日志任务已停止");
                    }
                }
            }
        }
        Ok(self.running.take().map(|running| running.launch))
    }

    async fn observe_exit(&mut self) {
        let Some(running) = self.running.as_mut() else {
            return;
        };
        match running.child.try_wait() {
            Ok(Some(status)) => {
                if let Some(output) = running.output.take() {
                    let _ = await_output(output).await;
                }
                self.running = None;
                self.publish(Phase::Failed, Some(format!("内核意外退出（{status}）")));
            }
            Ok(None) => {}
            Err(error) => self.publish(
                Phase::Failed,
                Some(format!("无法检查内核进程：{}", error.kind())),
            ),
        }
    }
}

pub(super) async fn terminate(child: &mut Child) -> Result<(), String> {
    if child
        .try_wait()
        .map_err(|error| format!("检查进程退出失败：{}", error.kind()))?
        .is_some()
    {
        return Ok(());
    }
    child
        .start_kill()
        .map_err(|error| format!("停止本应用内核失败：{}", error.kind()))?;
    tokio::time::timeout(Duration::from_secs(2), child.wait())
        .await
        .map_err(|_| "停止内核后未能确认进程退出".to_owned())?
        .map_err(|error| format!("等待本应用内核退出失败：{}", error.kind()))?;
    Ok(())
}

#[cfg(all(test, unix))]
#[path = "tests.rs"]
mod tests;

async fn await_output(
    mut output: tokio::task::JoinHandle<Result<(), String>>,
) -> Result<(), String> {
    match tokio::time::timeout(Duration::from_secs(2), &mut output).await {
        Ok(Ok(result)) => result,
        Ok(Err(error)) => Err(format!("内核日志任务失败：{error}")),
        Err(_) => {
            output.abort();
            let _ = output.await;
            Err("内核日志读取超时".into())
        }
    }
}
