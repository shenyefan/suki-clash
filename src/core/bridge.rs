//! Application-owned GPUI adapter; feature state remains in the lifecycle actor.
use super::{Command, Phase, Service, Snapshot};
use crate::storage::AppPaths;
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Entity, Global, Subscription, Task, WeakEntity, Window,
};

type Completion = Box<dyn FnOnce(Result<(), String>, &mut Window, &mut App)>;
pub(crate) struct Controller {
    snapshot: Snapshot,
    runtime: std::sync::Arc<super::Runtime>,
    delay_log_sequence: u64,
    _runtime_events: Option<Task<()>>,
    service: Option<Service>,
    _events: Option<Task<()>>,
    _quit: Option<Subscription>,
    quitting: Option<Task<()>>,
    pending: Option<u64>,
    notification_window: Option<AnyWindowHandle>,
    progress_toast: Option<(AnyWindowHandle, Entity<crate::ui::DownloadToast>)>,
    completions: std::collections::BTreeMap<u64, (AnyWindowHandle, Completion)>,
}
impl Controller {
    fn sync_progress(&mut self, cx: &mut gpui_kit::Context<Self>) {
        if let Some(progress) = &self.snapshot.progress {
            if let Some((_, toast)) = &self.progress_toast {
                toast.update(cx, |toast, cx| {
                    toast.update_progress(
                        progress.label(),
                        progress.received(),
                        progress.total(),
                        cx,
                    )
                });
            } else if let Some(handle) = self.notification_window {
                let toast = handle.update(cx, |_, window, cx| {
                    crate::ui::DownloadToast::show(
                        progress.label(),
                        progress.received(),
                        progress.total(),
                        window,
                        cx,
                    )
                });
                if let Ok(toast) = toast {
                    self.progress_toast = Some((handle, toast));
                }
            }
        } else if let Some((handle, toast)) = self.progress_toast.take() {
            let _ = handle.update(cx, |_, window, cx| {
                crate::ui::DownloadToast::dismiss(&toast, window, cx)
            });
        }
    }
    pub(super) fn service_test_delay(
        &mut self,
        test: crate::proxies::runtime::Test,
    ) -> Result<u64, String> {
        self.service
            .as_mut()
            .ok_or("内核执行端不可用")?
            .test_delay(test)
    }
    #[cfg(test)]
    pub(crate) fn shutdown_test_worker(
        &mut self,
    ) -> Option<std::thread::JoinHandle<Result<(), String>>> {
        self.service.as_mut().and_then(Service::shutdown)
    }
    pub(crate) fn runtime(&self) -> std::sync::Arc<super::Runtime> {
        self.runtime.clone()
    }
    pub(crate) fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }
    pub(crate) fn is_busy(&self) -> bool {
        self.quitting.is_some() || self.pending.is_some() || self.snapshot.is_busy()
    }
}
struct Handle(Entity<Controller>);
impl Global for Handle {}

pub(crate) fn controller(cx: &App) -> Entity<Controller> {
    cx.global::<Handle>().0.clone()
}

pub(crate) fn try_controller(cx: &App) -> Option<Entity<Controller>> {
    cx.try_global::<Handle>().map(|h| h.0.clone())
}

/// Register the first window before asynchronous startup results arrive. A
/// failure produced before window construction is replayed once as a toast.
pub(crate) fn attach_window(window: &Window, cx: &mut App) {
    let handle = window.window_handle();
    let report = controller(cx).update(cx, |this, _| {
        let first = this.notification_window.is_none();
        this.notification_window = Some(handle);
        first.then(|| (this.snapshot.error.clone(), this.snapshot.notice.clone()))
    });
    cx.defer(|cx| {
        controller(cx).update(cx, |this, cx| this.sync_progress(cx));
    });
    if let Some((error, notice)) = report {
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, cx| {
                if let Some(error) = error {
                    crate::ui::show_error(error, window, cx);
                }
                if let Some(notice) = notice {
                    crate::ui::notifications::push(
                        window,
                        gpui_kit::component::notification::Notification::warning(notice),
                        cx,
                    );
                }
            });
        });
    }
}

