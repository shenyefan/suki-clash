//! Retained editors for network inventory and native TCP/UDP tunnels.
use super::SettingsPage;
use crate::{
    configuration::tunnels::{self, Tunnel},
    core,
    session::Session,
    storage::Values,
    ui,
    ui::theme::{control, layout, space, text},
};
use gpui_kit::{
    assets::IconName,
    component::{
        Disableable, IndexPath, Sizable, WindowExt,
        button::{Button, ButtonVariants},
        h_flex,
        input::{Input, InputState},
        select::{Select, SelectState},
        v_flex,
    },
    *,
};
impl SettingsPage {
    pub(super) fn network_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let session = self.session.clone();
        let inventory = cx.new(|cx| Inventory::new(session, cx));
        core::request(core::Command::Interfaces, window, cx);
        window.open_dialog(cx, move |dialog, w, cx| {
            let inventory = inventory.clone();
            ui::modal(dialog, "网卡信息", layout::DIALOG_WIDTH, w, cx)
                .content(ui::modal_panel(layout::DIALOG_FORM_HEIGHT, move |_, _| {
                    inventory.clone().into_any_element()
                }))
                .footer(ui::modal_close("关闭"))
        });
    }
    pub(super) fn edit_tunnel(
        &mut self,
        previous: Option<Tunnel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let session = self.session.clone();
        let editor = cx.new(|cx| TunnelEditor::new(previous.clone(), session, window, cx));
        if self.session.read(cx).is_running() && !self.session.read(cx).core_busy() {
            core::request(core::Command::ProxyNames, window, cx);
        }
        let footer = cx.new(|cx| TunnelFooter::new(editor.clone(), cx));
        window.open_dialog(cx, move |dialog, w, cx| {
            let editor = editor.clone();
            ui::modal(
                dialog,
                if previous.is_some() {
                    "编辑隧道"
                } else {
                    "新增隧道"
                },
                layout::DIALOG_WIDTH,
                w,
                cx,
            )
            .content(ui::modal_content(
                "tunnel-editor",
                layout::DIALOG_FORM_HEIGHT,
                move |_, _| editor.clone().into_any_element(),
            ))
            .footer(footer.clone())
        });
    }
    pub(super) fn remove_tunnel(
        &mut self,
        entry: Tunnel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match core::submit(
            core::Command::Tunnel(tunnels::Change::Remove(entry)),
            window,
            cx,
        ) {
            Ok(id) => {
                self.saving = true;
                self.core_pending = Some((
                    id,
                    Values::from([("Tunnels".into(), String::new())]),
                    window.window_handle(),
                ));
            }
            Err(error) => ui::show_error(error, window, cx),
        }
        cx.notify();
    }
    pub(super) fn tunnel_rows(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let values = crate::integration::read("core", cx);
        let busy = self.session.read(cx).core_busy() || self.saving;
        let entries =
            match tunnels::parse(values.get("Tunnels").map(String::as_str).unwrap_or("[]")) {
                Ok(entries) => entries,
                Err(_) => {
                    return vec![
                        ui::data_preference_row(
                            "隧道",
                            "配置无法读取，请检查 tunnels 字段",
                            h_flex().gap(space::SM).child(
                                ui::icon_button("add-runtime-tunnel", IconName::Plus, "新增隧道")
                                    .disabled(true),
                            ),
                            cx,
                        )
                        .into_any_element(),
                    ];
                }
            };
        let mut rows = Vec::new();
        if !entries.is_empty() {
            let height = crate::ui::theme::list::DETAIL_ROW_HEIGHT.to_pixels(window.rem_size());
            let count = entries.len();
            let entries = std::rc::Rc::new(entries);
            rows.push(ui::virtual_rows::panel(
                cx.entity(),
                "tunnel-rows",
                vec![height; count],
                &self.lists["tunnels"],
                move |_this, index, _, cx| {
                    let entry = &entries[index];
                    let identity = format!("{}-{}", entry.address, entry.network.join("+"));
                    let edit = entry.clone();
                    let remove = entry.clone();
                    let controls = h_flex()
                        .gap(space::SM)
                        .child(
                            ui::icon_button(
                                SharedString::from(format!("edit-tunnel-{identity}")),
                                IconName::Pencil,
                                "编辑隧道",
                            )
                            .disabled(busy)
                            .on_click(cx.listener(
                                move |this, _, w, cx| this.edit_tunnel(Some(edit.clone()), w, cx),
                            )),
                        )
                        .child(
                            ui::icon_button(
                                SharedString::from(format!("remove-tunnel-{identity}")),
                                IconName::Trash,
                                "删除隧道",
                            )
                            .disabled(busy)
                            .on_click(cx.listener(
                                move |this, _, w, cx| this.remove_tunnel(remove.clone(), w, cx),
                            )),
                        );
                    ui::data_preference_row(
                        format!("{} → {}", entry.address, entry.target),
                        format!(
                            "{} · {}",
                            entry.network.join("+"),
                            entry.proxy.as_deref().unwrap_or("DIRECT")
                        ),
                        controls,
                        cx,
                    )
                    .into_any_element()
                },
            ));
        }
        rows.push(
            ui::data_preference_row(
                "隧道",
                "将本地 TCP / UDP 端口转发到目标",
                h_flex().gap(space::SM).child(
                    ui::icon_button("add-runtime-tunnel", IconName::Plus, "新增隧道")
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, w, cx| this.edit_tunnel(None, w, cx))),
                ),
                cx,
            )
            .into_any_element(),
        );
        rows
    }
}
struct Inventory {
    session: Entity<Session>,
    ipv6: bool,
    list_scroll: gpui_kit::component::VirtualListScrollHandle,
    _subscription: Subscription,
}
impl Inventory {
    fn new(session: Entity<Session>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&session, |_, _, cx| cx.notify());
        Self {
            session,
            ipv6: false,
            list_scroll: gpui_kit::component::VirtualListScrollHandle::new(),
            _subscription: subscription,
        }
    }
}
impl Render for Inventory {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = self.session.read(cx).core();
        let busy = self.session.read(cx).core_busy();
        let mut body = v_flex()
            .size_full()
            .min_h(space::NONE)
            .gap(space::LG)
            .child(
                h_flex()
                    .flex_shrink_0()
                    .gap(space::SM)
                    .child(
                        Button::new("interface-family")
                            .icon(gpui_kit::assets::IconName::Network)
                            .outline()
                            .label(if self.ipv6 { "IPv6" } else { "IPv4" })
                            .with_size(control::STANDARD)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.ipv6 = !this.ipv6;
                                cx.notify();
                            })),
                    )
                    .child(
                        ui::icon_button("refresh-interfaces", IconName::RefreshCw, "刷新网卡信息")
                            .disabled(busy)
                            .on_click(|_, w, cx| core::request(core::Command::Interfaces, w, cx)),
                    ),
            );
        if snapshot.interfaces().is_empty() {
            return body.child(ui::empty_inline(
                IconName::Network,
                if busy {
                    "正在读取网卡"
                } else {
                    "暂无网卡信息"
                },
                "可刷新重新读取系统网卡",
                cx,
            ));
        }
        let mut entries = Vec::new();
        for interface in snapshot.interfaces() {
            entries.push((interface.name.clone(), None));
            for address in interface
                .addresses
                .iter()
                .filter(|address| address.contains(':') == self.ipv6)
            {
                entries.push((
                    interface.name.clone(),
                    Some((if self.ipv6 { "IPv6" } else { "IPv4" }, address.clone())),
                ));
            }
            entries.push((interface.name.clone(), Some(("MAC", interface.mac.clone()))));
        }
        let height = crate::ui::theme::list::DETAIL_ROW_HEIGHT.to_pixels(window.rem_size());
        let heights = entries
            .iter()
            .map(|(_, value)| {
                if value.is_none() {
                    space::REGION.to_pixels(window.rem_size())
                } else {
                    height
                }
            })
            .collect();
        let entries = std::rc::Rc::new(entries);
        body = body.child(
            div()
                .flex_1()
                .min_h(space::NONE)
                .child(ui::virtual_rows::rows(
                    cx.entity(),
                    "interface-rows",
                    heights,
                    &self.list_scroll,
                    move |_, index, _, cx| {
                        let (name, value) = &entries[index];
                        let Some((label, value)) = value else {
                            return ui::section(name.clone(), cx)
                                .px(space::LG)
                                .into_any_element();
                        };
                        let copy = value.clone();
                        ui::data_preference_row(
                            *label,
                            "",
                            h_flex()
                                .gap(space::SM)
                                .child(if value.is_empty() {
                                    "—".into()
                                } else {
                                    value.clone()
                                })
                                .child(
                                    ui::icon_button(
                                        SharedString::from(format!(
                                            "copy-interface-{name}-{label}-{value}"
                                        )),
                                        IconName::Copy,
                                        "复制地址",
                                    )
                                    .disabled(value.is_empty())
                                    .on_click(
                                        move |_, _, cx| {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                copy.clone(),
                                            ))
                                        },
                                    ),
                                ),
                            cx,
                        )
                        .into_any_element()
                    },
                )),
        );
        body
    }
}
struct TunnelEditor {
    session: Entity<Session>,
    previous: Option<Tunnel>,
    local_host: Entity<InputState>,
    local_port: Entity<InputState>,
    target_host: Entity<InputState>,
    target_port: Entity<InputState>,
    transport: Entity<SelectState<Vec<SharedString>>>,
    proxy: Entity<SelectState<Vec<SharedString>>>,
    proxy_names: Vec<String>,
    pending: Option<u64>,
    _subscription: Subscription,
}
impl TunnelEditor {
    fn new(
        previous: Option<Tunnel>,
        session: Entity<Session>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscription = cx.observe_in(&session, window, |this, _, window, cx| {
            this.sync(window, cx);
            cx.notify();
        });
        fn split(address: &str) -> (String, String) {
            address
                .rsplit_once(':')
                .map(|(host, port)| {
                    (
                        host.trim_start_matches('[').trim_end_matches(']').into(),
                        port.into(),
                    )
                })
                .unwrap_or_default()
        }
        let (listen_host, listen_port) = previous
            .as_ref()
            .map(|entry| split(&entry.address))
            .unwrap_or_else(|| ("127.0.0.1".into(), String::new()));
        let (remote_host, remote_port) = previous
            .as_ref()
            .map(|entry| split(&entry.target))
            .unwrap_or_default();
        let local_host = cx.new(|cx| InputState::new(window, cx).default_value(listen_host));
        let local_port = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(listen_port)
                .placeholder("1080")
        });
        let target_host = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(remote_host)
                .placeholder("example.com")
        });
        let target_port = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(remote_port)
                .placeholder("443")
        });
        let transport = cx.new(|cx| {
            SelectState::new(
                vec!["tcp+udp".into(), "tcp".into(), "udp".into()],
                Some(IndexPath::new(0)),
                window,
                cx,
            )
        });
        let proxy = cx
            .new(|cx| SelectState::new(vec!["DIRECT".into()], Some(IndexPath::new(0)), window, cx));
        let mut editor = Self {
            session,
            previous,
            local_host,
            local_port,
            target_host,
            target_port,
            transport,
            proxy,
            proxy_names: Vec::new(),
            pending: None,
            _subscription: subscription,
        };
        editor.sync(window, cx);
        if let Some(entry) = &editor.previous {
            let network = SharedString::from(if entry.network.len() == 2 {
                "tcp+udp".into()
            } else {
                entry.network.join("+")
            });
            editor.transport.update(cx, |state, cx| {
                state.set_selected_value(&network, window, cx)
            });
            if let Some(proxy) = &entry.proxy {
                editor.proxy.update(cx, |state, cx| {
                    state.set_selected_value(&SharedString::from(proxy.clone()), window, cx)
                });
            }
        }
        editor
    }
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let snapshot = self.session.read(cx).core();
        let request = snapshot.request_id();
        let busy = self.session.read(cx).core_busy();
        let error = snapshot.error().map(str::to_owned);
        let mut names = vec!["DIRECT".to_owned()];
        names.extend(
            snapshot
                .proxy_names()
                .iter()
                .filter(|n| n.as_str() != "DIRECT")
                .cloned(),
        );
        if let Some(entry) = &self.previous
            && let Some(proxy) = &entry.proxy
            && !names.contains(proxy)
        {
            names.push(proxy.clone());
        }
        if names != self.proxy_names {
            let selected = self.proxy.read(cx).selected_value().cloned();
            self.proxy_names = names.clone();
            self.proxy.update(cx, |state, cx| {
                state.set_items(
                    names.into_iter().map(SharedString::from).collect(),
                    window,
                    cx,
                );
                state.set_selected_value(&selected.unwrap_or_else(|| "DIRECT".into()), window, cx);
            });
        }
        if let Some(id) = self.pending
            && request == Some(id)
            && !busy
        {
            self.pending = None;
            if error.is_none() {
                window.close_dialog(cx);
            }
        }
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let network = self
            .transport
            .read(cx)
            .selected_value()
            .map(|s| s.split('+').map(str::to_owned).collect())
            .unwrap_or_default();
        let proxy = self.proxy.read(cx).selected_value().map(|s| s.to_string());
        if self.pending.is_some() || self.session.read(cx).core_busy() {
            return;
        }
        fn endpoint(host: &str, port: &str) -> Result<String, String> {
            let host = host.trim().trim_start_matches('[').trim_end_matches(']');
            let port = port
                .trim()
                .parse::<u16>()
                .ok()
                .filter(|p| *p != 0)
                .ok_or("端口需为 1–65535 的整数")?;
            if host.is_empty() {
                return Err("请填写监听地址和目标主机".into());
            }
            Ok(if let Ok(ip) = host.parse::<std::net::IpAddr>() {
                std::net::SocketAddr::new(ip, port).to_string()
            } else {
                format!("{host}:{port}")
            })
        }
        let entry = (|| {
            let entry = Tunnel {
                network,
                address: endpoint(
                    &self.local_host.read(cx).value(),
                    &self.local_port.read(cx).value(),
                )?,
                target: endpoint(
                    &self.target_host.read(cx).value(),
                    &self.target_port.read(cx).value(),
                )?,
                proxy,
            };
            entry.validate()?;
            Ok::<_, String>(entry)
        })();
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                ui::show_error(error, window, cx);
                return;
            }
        };
        let change = match &self.previous {
            Some(previous) => tunnels::Change::Replace {
                previous: previous.clone(),
                entry,
            },
            None => tunnels::Change::Add(entry),
        };
        match core::submit(core::Command::Tunnel(change), window, cx) {
            Ok(id) => self.pending = Some(id),
            Err(error) => ui::show_error(error, window, cx),
        }
        cx.notify();
    }
}
impl Render for TunnelEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = self.session.read(cx).core_busy() || self.pending.is_some();
        let address_row = |host: &Entity<InputState>,
                           port: &Entity<InputState>,
                           host_label: &str,
                           port_label: &str| {
            ui::form_row()
                .child(
                    div().flex_1().min_w(space::NONE).child(ui::field_label(
                        host_label,
                        Input::new(host)
                            .with_size(control::STANDARD)
                            .w_full()
                            .disabled(busy)
                            .aria_label(host_label.to_owned()),
                    )),
                )
                .child(
                    div()
                        .w(layout::FILTER_WIDTH)
                        .flex_shrink_0()
                        .child(ui::field_label(
                            port_label,
                            Input::new(port)
                                .with_size(control::STANDARD)
                                .w_full()
                                .disabled(busy)
                                .aria_label(port_label.to_owned()),
                        )),
                )
        };
        ui::form()
            .child(ui::field_label(
                "协议",
                Select::new(&self.transport)
                    .with_size(control::STANDARD)
                    .w_full()
                    .disabled(busy)
                    .accessibility_label("隧道协议"),
            ))
            .child(address_row(
                &self.local_host,
                &self.local_port,
                "监听地址",
                "监听端口",
            ))
            .child(address_row(
                &self.target_host,
                &self.target_port,
                "目标主机",
                "目标端口",
            ))
            .child(ui::field_label(
                "代理组 / 节点",
                Select::new(&self.proxy)
                    .with_size(control::STANDARD)
                    .w_full()
                    .disabled(busy)
                    .accessibility_label("隧道代理组或节点"),
            ))
            .child(
                ui::muted("监听地址填写本机 IP；目标支持域名、IPv4 和 IPv6。", cx)
                    .text_size(text::CAPTION),
            )
    }
}
struct TunnelFooter {
    editor: Entity<TunnelEditor>,
    _subscriptions: Vec<Subscription>,
}
impl TunnelFooter {
    fn new(editor: Entity<TunnelEditor>, cx: &mut Context<Self>) -> Self {
        let session = editor.read(cx).session.clone();
        let subscriptions = vec![
            cx.observe(&editor, |_, _, cx| cx.notify()),
            cx.observe(&session, |_, _, cx| cx.notify()),
        ];
        Self {
            editor,
            _subscriptions: subscriptions,
        }
    }
}
impl Render for TunnelFooter {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editor = self.editor.read(cx);
        let busy = editor.pending.is_some() || editor.session.read(cx).core_busy();
        let label = if editor.pending.is_some() {
            "应用中"
        } else if editor.previous.is_some() {
            "保存"
        } else {
            "添加"
        };
        let owner = self.editor.clone();
        h_flex()
            .w_full()
            .justify_end()
            .gap(space::SM)
            .child(
                Button::new("cancel-tunnel")
                    .icon(gpui_kit::assets::IconName::X)
                    .with_size(control::STANDARD)
                    .label("取消")
                    .disabled(busy)
                    .on_click(|_, w, cx| w.close_dialog(cx)),
            )
            .child(
                Button::new("save-tunnel")
                    .icon(gpui_kit::assets::IconName::Save)
                    .with_size(control::STANDARD)
                    .primary()
                    .label(label)
                    .disabled(busy)
                    .on_click(move |_, w, cx| owner.update(cx, |this, cx| this.save(w, cx))),
            )
    }
}
