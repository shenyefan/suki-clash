# 开关执行接入复核（2026-11-03）

状态更新（2026-11-09）：用户确认文档此前列出的实机验收部分均已完成，相关待验收项关闭。下文各次检查中的“未验收／待验收”保留为当时记录；本次未重新运行验收。实现范围、暂缓项及能力限制以[当前功能对照](../../reference/cv/cv-gap-checklist.md)为准。

范围：当前 `src/preferences/schema.rs` 的全部 41 个 Toggle，以及仪表盘以外 `Switch::new` 调用点。只读源码复核，未执行真实网络、服务或凭据操作，不能代替平台验收。当前未发现只改界面布尔值却没有保存/应用入口的正式开关。

## 共同提交链

设置页 `src/settings/fields.rs::field_row` / `runtime.rs::runtime_view` / `sysproxy.rs::system_proxy_view` → `SettingsPage::toggle` → `core_apply.rs::commit_core_field/commit_core` → `core::Command::Settings` → `core/service.rs` 生命周期串行所有者。Core/TUN 参数走 Preferences 候选合成、校验、运行应用后提交；DNS/嗅探通过 native YAML 文档候选；General 应用偏好持久化后由对应所有者读取。AutoRoute 关闭同时提交 AutoRedirect=false，非空 UI 假联动。

| key | 实际执行消费者与语义 |
| --- | --- |
| IsSystemProxyEnabled | core/system_proxy.rs::system_proxy → system_proxy/mod.rs 单一 OS 所有者；原生写入后回读，界面显示 observed 状态 |
| EnableProxyGuard | system_proxy/mod.rs::guard；启用时按配置间隔检查/恢复代理，停止时与清理协调 |
| EnableBypassCheck | preferences/schema.rs::validate_preferences；控制手动模式绕过列表输入校验，关闭后不是空实现 |
| UseDefaultBypass | system_proxy/mod.rs::target_from_values；决定自定义绕过是否叠加应用默认列表 |
| IsTunEnabled | configuration/tun.rs::FIELDS/apply → core 配置应用与 privileged/service 运行路径；不只写 JSON |
| AutoRoute | configuration/tun.rs::apply 写 tun.auto-route；settings/core_apply.rs 关闭时连同 auto-redirect=false 提交 |
| AutoRedirect | configuration/tun.rs::apply 写 tun.auto-redirect；非 Linux 开启被明确拒绝 |
| StrictRoute | configuration/tun.rs::apply 写 tun.strict-route |
| AutoDetectInterface | configuration/tun.rs::apply 写 tun.auto-detect-interface |
| IpAutoRefresh | core/service.rs General Settings 成功后更新 probe_controls.auto_refresh；core/monitor.rs 消费。虽属于仪表盘设置，已有真实监测行为 |
| EnableTraySpeed | tray/mod.rs::sync；macOS 托盘标题显示实时速率。schema 平台能力限制，其他平台不伪装支持 |
| TrayInlineModes | tray/menu.rs::build；实际改变 GPUI 托盘浮层出站模式内联／子层排布 |
| EnableAutoDelay | core/delays.rs::run 自动调度分支；启用后按 AutoDelayInterval 请求当前已选择组节点，共用测速 lane |
| SnifferOverrideEnabled | configuration/flow.rs::apply_settings；关闭继承来源，开启插入全局 sniffer 文档 |
| SnifferEnabled | configuration/flow.rs::SNIFFER_FLAGS/sniffer_form → sniffer.enable |
| OverrideDestination | 同上 → sniffer.override-destination |
| ForceDnsMapping | 同上 → sniffer.force-dns-mapping |
| ParsePureIp | 同上 → sniffer.parse-pure-ip |
| DnsOverrideEnabled | configuration/flow.rs::apply_dns；控制全局 dns/hosts 覆写参与合成 |
| DnsEnabled | configuration/flow.rs::KEYS/dns_document → dns.enable |
| UseSystemHosts | 同上 → dns.use-system-hosts |
| UseHosts | 同上 → dns.use-hosts |
| DnsIpv6 | 同上 → dns.ipv6 |
| PreferH3 | 同上 → dns.prefer-h3 |
| DirectNameserverFollowPolicy | 同上 → dns.direct-nameserver-follow-policy |
| RespectRules | 同上 → dns.respect-rules |
| FallbackGeoIp | 同上 → dns.fallback-filter.geoip |
| EnableExternalController | configuration/preferences.rs::from_values；开启写有效 external-controller，关闭写空，但私有控制通道继续保留 |
| CorsPrivateNetwork | 同上 → external-controller-cors.allow-private-network |
| AllowLan | configuration/preferences.rs FLAGS/from_values/overlay → allow-lan 与绑定地址策略 |
| Ipv6 | 同上 → ipv6 |
| UnifiedDelay | 同上 → unified-delay |
| TcpConcurrent | 同上 → tcp-concurrent |
| StoreSelected | configuration/preferences.rs → profile.store-selected；core/proxy_control.rs/core/chain.rs 同时据此持久化与恢复选择 |
| StoreFakeIp | configuration/preferences.rs → profile.store-fake-ip，实际由内核维护映射 |
| GeoAutoUpdate | configuration/resources.rs::apply → geo-auto-update，随运行配置由内核执行更新 |
| AutoRun | core/service.rs Settings 分支 → desktop/autostart::checkpoint/apply；OS 注册失败恢复 checkpoint，保存失败也恢复 OS 注册；实际状态单独查询 |
| SilentStart | main.rs 启动时决定主窗口显示状态，不影响服务是否安装/自启 |
| CloseToTray | main.rs on_window_should_close → tray::close_to_tray；托盘可用且期望开启时隐藏窗口，否则应用退出清理 |
| ProfileUseProxy | profiles/mod.rs::remote_document；只作为新订阅 self_proxy 初始值，已有订阅由各自传输选项决定，说明与真实范围一致 |
| AutoCloseConnection | core/connections.rs::auto_close_connections/cleanup_*；proxy_control.rs 节点/模式切换及 core/system_proxy.rs 关闭系统代理后清理对应连接 |

