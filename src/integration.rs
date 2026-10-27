//! GPUI bridge: foreground snapshots and callbacks, background durable commands.
use crate::preferences::choices;
mod service;
pub(crate) use crate::storage::{AppPaths, DirectoryKind, Repository};
use crate::{
    storage::{Namespace, Values},
    ui,
};
use gpui_kit::*;
#[cfg(test)]
use service::DocumentWrite;
pub(crate) use service::{Command, Outcome, Service, ServiceError};
use std::{collections::BTreeMap, path::PathBuf};

struct Bridge {
    service: service::Service,
    tasks: BTreeMap<service::RequestId, Task<()>>,
    _quit: Subscription,
}
impl Global for Bridge {}

pub(crate) fn logs(cx: &App) -> crate::logging::Feed {
    cx.global::<Bridge>().service.logs()
}
pub(crate) fn shutdown(cx: &mut App) -> Option<std::thread::JoinHandle<()>> {
    cx.global_mut::<Bridge>().service.shutdown()
}
pub(crate) fn announce(level: &'static str, message: String, cx: &mut App) {
    announce_module(level, "core", message, cx);
}
pub(crate) fn announce_module(
    level: &'static str,
    module: &'static str,
    message: String,
    cx: &mut App,
) {
    if let Err(error) = cx.global_mut::<Bridge>().service.submit(Command::Log {
        level,
        module,
        message,
    }) {
        logs(cx).publish(crate::logging::Record::program(
            crate::logging::Level::Error,
            "logging",
            &format!("程序日志写入请求失败：{error}"),
        ));
    }
}
pub(crate) fn init(service: Service, cx: &mut App) {
    crate::ui::notifications::init(cx);
    if let Some(paths) = service.paths() {
        crate::core::init(paths.clone(), service.logs(), cx);
    } else {
        crate::core::init_unavailable("无法确定应用数据目录".into(), cx);
    }
    let quit = cx.on_app_quit(|cx| {
        let worker = cx.global_mut::<Bridge>().service.shutdown();
        cx.background_spawn(async move {
            if let Some(worker) = worker
                && worker.join().is_err()
            {
                eprintln!("后台存储线程异常退出");
            }
        })
    });
    cx.set_global(Bridge {
        service,
        tasks: BTreeMap::new(),
        _quit: quit,
    });
}
pub(crate) fn read(page: &str, cx: &App) -> Values {
    if matches!(page, "core" | "resources")
        && let Some(preferences) = crate::core::controller(cx)
            .read(cx)
            .snapshot()
            .preferences()
    {
        return preferences.values();
    }
    if (crate::preferences::kind_for_page(page).is_some()
        || matches!(page, "profile-index" | "override-index")
        || page.starts_with("profile-enhance-"))
        && let Some(values) = crate::core::controller(cx)
            .read(cx)
            .snapshot()
            .settings(page)
    {
        return choices::decode(values.clone());
    }
    let Ok(namespace) = Namespace::parse(page) else {
        return Values::new();
    };
    choices::decode(cx.global::<Bridge>().service.read(&namespace))
}
pub(crate) fn document_path(
    overrides: bool,
    id: u64,
    extension: &str,
    cx: &App,
) -> Result<PathBuf, String> {
    let paths = cx
        .global::<Bridge>()
        .service
        .paths()
        .ok_or("无法确定应用数据目录")?;
    Ok(paths
        .data()
        .join(if overrides { "overrides" } else { "profiles" })
        .join(format!("{id}.{extension}")))
}

