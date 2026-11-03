//! Desktop integration owns OS launch behavior and instance lifetime.
pub(crate) mod autostart;
pub(crate) mod instance;
#[cfg(not(target_os = "macos"))]
pub(crate) mod scheme;

pub(crate) async fn startup_script(paths: &crate::storage::AppPaths) -> Result<(), String> {
    let repository =
        crate::storage::Repository::open(paths.clone()).map_err(|error| error.to_string())?;
    let values = crate::preferences::values(&repository, crate::preferences::SettingsKind::General)
        .map_err(|error| error.to_string())?;
    let Some(path) = values
        .get("StartupScript")
        .filter(|value| !value.trim().is_empty())
    else {
        return Ok(());
    };
    let path = std::path::Path::new(path);
    if !path.is_absolute() || !path.is_file() {
        return Err("启动脚本需为存在的绝对文件路径".into());
    }
    let extension = path
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or_default();
    let mut command = match extension {
        "sh" => {
            let mut c = tokio::process::Command::new("bash");
            c.arg(path);
            c
        }
        "ps1" => {
            let mut c =
                tokio::process::Command::new(if cfg!(windows) { "powershell" } else { "pwsh" });
            c.args(["-NoProfile", "-File"]).arg(path);
            c
        }
        "bat" if cfg!(windows) => {
            let mut c = tokio::process::Command::new("cmd");
            c.args(["/C"]).arg(path);
            c
        }
        _ => return Err("启动脚本仅支持 .sh、.ps1 和 Windows .bat".into()),
    };
    let mut child = command
        .current_dir(path.parent().ok_or("启动脚本目录无效")?)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| format!("无法执行启动脚本：{error}"))?;
    match tokio::time::timeout(std::time::Duration::from_secs(60), child.wait()).await {
        Ok(Ok(status)) if status.success() => Ok(()),
        Ok(Ok(status)) => Err(format!("启动脚本未完成（{status}）")),
        Ok(Err(error)) => Err(format!("启动脚本执行失败：{error}")),
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            Err("启动脚本执行超时，已终止".into())
        }
    }
}

use gpui_kit::*;
struct Instance {
    service: instance::Service,
    main: WindowHandle<gpui_kit::base::Root>,
    app: Entity<crate::app::SukiApp>,
    _task: Task<()>,
    _quit: Subscription,
}
impl Global for Instance {}
pub(crate) fn shutdown(cx: &mut App) -> Option<std::thread::JoinHandle<()>> {
    cx.try_global::<Instance>()?;
    cx.global_mut::<Instance>().service.shutdown()
}
/// Reinstall the instance endpoint when spawning the replacement application failed.
pub(crate) fn recover(service: instance::Service, cx: &mut App) {
    let instance = cx.global::<Instance>();
    let main = instance.main;
    let app = instance.app.clone();
    install(service, main, app, cx);
}
/// One foreground entry for Dock/Finder reopen, tray and single-instance activation.
pub(crate) fn show_main(main: AnyWindowHandle, cx: &mut App) {
    let _ = main.update(cx, |_, window, cx| {
        window.activate_window();
        cx.activate(true);
    });
}
pub(crate) fn reopen(cx: &mut App) {
    if let Some(instance) = cx.try_global::<Instance>() {
        show_main(instance.main.into(), cx);
    }
}
pub(crate) fn install(
    service: instance::Service,
    main: WindowHandle<gpui_kit::base::Root>,
    app: Entity<crate::app::SukiApp>,
    cx: &mut App,
) {
    let receiver = service.events.clone();
    let application = app.clone();
    let task = cx.spawn(async move |cx| {
        while let Ok(activation) = receiver.recv().await {
            let app = app.clone();
            cx.update(|cx| show_main(main.into(), cx));
            let _ = main.update(cx, |_, window, cx| {
                if let Some(uri) = activation.uri {
                    app.update(cx, |app, cx| app.import_uri(&uri, window, cx));
                }
            });
        }
    });
    let quit = cx.on_app_quit(|cx| {
        let worker = cx.global_mut::<Instance>().service.shutdown();
        cx.background_spawn(async move {
            if let Some(worker) = worker {
                let _ = worker.join();
            }
        })
    });
    cx.set_global(Instance {
        service,
        main,
        app: application,
        _task: task,
        _quit: quit,
    });
}