pub(crate) fn init(paths: AppPaths, _logs: crate::logging::Feed, cx: &mut App) {
    #[cfg(not(test))]
    let (service, snapshot) = match Service::start_with_logs(paths, _logs) {
        Ok(service) => (Some(service), Snapshot::default()),
        Err(error) => (
            None,
            Snapshot {
                phase: Phase::Failed,
                error: Some(format!("内核执行端初始化失败：{}", error.kind())),
                ..Snapshot::default()
            },
        ),
    };
    // GPUI's deterministic scheduler cannot be woken by an OS thread. UI
    // integration tests render an explicitly stopped adapter; lifecycle and
    // process work is exercised separately through the real Service/Actor tests.
    #[cfg(test)]
    let (service, snapshot) = (
        None,
        Snapshot {
            preferences: super::Preferences::load(&paths).ok(),
            ..Snapshot::default()
        },
    );
    install(service, snapshot, cx);
}
pub(crate) fn init_unavailable(message: String, cx: &mut App) {
    install(
        None,
        Snapshot {
            phase: Phase::Failed,
            error: Some(message),
            ..Snapshot::default()
        },
        cx,
    );
}
fn install(service: Option<Service>, snapshot: Snapshot, cx: &mut App) {
    let owner = cx.new(|_| Controller {
        snapshot,
        runtime: Default::default(),
        delay_log_sequence: 0,
        _runtime_events: None,
        service,
        _events: None,
        _quit: None,
        quitting: None,
        pending: None,
        notification_window: None,
        progress_toast: None,
        completions: Default::default(),
    });
    owner.update(cx, |this, cx| {
        if let Some(service) = &this.service {
            let runtime_events = service.runtime_events();
            this._runtime_events =
                Some(cx.spawn(async move |owner: WeakEntity<Controller>, cx| {
                    while let Ok(runtime) = runtime_events.recv().await {
                        let report = owner.update(cx, |this, cx| {
                            if runtime.generation != 0
                                && (runtime.generation != this.snapshot.generation
                                    || runtime.epoch < this.snapshot.monitor_epoch)
                            {
                                return vec![];
                            }
                            let mut errors = vec![];
                            for (current, previous, level) in [
                                (&runtime.error, &this.runtime.error, "ERROR"),
                                (&runtime.stream_error, &this.runtime.stream_error, "WARNING"),
                                (&runtime.probe_error, &this.runtime.probe_error, "WARNING"),
                            ] {
                                if current != previous
                                    && let Some(error) = current
                                    && !errors.iter().any(|(_, _, message)| message == error)
                                {
                                    errors.push((level, "statistics", error.clone()));
                                }
                            }
                            if runtime.icons.error != this.runtime.icons.error
                                && let Some(error) = &runtime.icons.error
                            {
                                errors.push(("WARNING", "proxies", error.clone()));
                            }
                            if runtime.delay_error != this.runtime.delay_error
                                && let Some((_, error)) = &runtime.delay_error
                            {
                                errors.push(("WARNING", "proxies", error.clone()));
                            }
                            if runtime.icons != this.runtime.icons
                                && !runtime.icons.pending
                                && !runtime.icons.files.is_empty()
                            {
                                crate::integration::announce_module(
                                    "DEBUG",
                                    "proxies",
                                    format!("图标缓存已就绪，{} 项", runtime.icons.files.len()),
                                    cx,
                                );
                            }
                            for (sequence, count, failed) in &runtime.delay_logs {
                                if *sequence > this.delay_log_sequence {
                                    this.delay_log_sequence = *sequence;
                                    crate::integration::announce_module(
                                        if *failed == 0 { "DEBUG" } else { "WARNING" },
                                        "proxies",
                                        format!("测速完成，{count} 项，{failed} 项失败"),
                                        cx,
                                    );
                                }
                            }
                            this.runtime = runtime;

                            cx.notify();
                            errors
                        });
                        match report {
                            Ok(errors) => cx.update(|cx| {
                                for (level, module, error) in errors {
                                    crate::integration::announce_module(
                                        level,
                                        module,
                                        error.clone(),
                                        cx,
                                    );
                                    if let Some(window) = cx.active_window() {
                                        let _ = window.update(cx, |_, window, cx| {
                                            crate::ui::show_error(error, window, cx)
                                        });
                                    }
                                }
                            }),
                            Err(_) => break,
                        }
                    }
                }));
            let events = service.events();
            this._events = Some(cx.spawn(async move |owner: WeakEntity<Controller>, cx| {
                while let Ok(snapshot) = events.recv().await {
                    let report = owner.update(cx, |this, cx| {
                        if snapshot.follows(&this.snapshot) && snapshot.request_id >= this.pending {
                            if snapshot.phase != this.snapshot.phase {
                                let event = match snapshot.phase {
                                    Phase::Ready => Some(("INFO", "内核已就绪")),
                                    Phase::Stopped => Some(("INFO", "内核已停止")),
                                    Phase::Starting => Some(("DEBUG", "正在启动内核")),
                                    Phase::Stopping => Some(("DEBUG", "正在停止内核")),
                                    _ => None,
                                };
                                if let Some((level, message)) = event {
                                    crate::integration::announce(level, message.into(), cx);
                                }
                            }
                            let report = snapshot
                                .error()
                                .filter(|error| {
                                    Some(*error) != this.snapshot.error()
                                        || snapshot.request_id != this.snapshot.request_id
                                })
                                .map(|error| ("ERROR", error.to_owned(), this.notification_window))
                                .or_else(|| {
                                    (snapshot.notice != this.snapshot.notice)
                                        .then(|| {
                                            snapshot.notice.as_ref().map(|notice| {
                                                (
                                                    "WARNING",
                                                    notice.clone(),
                                                    this.notification_window,
                                                )
                                            })
                                        })
                                        .flatten()
                                })
                                .or_else(|| {
                                    (snapshot.information != this.snapshot.information
                                        || snapshot.request_id != this.snapshot.request_id)
                                        .then(|| {
                                            snapshot.information.as_ref().map(|message| {
                                                ("INFO", message.clone(), this.notification_window)
                                            })
                                        })
                                        .flatten()
                                });
                            if !snapshot.is_busy() {
                                this.pending = None;
                            }
                            let module = snapshot.operation_module.unwrap_or("core");
                            this.snapshot = snapshot;
                            this.sync_progress(cx);
                            cx.notify();
                            report.map(|(level, message, window)| (level, message, window, module))
                        } else {
                            None
                        }
                    });
                    match report {
                        Ok(Some((level, message, origin, module))) => cx.update(|cx| {
                            crate::integration::announce_module(level, module, message.clone(), cx);
                            if let Some(window) = origin.or_else(|| cx.active_window()) {
                                let _ = window.update(cx, |_, window, cx| {
                                    if level == "ERROR" {
                                        crate::ui::show_error(message, window, cx);
                                    } else {
                                        let note = if level == "WARNING" {
                                            gpui_kit::component::notification::Notification::warning(message)
                                        } else {
                                            gpui_kit::component::notification::Notification::info(message)
                                        };
                                        crate::ui::notifications::push(window, note, cx);
                                    }
                                });
                            }
                        }),
                        Ok(None) => {}
                        Err(_) => break,
                    }
                    let completed = owner.update(cx, |this, _| {
                        if this.snapshot.is_busy() {
                            return vec![];
                        }
                        let Some(request) = this.snapshot.request_id else {
                            return vec![];
                        };
                        let ids = this
                            .completions
                            .keys()
                            .copied()
                            .filter(|id| *id <= request)
                            .collect::<Vec<_>>();
                        ids.into_iter()
                            .filter_map(|id| {
                                this.completions.remove(&id).map(|(window, complete)| {
                                    (window, complete, this.snapshot.error.clone())
                                })
                            })
                            .collect::<Vec<_>>()
                    });
                    if let Ok(completed) = completed {
                        for (window, complete, error) in completed {
                            let _ = window.update(cx, |_, window, cx| {
                                complete(error.map_or(Ok(()), Err), window, cx)
                            });
                        }
                    }
                }
            }));
        }
        this._quit = Some(cx.on_app_quit(move |this, cx| {
            let worker = this.service.as_mut().and_then(Service::shutdown);
            // GPUI gives returned futures only 200 ms. A native termination is
            // already irreversible, so wait here with a hard deadline for the
            // existing actor (which never needs the foreground executor).
            // Interactive quit performs its cancellable preparation asynchronously.
            if let Some(worker) = worker
                && let Err(error) = wait_session_cleanup(worker, std::time::Duration::from_secs(3))
            {
                crate::integration::logs(cx).publish(crate::logging::Record::program(
                    crate::logging::Level::Error,
                    "core",
                    &format!("系统结束清理未完成：{error}"),
                ));
            }
            std::future::ready(())
        }));
    });
    cx.set_global(Handle(owner));
}