注：本次静态枚举实际列出 41 条（包含 IP 自动刷新）。原生 YAML 参数的执行效果属于目标 Mihomo 能力，不代表本机未启动内核时开关立刻产生网络效果；停止状态只保存，符合项目契约。

## 其他页面 Switch

- `profiles/editor.rs` editor-enabled：编辑覆写草稿的 Document.enabled，保存后由统一 Overrides Change::Save 生命周期命令处理；`configuration/mod.rs` 遍历有序覆写时跳过 !enabled，真实参与合成。
- `profiles/editor.rs` with-proxy/self-proxy/invalid-certs：保存入 Document；两种传输开关互斥；`core/profiles.rs::fetch_profile` 选择 self_proxy > with_proxy > Direct，`core/download.rs` → `network.rs::client_with_tls` 按 accept_invalid_certs 使用独立客户端。
- `profiles/view.rs` enable-override、`profiles/overrides.rs` override-enabled：persist_documents → core Overrides Save，expected 索引校验和活动候选应用，不是仅卡片状态改变。
- `profiles/enhance.rs` enhance-no-resolve：EntryEditor::value 将 no-resolve 后缀写入规则，增强保存后参与源合成。
- `profiles/enhance.rs` GROUP_SWITCHES：include-all、include-all-proxies、include-all-providers、lazy、disable-udp、hidden 作为布尔原生字段写入代理组 Value，经过增强文件与统一合成进入 Mihomo；运行时 hidden 从内核代理信息读取。表单保存前属于明确草稿。
- `proxies/chain.rs` chain-enabled：草稿 enabled → Config/Change → core Chain 应用，启用物化/选择出口，关闭重建普通来源与清理，不以草稿代替运行事实。
- `settings/gist.rs` gist-encrypt：on_change 调用 commit → Gist Command::Configure 持久化 Settings.encrypt；`core/gist.rs` Create 使用 archive.encode(settings.encrypt.then_some(recipient))，Age 加密为真实备份输出行为。
- `settings/runtime.rs` 只重复呈现全局 DNS/嗅探使能；`settings/sysproxy.rs` 只重复呈现系统代理 observed-state 开关；都走同一设置命令。

## 非开关动作

