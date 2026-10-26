//! GPUI bridge: foreground snapshots and callbacks, background durable commands.
mod service;
pub(crate) use crate::storage::{AppPaths, DirectoryKind, Repository};
use crate::{
    storage::{Namespace, Values},
    ui,
};
use gpui_kit::{component::WindowExt, *};
pub(crate) use service::{Command, DocumentWrite, Outcome, Service, ServiceError};
use std::{collections::BTreeMap, path::PathBuf};

struct Bridge {
    service: service::Service,
    tasks: BTreeMap<service::RequestId, Task<()>>,
    _quit: Subscription,
    notices: Entity<NoticeSource>,
}
impl Global for Bridge {}

pub(crate) struct Notice {
    time: String,
    level: &'static str,
    message: String,
}
impl Notice {
    pub(crate) fn time(&self) -> &str {
        &self.time
    }
    pub(crate) fn level(&self) -> &str {
        self.level
    }
    pub(crate) fn message(&self) -> &str {
        &self.message
    }
}
pub(crate) struct NoticeSource;
impl EventEmitter<Notice> for NoticeSource {}
pub(crate) fn notices(cx: &App) -> Entity<NoticeSource> {
    cx.global::<Bridge>().notices.clone()
}
fn announce(level: &'static str, message: String, cx: &mut App) {
    let source = notices(cx);
    source.update(cx, |_, cx| {
        cx.emit(Notice {
            time: chrono::Local::now().format("%H:%M:%S").to_string(),
            level,
            message,
        })
    });
}
pub(crate) fn init(service: Service, cx: &mut App) {
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
    let notices = cx.new(|_| NoticeSource);
    cx.set_global(Bridge {
        service,
        tasks: BTreeMap::new(),
        _quit: quit,
        notices,
    });
}
pub(crate) fn read(page: &str, cx: &App) -> Values {
    let Ok(namespace) = Namespace::parse(page) else {
        return Values::new();
    };
    cx.global::<Bridge>().service.read(&namespace)
}
pub(crate) fn document_path(overrides: bool, id: u64, extension: &str, cx: &App) -> PathBuf {
    cx.global::<Bridge>()
        .service
        .paths()
        .data()
        .join(if overrides { "overrides" } else { "profiles" })
        .join(format!("{id}.{extension}"))
}
pub(crate) fn execute(
    command: Command,
    window: &mut Window,
    cx: &mut App,
    complete: impl FnOnce(Result<Outcome, ServiceError>, &mut Window, &mut App) + 'static,
) {
    let label = command.label();
    let pending = match cx.global_mut::<Bridge>().service.submit(command) {
        Ok(pending) => pending,
        Err(error) => {
            let handle = window.window_handle();
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, window, cx| {
                    announce("ERROR", format!("{label}失败：{error}"), cx);
                    complete(Err(error), window, cx);
                });
            });
            return;
        }
    };
    let id = pending.id();
    let handle = window.window_handle();
    let task = cx.spawn(async move |cx| {
        let result = pending.receive().await;
        cx.update(|cx| match &result {
            Ok(_) => announce("INFO", format!("{label}已完成"), cx),
            Err(error) => announce("ERROR", format!("{label}失败：{error}"), cx),
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
pub(crate) fn save(
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
            values,
            documents,
        },
        window,
        cx,
        move |result, w, cx| {
            complete(result.map(|_| ()).map_err(|error| error.to_string()), w, cx);
        },
    );
}
pub(crate) fn save_draft<T: 'static, R: PartialEq + 'static>(
    page: &str,
    values: Values,
    owner: WeakEntity<T>,
    revision: R,
    read_revision: impl FnOnce(&T, &App) -> R + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    let submit_focus = window.focused(cx);
    save(
        page,
        values,
        Vec::new(),
        window,
        cx,
        move |result, w, cx| match result {
            Ok(()) => {
                if submit_focus
                    .as_ref()
                    .is_some_and(|focus| focus.is_focused(w))
                    && owner
                        .upgrade()
                        .is_some_and(|entity| read_revision(entity.read(cx), cx) == revision)
                {
                    w.close_dialog(cx);
                }
            }
            Err(error) => ui::show_error(error, w, cx),
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
        announce("WARNING", message.clone(), cx);
        window.push_notification(message, cx);
    }
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
            Service::start(Repository::open(AppPaths::isolated(root.path()), None).unwrap())
                .unwrap();
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
    }
    #[gpui_kit::test]
    fn successful_save_updates_the_committed_cache_and_foreground_callback(
        cx: &mut TestAppContext,
    ) {
        let root = tempfile::tempdir().unwrap();
        let service =
            Service::start(Repository::open(AppPaths::isolated(root.path()), None).unwrap())
                .unwrap();
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
    }
}