pub(crate) fn request(command: Command, window: &mut Window, cx: &mut App) {
    if let Err(error) = submit(command, window, cx) {
        crate::ui::show_error(error, window, cx);
    }
}

pub(crate) fn submit(command: Command, window: &mut Window, cx: &mut App) -> Result<u64, String> {
    let owner = controller(cx);
    let result = owner.update(cx, |this, cx| {
        if this.quitting.is_some() {
            return Err(super::ServiceError::Busy);
        }
        let service = this.service.as_mut().ok_or(super::ServiceError::Closed)?;
        let id = service.submit(command)?;
        this.pending = Some(id);
        this.notification_window = Some(window.window_handle());
        cx.notify();
        Ok::<_, super::ServiceError>(id)
    });
    result.map_err(|error| {
        let message = match error {
            super::ServiceError::Closed => owner
                .read(cx)
                .snapshot
                .error
                .clone()
                .unwrap_or_else(|| error.to_string()),
            _ => error.to_string(),
        };
        crate::integration::announce_module("ERROR", "core", message.clone(), cx);
        message
    })
}

pub(crate) fn refresh_ip(window: &mut Window, cx: &mut App) {
    let available = controller(cx).update(cx, |this, _| {
        if this.snapshot.phase != Phase::Ready {
            return false;
        }
        if let Some(service) = &this.service {
            service.refresh_ip();
            true
        } else {
            false
        }
    });
    if !available {
        crate::ui::show_error("内核未就绪，无法刷新出口 IP".into(), window, cx);
    }
}
pub(crate) fn execute(
    command: Command,
    window: &mut Window,
    cx: &mut App,
    complete: impl FnOnce(Result<(), String>, &mut Window, &mut App) + 'static,
) -> Result<(), String> {
    let request = submit(command, window, cx)?;
    controller(cx).update(cx, |owner, _| {
        owner
            .completions
            .insert(request, (window.window_handle(), Box::new(complete)));
    });
    Ok(())
}
/// Explicit quit waits before calling GPUI quit (whose fallback cleanup budget
/// is only 200 ms). Window-close and all application quit commands use this path.
pub(crate) fn quit(cx: &mut App) {
    finish(Exit::Quit, cx);
}
pub(crate) fn restart_program(cx: &mut App) {
    finish(Exit::Restart, cx);
}
pub(crate) fn reset_program(window: &mut Window, cx: &mut App) {
    let Some(paths) = crate::integration::paths(cx) else {
        crate::ui::show_error("无法确定应用配置目录".into(), window, cx);
        return;
    };
    finish(Exit::Reset(paths), cx);
}
pub(crate) fn apply_app_update(expected: String, cx: &mut App) {
    finish(Exit::Update(expected), cx);
}
enum Exit {
    Quit,
    Update(String),
    Restart,
    Reset(AppPaths),
}
fn finish(exit: Exit, cx: &mut App) {
    let owner = controller(cx);
    owner.update(cx, |this, cx| {
        if this.quitting.is_some() {
            return;
        }
        this.quitting = Some(cx.spawn(async move |owner, cx| {
            let joined = async {
                let installer = if let Exit::Update(expected) = &exit {
                    let expected = expected.clone();
                    Some(
                        cx.background_spawn(
                            async move { crate::updates::prepare_installer(&expected) },
                        )
                        .await?,
                    )
                } else {
                    None
                };
                let preparation = owner
                    .update(cx, |this, _| {
                        this.service.as_mut().map(Service::prepare_exit).transpose()
                    })
                    .map_err(|error| error.to_string())?;
                if let Some(reply) = preparation.map_err(|error| error.to_string())? {
                    reply.recv().await.map_err(|_| "内核退出准备应答中断")??;
                }
                if let Some(flush) = cx
                    .update(crate::integration::flush)
                    .map_err(|error| error.to_string())?
                {
                    flush.receive().await.map_err(|error| error.to_string())?;
                }
                if let Exit::Reset(paths) = &exit {
                    let paths = paths.clone();
                    cx.background_spawn(async move {
                        crate::desktop::autostart::reset_configuration(&paths)
                    })
                    .await?;
                }
                if let Some(installer) = installer {
                    cx.background_spawn(async move { installer.launch() })
                        .await?;
                }
                if let Exit::Restart = exit {
                    // Resolve the executable before giving up instance ownership.
                    let executable = cx
                        .background_spawn(async {
                            std::env::current_exe()
                                .map_err(|error| format!("无法定位程序：{error}"))
                        })
                        .await?;
                    let paths = cx
                        .update(|cx| crate::integration::paths(cx))
                        .ok_or("无法确定实例控制目录")?;
                    let desktop_worker = cx.update(crate::desktop::shutdown);
                    let replacement = cx
                        .background_spawn(async move {
                            let stopped = desktop_worker.map_or(Ok(()), |worker| {
                                worker
                                    .join()
                                    .map_err(|_| "实例控制服务清理失败".to_string())
                            });
                            let spawned = stopped.and_then(|()| {
                                std::process::Command::new(executable)
                                    .spawn()
                                    .map(|_| ())
                                    .map_err(|error| format!("无法重新启动程序：{error}"))
                            });
                            match spawned {
                                Ok(()) => Ok(None),
                                Err(message) => {
                                    match crate::desktop::instance::Service::acquire(&paths, None) {
                                        Ok(crate::desktop::instance::Disposition::Primary(
                                            service,
                                        )) => Ok(Some((service, message))),
                                        Ok(crate::desktop::instance::Disposition::Secondary) => {
                                            Err(format!("{message}；实例控制已由另一进程接管"))
                                        }
                                        Err(recovery) => {
                                            Err(format!("{message}；实例控制恢复失败：{recovery}"))
                                        }
                                    }
                                }
                            }
                        })
                        .await?;
                    if let Some((service, error)) = replacement {
                        cx.update(|cx| crate::desktop::recover(service, cx));
                        return Err(error);
                    }
                }
                Ok::<_, String>(())
            }
            .await;
            if let Err(error) = joined {
                let window = owner
                    .update(cx, |this, cx| {
                        if let Some(task) = this.quitting.take() {
                            task.detach();
                        }
                        cx.notify();
                        this.notification_window
                    })
                    .ok()
                    .flatten();
                cx.update(|cx| {
                    crate::integration::announce("ERROR", format!("退出清理未完成：{error}"), cx);
                    if let Some(window) = window.or_else(|| cx.active_window()) {
                        let _ = window.update(cx, |_, window, cx| {
                            window.activate_window();
                            cx.activate(true);
                            crate::ui::show_error(format!("退出清理未完成：{error}"), window, cx)
                        });
                    }
                });
                return;
            }
            // All fallible preparation completed. Only now destroy owners.
            let (core_worker, storage_worker, desktop_worker) = cx.update(|cx| {
                let core_worker = controller(cx).update(cx, |this, _| {
                    this.service.as_mut().and_then(Service::shutdown)
                });
                (
                    core_worker,
                    crate::integration::shutdown(cx),
                    crate::desktop::shutdown(cx),
                )
            });
            let logs = cx.update(|cx| crate::integration::logs(cx));
            cx.background_spawn(async move {
                let mut errors = Vec::new();
                if let Some(worker) = core_worker {
                    match worker.join() {
                        Ok(Ok(())) => {}
                        Ok(Err(error)) => errors.push(error),
                        Err(_) => errors.push("内核执行线程异常退出".into()),
                    }
                }
                if let Some(worker) = storage_worker
                    && worker.join().is_err()
                {
                    errors.push("存储执行线程异常退出".into());
                }
                if let Some(worker) = desktop_worker
                    && worker.join().is_err()
                {
                    errors.push("实例控制线程异常退出".into());
                }
                for error in errors {
                    logs.publish(crate::logging::Record::program(
                        crate::logging::Level::Error,
                        "core",
                        &error,
                    ));
                    eprintln!("{error}");
                }
            })
            .await;
            cx.update(|cx| cx.quit());
        }));
    });
}

