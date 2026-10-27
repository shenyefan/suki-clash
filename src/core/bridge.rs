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
        self.pending.is_some() || self.snapshot.is_busy()
    }
}
struct Handle(Entity<Controller>);
impl Global for Handle {}

pub(crate) fn controller(cx: &App) -> Entity<Controller> {
    cx.global::<Handle>().0.clone()
}

/// Register the first window before asynchronous startup results arrive. A
/// failure produced before window construction is replayed once as a toast.
pub(crate) fn attach_window(window: &Window, cx: &mut App) {
    let handle = window.window_handle();
    let report = controller(cx).update(cx, |this, _| {
        let first = this.notification_window.is_none();
        this.notification_window = Some(handle);
        first
            .then(|| {
                this.snapshot
                    .error
                    .clone()
                    .map(|message| (true, message))
                    .or_else(|| this.snapshot.notice.clone().map(|message| (false, message)))
            })
            .flatten()
    });
    cx.defer(|cx| {
        controller(cx).update(cx, |this, cx| this.sync_progress(cx));
    });
    if let Some((error, message)) = report {
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, cx| {
                if error {
                    crate::ui::show_error(message, window, cx);
                } else {
                    crate::ui::notifications::push(window, message, cx);
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
                                (&runtime.storage_error, &this.runtime.storage_error, "ERROR"),
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
                                        crate::ui::notifications::push(window, message, cx);
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
            cx.background_spawn(async move {
                if let Some(worker) = worker {
                    let _ = worker.join();
                }
            })
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
    let storage_worker = crate::integration::shutdown(cx);
    let owner = controller(cx);
    owner.update(cx, |this, cx| {
        if this.quitting.is_some() {
            return;
        }
        let worker = this.service.as_mut().and_then(Service::shutdown);
        this.quitting = Some(cx.spawn(async move |owner, cx| {
            let joined = cx
                .background_spawn(async move {
                    let storage_failed =
                        storage_worker.is_some_and(|worker| worker.join().is_err());
                    let core_result = match worker {
                        Some(worker) => worker
                            .join()
                            .unwrap_or_else(|_| Err("内核清理线程异常退出".into())),
                        None => Ok(()),
                    };
                    if storage_failed {
                        Err(format!(
                            "存储清理线程异常退出；内核清理结果：{core_result:?}"
                        ))
                    } else {
                        core_result
                    }
                })
                .await;
            if let Err(error) = joined {
                let _ = owner.update(cx, |this, cx| {
                    this.snapshot.phase = Phase::Failed;
                    this.snapshot.error = Some(format!("退出清理未完成：{error}"));
                    if let Some(task) = this.quitting.take() {
                        task.detach();
                    }
                    cx.notify();
                });
                cx.update(|cx| {
                    crate::integration::announce("ERROR", format!("退出清理未完成：{error}"), cx);
                    if let Some(window) = cx.active_window() {
                        let _ = window.update(cx, |_, window, cx| {
                            crate::ui::show_error(format!("退出清理未完成：{error}"), window, cx)
                        });
                    }
                });
                return;
            }
            cx.update(|cx| cx.quit());
        }));
    });
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