pub(crate) fn execute(
    command: Command,
    window: &mut Window,
    cx: &mut App,
    complete: impl FnOnce(Result<Outcome, ServiceError>, &mut Window, &mut App) + 'static,
) {
    let label = command.label();
    let module = match &command {
        #[cfg(test)]
        Command::Save { namespace, .. } => match namespace.name() {
            "settings" => "settings",
            _ => "storage",
        },
        Command::Backup(namespace) | Command::Restore(namespace) => match namespace.name() {
            "profile-index" => "profiles",
            "override-index" => "overrides",
            "settings" => "settings",
            _ => "storage",
        },
        Command::Log { module, .. } => module,
        _ => "storage",
    };
    let success_level = match &command {
        #[cfg(test)]
        Command::Save { .. } => "DEBUG",
        Command::OpenDirectory(_) | Command::OpenDocument(_) => "DEBUG",
        _ => "INFO",
    };
    let pending = match cx.global_mut::<Bridge>().service.submit(command) {
        Ok(pending) => pending,
        Err(error) => {
            let handle = window.window_handle();
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, window, cx| {
                    announce_module("ERROR", module, error.to_string(), cx);
                    complete(Err(error), window, cx);
                });
            });
            return;
        }
    };
    let id = pending.id();
    let handle = window.window_handle();
    let task = cx.spawn(async move |cx| {
        let result = pending.receive().await.map(|outcome| match outcome {
            Outcome::Restored(values) => Outcome::Restored(choices::decode(values)),
            other => other,
        });
        cx.update(|cx| match &result {
            Ok(_) => announce_module(success_level, module, format!("{label}已完成"), cx),
            Err(error) => announce_module("ERROR", module, error.to_string(), cx),
        });
        let _ = handle.update(cx, |_, window, cx| complete(result, window, cx));
        cx.update(|cx| {
            if let Some(task) = cx.global_mut::<Bridge>().tasks.remove(&id) {
                task.detach();
            }
        });
    });
    cx.global_mut::<Bridge>().tasks.insert(id, task);
}
#[cfg(test)]
fn save(
    page: &str,
    values: Values,
    documents: Vec<DocumentWrite>,
    window: &mut Window,
    cx: &mut App,
    complete: impl FnOnce(Result<(), String>, &mut Window, &mut App) + 'static,
) {
    let namespace = match Namespace::parse(page) {
        Ok(namespace) => namespace,
        Err(error) => {
            let handle = window.window_handle();
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, window, cx| {
                    complete(Err(error.to_string()), window, cx)
                });
            });
            return;
        }
    };
    execute(
        Command::Save {
            namespace,
            values: choices::encode(values),
            documents,
        },
        window,
        cx,
        move |result, w, cx| {
            complete(result.map(|_| ()).map_err(|error| error.to_string()), w, cx);
        },
    );
}
// Transitional adapter for feature commands awaiting their domain implementation.
// Requests fail explicitly instead of accumulating in an unconsumed queue.
pub(crate) fn request(command: &str, window: &mut Window, cx: &mut App) {
    request_with(command, BTreeMap::new(), window, cx);
}
pub(crate) fn request_with(
    command: &str,
    _parameters: BTreeMap<String, String>,
    window: &mut Window,
    cx: &mut App,
) {
    let kind = match command {
        "打开配置目录" => Some(DirectoryKind::Config),
        "打开内核目录" => Some(DirectoryKind::Core),
        "打开日志目录" => Some(DirectoryKind::Logs),
        "打开备份目录" => Some(DirectoryKind::Backups),
        _ => None,
    };
    if let Some(kind) = kind {
        execute(
            Command::OpenDirectory(kind),
            window,
            cx,
            |result, w, cx| match result {
                Ok(Outcome::Directory(path)) => cx.open_with_system(&path),
                Err(error) => ui::show_error(error.to_string(), w, cx),
                _ => {}
            },
        );
    } else {
        let message = format!("{command}尚未接入实际执行逻辑");
        announce_module("WARNING", "application", message.clone(), cx);
        crate::ui::notifications::push(window, message, cx);
    }
}

/// Test contexts do not perform the native app's awaited quit flow. Stop the
/// actual workers before destroying their foreground dispatcher or temp storage.
#[cfg(test)]
pub(crate) fn finish_test_services(cx: &mut gpui_kit::TestAppContext) {
    let core = cx.update(|cx| {
        crate::core::controller(cx).update(cx, |owner, _| owner.shutdown_test_worker())
    });
    if let Some(worker) = core {
        worker.join().unwrap().unwrap();
    }
    let storage = cx.update(|cx| cx.global_mut::<Bridge>().service.shutdown());
    if let Some(worker) = storage {
        worker.join().unwrap();
    }
    cx.run_until_parked();
}

#[cfg(test)]
mod tests {
    use super::{AppPaths, Bridge, Repository, Service, Values, init, read, save};
    use gpui_kit::{Context, IntoElement, Render, TestAppContext, Window, div, px, size};
    use std::{cell::Cell, rc::Rc};
    struct Host;
    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }
    #[gpui_kit::test]
    fn rejected_submission_completes_after_the_submitting_update(cx: &mut TestAppContext) {
        let root = tempfile::tempdir().unwrap();
        let service =
            Service::start(Repository::open(AppPaths::isolated(root.path())).unwrap()).unwrap();
        cx.update(|cx| {
            gpui_kit::init(cx);
            init(service, cx);
        });
        let window = cx.open_window(size(px(400.), px(300.)), |_, _| Host);
        cx.update(|cx| cx.global_mut::<Bridge>().service.shutdown())
            .unwrap()
            .join()
            .unwrap();
        let complete = Rc::new(Cell::new(false));
        window
            .update(cx, |_, window, cx| {
                let received = complete.clone();
                save(
                    "settings",
                    Values::new(),
                    Vec::new(),
                    window,
                    cx,
                    move |result, _, _| {
                        assert!(result.is_err());
                        received.set(true);
                    },
                );
                assert!(
                    !complete.get(),
                    "completion must not re-enter the submitting entity"
                );
            })
            .unwrap();
        cx.run_until_parked();
        assert!(complete.get());
        super::finish_test_services(cx);
    }
    #[gpui_kit::test]
    fn successful_save_updates_the_committed_cache_and_foreground_callback(
        cx: &mut TestAppContext,
    ) {
        let root = tempfile::tempdir().unwrap();
        let service =
            Service::start(Repository::open(AppPaths::isolated(root.path())).unwrap()).unwrap();
        cx.update(|cx| {
            gpui_kit::init(cx);
            init(service, cx);
        });
        let window = cx.open_window(size(px(400.), px(300.)), |_, _| Host);
        let complete = Rc::new(Cell::new(false));
        let values = Values::from([("ThemeIndex".into(), "浅色".into())]);
        window
            .update(cx, |_, window, cx| {
                let received = complete.clone();
                save(
                    "settings",
                    values.clone(),
                    Vec::new(),
                    window,
                    cx,
                    move |result, _, _| {
                        assert!(result.is_ok());
                        received.set(true);
                    },
                );
            })
            .unwrap();
        cx.update(|cx| cx.global_mut::<Bridge>().service.shutdown())
            .unwrap()
            .join()
            .unwrap();
        cx.run_until_parked();
        assert!(complete.get());
        assert_eq!(cx.update(|cx| read("settings", cx)), values);
        super::finish_test_services(cx);
    }
}