fn wait_session_cleanup(
    worker: std::thread::JoinHandle<Result<(), String>>,
    deadline: std::time::Duration,
) -> Result<(), String> {
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("suki-session-ending".into())
        .spawn(move || {
            let result = worker
                .join()
                .unwrap_or_else(|_| Err("内核清理线程异常退出".into()));
            let _ = sender.send(result);
        })
        .map_err(|error| format!("无法等待内核清理：{error}"))?;
    receiver
        .recv_timeout(deadline)
        .map_err(|error| match error {
            std::sync::mpsc::RecvTimeoutError::Timeout => "已达到系统结束清理时限".to_string(),
            std::sync::mpsc::RecvTimeoutError::Disconnected => "系统结束清理应答中断".to_string(),
        })?
}

#[cfg(test)]
impl Controller {
    pub(crate) fn publish_test_settings(
        &mut self,
        key: &str,
        values: crate::storage::Values,
        cx: &mut gpui_kit::Context<Self>,
    ) {
        self.snapshot.settings.insert(key.into(), values);
        self.snapshot.revision = self.snapshot.revision.saturating_add(1);
        cx.notify();
    }
}

#[cfg(test)]
mod exit_tests {
    use super::wait_session_cleanup;
    use std::time::Duration;

    #[test]
    fn session_ending_preserves_cleanup_error() {
        let worker = std::thread::spawn(|| Err("代理恢复拒绝".into()));
        assert_eq!(
            wait_session_cleanup(worker, Duration::from_secs(1)),
            Err("代理恢复拒绝".into())
        );
    }