当前本地备份创建/历史、Gist 配置/创建/历史、服务管理、Windows 防火墙/UWP、诊断导出、运行配置查看、内核资源管理均有具体命令或平台实现。本次检查的上述动作均有具体执行入口，未发现仅返回成功/示例内容的空实现；这部分不是每个动作的独立实机验收。开发者工具、通用设置中的重复后台服务、复制版本及 WebDAV 已删除；TUN 页的真实服务管理保留。

开机自启不提供手动刷新按钮；每次进入通用设置时请求 AutostartStatus，开关提交或失败恢复后也回读 OS 实际注册状态。期望值与实际值仍分别维护，不因回读自动修改用户设置；说明只保留功能描述，不展示系统注册状态文案。

## 本次验证

- 隔离全量回归：347 通过、16 忽略；包含默认 User-Agent 实际请求及自定义值优先回归。
- 搜索能力启用后的来源多选专项回归通过，严格 Clippy、格式和差异检查通过。
- macOS 隔离窗口确认五个设置页签、精简托盘入口、同步与维护合并、备份动作图标；代理组成员连续选择两项后下拉保持打开，关闭后在同一控件内回显，不存在额外来源输入框。取消草稿后退出预览，未改真实配置。
- 最终启用搜索和移除公共取消图标后做了专项回归及编译检查；该最后两处小调整未重开窗口。真实系统代理、服务、TUN、Windows/Linux 与安全凭据效果未在本次实机操作验证。

## 自动同步与开关排布（2026-11-03）

全部设置页进入时同步统一执行端的最新已提交值，保存中不覆盖；外部入口提交同一设置后，无编辑／失败草稿的页面按提交值变化回填。流量刷新不反复重建输入值。当前页面的编辑内容与失败草稿保留；离开后重新进入读取已提交值。应用偏好以已提交设置为准，系统代理、TUN 和开机注册分别使用其实际状态回读，不把所有设置都伪装成操作系统状态。

全程序已检查原生 Switch 调用方：设置、订阅／覆写、增强规则与代理组、代理链、Gist 及标题开关。设置与表单开关左侧标题、右侧 Switch，Gist 加密不再采用内置右侧标签；仪表盘外部 IP 已有开关仅调整标签先后，不更改仪表盘功能。

本次补充验证：348 项通过、16 项忽略；新增设置同步 UI 回归覆盖外部提交、失败草稿保留与重新进入。严格 Clippy、格式及差异检查通过。macOS 隔离窗口确认 Gist 配置／创建备份／备份历史三按钮的最终文案及设置开关水平排布；其他按钮省略号已在应用源码中清理。未操作真实系统自启动、网络、服务或凭据。

凭据辅助动作改为原生输入框右侧图标，覆盖 Gist 清除／生成、订阅与覆写清除／撤销、控制器生成密钥。macOS 隔离窗口已确认 Gist 按钮不再单独占行、输入框显隐与辅助动作共存；QA 使用隔离假凭据状态，不访问真实安全存储。生成密钥与清除撤销回归通过，严格 Clippy／格式／差异检查通过。

虚拟网卡服务入口按 CV 当前行为精简：删除高级页签，后台服务紧跟启用虚拟网卡；右侧直接提供安装，或服务可用时提供修复／卸载，不再使用菜单。移除自创的停止服务内核、转为子进程命令入口及无调用的动作分支；卸载后的安全停止与普通内核恢复仍由生命周期所有者执行。CV 对照：`components/shared/proxy-control-switches.tsx` 的 serviceUsable／卸载，以及 `components/layout/service-migration-dialog.tsx` 安装／修复。

服务入口调整验证：348 项通过、16 忽略，严格 Clippy 通过；随后补充的原生枚举同步回归通过。macOS 隔离窗口确认常规／路由与 DNS 两页签、启用开关下方安装按钮及协议栈 Mips 回显。未在本机安装／修复／卸载服务；可用服务时的修复／卸载分支仅作源码核对。

仪表盘后续调整：IpAutoRefresh 设置与运行时偏好已删除，不再属于正式开关；改为初次内核就绪及每次进入仪表盘刷新一次，不定时刷新。上述41项为删除前记录，当前设置 Toggle 为40项。
