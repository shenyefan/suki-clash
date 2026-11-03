//! Application update preparation shares the lifecycle's single command owner.
use super::service::Actor;
#[derive(Clone)]
pub(crate) enum Command {
    Check,
    Download { expected: String },
}
impl Actor {
    pub(super) async fn app_update(
        &mut self,
        command: Command,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> Result<(), String> {
        self.snapshot.operation_module = Some("updater");
        match command {
            Command::Check => {
                self.progress("检查应用更新");
                let release = tokio::select! { biased;
                    _=shutdown.changed()=>return Err("应用退出，更新检查已取消".into()),
                    result=crate::updates::check(&self.http,&self.paths)=>result?,
                };
                let release = match release {
                    crate::updates::Check::Managed(release) => {
                        self.snapshot.app_update.manual_url = None;
                        release
                    }
                    crate::updates::Check::Manual => {
                        self.snapshot.app_update = crate::updates::State {
                            checked: true,
                            manual_url: Some(crate::updates::manual_releases_url()?),
                            ..Default::default()
                        };
                        self.app_release = None;
                        return Ok(());
                    }
                };
                self.snapshot.app_update.downloaded = release
                    .as_ref()
                    .filter(|release| release.staged)
                    .map(|release| release.version().into());
                self.snapshot.app_update.available =
                    release.as_ref().map(|release| release.version().into());
                self.snapshot.app_update.checked = true;
                self.app_release = release.map(|release| *release);
                self.completed(
                    if self.snapshot.app_update.available.is_some() {
                        "发现应用更新"
                    } else {
                        "当前已是最新版本"
                    }
                    .into(),
                );
            }
            Command::Download { expected } => {
                let release = self
                    .app_release
                    .as_ref()
                    .filter(|release| release.version() == expected)
                    .ok_or("应用更新信息已变化，请重新检查")?;
                if self.snapshot.app_update.downloaded.as_deref() == Some(expected.as_str()) {
                    return Ok(());
                }
                // Create progress without borrowing all of the actor while the retained release is in use.
                self.snapshot.progress = Some(super::TransferProgress::pending("下载应用更新"));
                let snapshot = &mut self.snapshot;
                let updates = &self.updates;
                let observer = &self.observer;
                release
                    .download(&self.http, shutdown, |received, total| {
                        if let Some(progress) = &mut snapshot.progress {
                            progress.received = received;
                            progress.total = Some(total);
                        }
                        snapshot.revision = snapshot.revision.saturating_add(1);
                        Self::send_snapshot(snapshot, updates, observer);
                    })
                    .await?;
                self.snapshot.app_update.downloaded = Some(expected);
                self.snapshot.operation_committed = true;
                self.completed("应用更新已下载，可安装并重启".into());
            }
        }
        Ok(())
    }
}