    #[test]
    fn session_ending_deadline_does_not_wait_for_stalled_cleanup() {
        let (release, blocked) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _ = blocked.recv();
            Ok(())
        });
        let result = wait_session_cleanup(worker, Duration::from_millis(5));
        release.send(()).unwrap();
        assert_eq!(result, Err("已达到系统结束清理时限".into()));
    }
}

#[cfg(test)]
mod startup_notice_tests {
    use gpui_kit::{
        AppContext, Context, IntoElement, Render, TestAppContext, Window, WindowOptions, div,
    };
    struct Host;
    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }
    #[gpui_kit::test]
    fn first_window_reports_missing_core_and_geodata_once_each(cx: &mut TestAppContext) {
        let root = tempfile::tempdir().unwrap();
        let service = crate::integration::Service::start(
            crate::storage::Repository::open(crate::storage::AppPaths::isolated(root.path()))
                .unwrap(),
        )
        .unwrap();
        cx.update(|cx| {
            crate::integration::init(service, None, cx);
            gpui_kit::init(cx);
            cx.set_reduce_motion(true);
            super::controller(cx).update(cx, |owner, _| {
                owner.snapshot.error = Some("未安装 Mihomo，请前往「内核」下载内核。".into());
                owner.snapshot.notice =
                    Some("缺少 geoip.dat；启动时自动下载，失败请检查网络。".into());
            });
        });
        let (window, _) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| Host)).unwrap()
        });
        window
            .update(cx, |_, window, cx| super::attach_window(window, cx))
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |_, window, cx| {
                assert_eq!(crate::ui::notifications::current(window, cx).len(), 2);
                super::attach_window(window, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |_, window, cx| {
                assert_eq!(crate::ui::notifications::current(window, cx).len(), 2)
            })
            .unwrap();
        crate::integration::finish_test_services(cx);
    }
}

