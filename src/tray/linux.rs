//! StatusNotifierItem icon with the protocol's ContextMenu callback.
//! ksni deliberately rejects ContextMenu, so it cannot host an application popup.
use super::platform::Event;
use futures_util::StreamExt;
use gpui_kit::{App, AppContext as _, Task};
use tray_icon::{MouseButton, MouseButtonState, TrayIconEvent, dpi::PhysicalPosition};
use zbus::{OwnedObjectPath, object_server::SignalEmitter};

#[derive(Clone)]
pub(super) struct Icon(Vec<u8>);
impl Icon {
    pub(super) fn from_rgba(mut bytes: Vec<u8>, _: u32, _: u32) -> Result<Self, String> {
        for pixel in bytes.chunks_exact_mut(4) {
            pixel.rotate_right(1);
        }
        Ok(Self(bytes))
    }
}
enum Update {
    Icon(Icon),
    Tooltip(String),
}
pub(super) struct Native {
    sender: async_channel::Sender<Update>,
    _task: Task<()>,
}
impl Native {
    pub(super) fn new(icon: Icon, events: async_channel::Sender<Event>, cx: &App) -> Self {
        let (sender, receiver) = async_channel::unbounded();
        let task = cx.background_spawn(async move {
            if let Err(error) = run(icon, events.clone(), receiver).await {
                let _ = events
                    .send(Event::Error(format!("托盘连接：{error}")))
                    .await;
            }
        });
        Self {
            sender,
            _task: task,
        }
    }
    pub(super) fn set_icon(&self, icon: Option<Icon>) -> Result<(), String> {
        if let Some(icon) = icon {
            self.sender
                .try_send(Update::Icon(icon))
                .map_err(crate::logging::error_message)?;
        }
        Ok(())
    }
    pub(super) fn set_tooltip(&self, value: Option<&str>) -> Result<(), String> {
        self.sender
            .try_send(Update::Tooltip(value.unwrap_or_default().into()))
            .map_err(crate::logging::error_message)
    }
    pub(super) fn set_title(&self, _: Option<&str>) {}
}
struct Item {
    icon: Icon,
    tooltip: String,
    events: async_channel::Sender<Event>,
}
impl Item {
    fn click(&self, button: MouseButton, x: i32, y: i32) {
        let _ = self.events.try_send(Event::Icon(TrayIconEvent::Click {
            id: "suki-clash".into(),
            position: PhysicalPosition::new(x as f64, y as f64),
            rect: Default::default(),
            button,
            button_state: MouseButtonState::Up,
        }));
    }
}
#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl Item {
    fn context_menu(&self, x: i32, y: i32) {
        self.click(MouseButton::Right, x, y);
    }
    fn activate(&self, x: i32, y: i32) {
        self.click(MouseButton::Left, x, y);
    }
    fn secondary_activate(&self, x: i32, y: i32) {
        self.click(MouseButton::Middle, x, y);
    }
    fn scroll(&self, _delta: i32, _orientation: &str) {}
    #[zbus(property)]
    fn category(&self) -> &str {
        "ApplicationStatus"
    }
    #[zbus(property)]
    fn id(&self) -> &str {
        "suki-clash"
    }
    #[zbus(property)]
    fn title(&self) -> &str {
        "Suki Clash"
    }
    #[zbus(property)]
    fn status(&self) -> &str {
        "Active"
    }
    #[zbus(property)]
    fn window_id(&self) -> u32 {
        0
    }
    #[zbus(property)]
    fn icon_name(&self) -> &str {
        ""
    }
    #[zbus(property)]
    fn icon_pixmap(&self) -> Vec<(i32, i32, Vec<u8>)> {
        vec![(32, 32, self.icon.0.clone())]
    }
    #[zbus(property)]
    fn overlay_icon_name(&self) -> &str {
        ""
    }
    #[zbus(property)]
    fn overlay_icon_pixmap(&self) -> Vec<(i32, i32, Vec<u8>)> {
        vec![]
    }
    #[zbus(property)]
    fn attention_icon_name(&self) -> &str {
        ""
    }
    #[zbus(property)]
    fn attention_icon_pixmap(&self) -> Vec<(i32, i32, Vec<u8>)> {
        vec![]
    }
    #[zbus(property)]
    fn attention_movie_name(&self) -> &str {
        ""
    }
    #[zbus(property)]
    fn tool_tip(&self) -> (String, Vec<(i32, i32, Vec<u8>)>, String, String) {
        (
            String::new(),
            vec![],
            "Suki Clash".into(),
            self.tooltip.clone(),
        )
    }
    #[zbus(property)]
    fn item_is_menu(&self) -> bool {
        false
    }
    // No DBusMenu: ContextMenu is rendered in the GPUI window.
    #[zbus(property)]
    fn menu(&self) -> OwnedObjectPath {
        OwnedObjectPath::default()
    }
    #[zbus(signal)]
    async fn new_icon(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn new_tool_tip(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}
async fn run(
    icon: Icon,
    events: async_channel::Sender<Event>,
    receiver: async_channel::Receiver<Update>,
) -> zbus::Result<()> {
    let connection = zbus::connection::Builder::session()
        .serve_at(
            "/StatusNotifierItem",
            Item {
                icon,
                tooltip: "Suki Clash".into(),
                events,
            },
        )
        .build()
        .await?;
    let watcher = zbus::Proxy::new(
        &connection,
        "org.kde.StatusNotifierWatcher",
        "/StatusNotifierWatcher",
        "org.kde.StatusNotifierWatcher",
    )
    .await?;
    let dbus = zbus::fdo::DBusProxy::new(&connection).await?;
    let mut owners = dbus
        .receive_name_owner_changed_with_args(&[(0, "org.kde.StatusNotifierWatcher")])
        .await?;
    let service = connection
        .unique_name()
        .map(|n| n.as_str())
        .unwrap_or_default();
    if dbus
        .name_has_owner("org.kde.StatusNotifierWatcher".try_into()?)
        .await?
    {
        watcher
            .call::<_, _, ()>("RegisterStatusNotifierItem", &(service,))
            .await?;
    }
    let interface = connection
        .object_server()
        .interface::<_, Item>("/StatusNotifierItem")
        .await?;
    loop {
        match futures_util::future::select(Box::pin(receiver.recv()), Box::pin(owners.next())).await
        {
            futures_util::future::Either::Left((Ok(update), _)) => {
                let mut item = interface.get_mut().await;
                match update {
                    Update::Icon(icon) => {
                        item.icon = icon;
                        Item::new_icon(interface.signal_emitter()).await?;
                    }
                    Update::Tooltip(tooltip) => {
                        item.tooltip = tooltip;
                        Item::new_tool_tip(interface.signal_emitter()).await?;
                    }
                }
            }
            futures_util::future::Either::Right((Some(signal), _)) => {
                if signal.args()?.new_owner().is_some() {
                    watcher
                        .call::<_, _, ()>("RegisterStatusNotifierItem", &(service,))
                        .await?;
                }
            }
            _ => break,
        }
    }
    Ok(())
}