#[cfg(test)]
mod settings_sync_tests {
    use gpui_kit::{AppContext, TestAppContext, WindowOptions, test::TestWindowExt};

    #[gpui_kit::test]
    fn settings_follow_external_commits_preserve_failed_drafts_and_refresh_on_reentry(
        cx: &mut TestAppContext,
    ) {
        let root = tempfile::tempdir().unwrap();
        let service = crate::integration::Service::start(
            crate::storage::Repository::open(crate::storage::AppPaths::isolated(root.path()))
                .unwrap(),
        )
        .unwrap();
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::integration::init(service, None, cx);
            cx.set_reduce_motion(true);
        });
        cx.update(|cx| {
            let preferences =
                super::super::Preferences::load(&crate::storage::AppPaths::isolated(root.path()))
                    .unwrap();
            super::controller(cx).update(cx, |controller, _| {
                controller.snapshot.preferences = Some(preferences);
            });
            assert_eq!(crate::integration::read("tun", cx)["StackIndex"], "Mips");
        });
        let (window, page) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                let session = cx.new(crate::session::Session::new);
                cx.new(|cx| {
                    crate::settings::SettingsPage::new(
                        crate::preferences::SettingsKind::General,
                        session,
                        window,
                        cx,
                    )
                })
            })
            .unwrap()
        });
        cx.run_until_parked();
        cx.update(|cx| {
            super::controller(cx).update(cx, |controller, cx| {
                let mut values =
                    crate::preferences::defaults(crate::preferences::SettingsKind::General);
                values.insert("SilentStart".into(), "true".into());
                controller
                    .snapshot
                    .settings
                    .insert("settings".into(), values);
                cx.notify();
            });
        });
        cx.run_until_parked();
        window
            .update(cx, |_, window, cx| {
                window.render_frame(cx);
                assert_eq!(window.find("setting-SilentStart").checked(), Some(true));
                // The isolated UI executor rejects the change: retain the requested
                // draft instead of replacing it when another committed field changes.
                window.click("setting-SilentStart", cx);
            })
            .unwrap();
        cx.run_until_parked();
        cx.update(|cx| {
            super::controller(cx).update(cx, |controller, cx| {
                controller
                    .snapshot
                    .settings
                    .get_mut("settings")
                    .unwrap()
                    .insert("ProfileUseProxy".into(), "true".into());
                cx.notify();
            });
        });
        cx.run_until_parked();
        window
            .update(cx, |_, window, cx| {
                window.render_frame(cx);
                assert_eq!(window.find("setting-SilentStart").checked(), Some(false));
                // Leaving and re-entering discards the rejected draft and reads the
                // latest committed source for every settings destination.
                page.update(cx, |page, cx| page.reopen(window, cx));
                window.render_frame(cx);
                assert_eq!(window.find("setting-SilentStart").checked(), Some(true));
            })
            .unwrap();
        crate::integration::finish_test_services(cx);
    }
}
