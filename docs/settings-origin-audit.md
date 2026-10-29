# 设置来源核查清单（2026-10-29）

## 结论

删除 2 项：

| 设置 | 问题 | 处理 |
| --- | --- | --- |
| 启动时运行内核 `StartCore` | CV 没有独立开关，也没有用户指定依据 | 删除字段、默认值、持久化类型分支、界面及启动条件；启动直接检查并尝试启动内核，缺失显示“不存在内核” |
| TUN DNS 模式 `TunDnsMode` | CV／ClashSuki 无对应设置，项目只有表单与枚举，没有执行消费者 | 删除表单字段和持久化枚举；保留真实的 DNS 劫持字段 |

没有自动修改、迁移或删除用户现有设置文件。删除的键不再有功能入口或执行含义。

## 核查范围与判定

核查七个设置目的地的全部字段，含只读状态与动作入口；共 155 项（不是 155 个可保存设置）。另检查仪表盘、代理组、订阅、覆写、连接、规则、日志、标题栏与编辑弹窗的开关、枚举及显示选项。仅用于搜索、排序、筛选、选中、展开和切换视图的会话控件不计入设置字段。

来源依据是当前本地 CV、用户指定的 ClashSuki、原生 Mihomo 配置映射，以及当前 AGENTS.md / DESIGN.md 和本轮明确要求。不能从现有资料证明每个历史选项都经过用户逐项批准；“参考项目有”与“用户指定”分别标注，不混为一谈。

- CV：`clash-verge-rev-dev/src-tauri/src/config/verge.rs`、`src/components/setting/`、服务／备份／更新调用链。
- ClashSuki：`/Users/shenyefan/VSProjects/ClashSuki/ClashSuki` 中同名 C# 属性和 XAML 字段。
- 当前原生映射：`src/configuration/preferences.rs`、`src/configuration/flow.rs`、`src/preferences/native.rs`。
- 当前所有字段：`src/preferences/schema.rs`；动态 WebUI / 隧道集合由专用表单维护。

## 用户明确要求保留的产品决定

- 全局 DNS 开关与原生 YAML、嗅探配置开关；不是照搬 CV 的按订阅 DNS。
- 外观仅保留主题和支持的窗口背景；字体、侧栏偏好、全局快捷键、轻量模式、自定义调色板没有设置入口。
- 托盘和同步设置按项目约定先保留；不能因此声称它们已经接入。
- WebUI、隧道内嵌管理，统一失焦自动保存和弹出编辑。
- 系统代理手动／PAC 放右上角；绕过地址和 PAC 脚本均可弹出编辑；不展示多余的实际状态、服务器地址行。
- 代理延迟逐条显示颜色，批次结束才调整排序。

## 有来源但当前不生效或未完整接入的设置

这些不是与 StartCore 同类的无来源新增项，本轮保留并明确列出问题。来源不代表实现完成。

| 设置 | 来源 | 当前问题 |
| --- | --- | --- |
| 开机自启、静默启动、关闭时最小化到托盘 | CV／ClashSuki | 缺平台启动／托盘窗口行为；当前关闭窗口仍退出 |
| 托盘点击行为、图标、网速、代理组布局、内联出站模式、自定义图标 | CV；用户要求先保留托盘 | 缺托盘实体及消费者 |
| GitHub 下载代理 | ClashSuki | 当前没有全局下载代理消费者 |
| 内核进程优先级、独立工作目录 | ClashSuki | 只有设置字段，无进程执行消费者 |
| 热重载订阅、热重载前关闭连接 | ClashSuki | 工作流按契约热应用，未读取这两个开关 |
| 启动前检查配置 | ClashSuki | 工作流固定校验，开关没有控制作用 |
| 指定 Wi-Fi 直连、直连时使用系统 DNS | ClashSuki | 无 SSID 监听和网络切换执行 |
| 切换代理后关闭连接 | CV `auto_close_connection`／ClashSuki | 未接入切换后的关闭连接策略 |
| 自动检测延迟、检测间隔 | CV | 缺周期调度；手动测速已经接入 |
| 自动检查更新／应用更新 | CV | 缺完整应用升级执行链；内核更新属于另一路径 |

| 定时备份、备份间隔、配置变更备份、历史备份与完整恢复 | CV | 缺完整归档／恢复及调度 |
| WebDAV 连接、上传、获取、远程历史 | CV；用户要求先保留同步 | 缺远程执行 |
| Gist 同步、Token、Gist ID、Age 加密与密钥 | ClashSuki；项目约定先保留同步 | 缺 Gist／Age 执行 |
| 后台服务安装／修复／停止、防火墙重建 | CV／ClashSuki | 缺平台执行，不能宣称成功 |
| 打开开发工具、导出诊断信息 | CV 同类入口 | 当前动作未接入 GPUI 工具或诊断导出 |
| TUN 启用及参数 | Mihomo／ClashSuki | 尚未打通 TUN 权限与配置运行链；不能把表单保存当成网卡启用 |

订阅超时、User-Agent、订阅下载代理、测速并发／超时有执行消费者，没有按名称猜测为无效项。DNS、嗅探、内核和 Geo 字段需按配置工作流判断，不能因为不在 CV 的 IVerge 中就认定是自行添加。

## 逐项来源

只读动作也列出，便于核对整个页面。来源为 ClashSuki 的行表示找到同名属性；不表示 CV 也有。原生字段表示配置概念有来源，不表示所有平台运行链已接入。

### 系统代理（7 项）

| 名称 | 标识 | 依据 |
| --- | --- | --- |
| 启用系统代理 | `IsSystemProxyEnabled` | ClashSuki 同名属性 |
| 代理主机 | `SystemProxyHost` | ClashSuki 同名属性 |
| 代理模式 | `SystemProxyModeIndex` | ClashSuki 同名属性 |
| 代理守卫 | `EnableProxyGuard` | CV `enable_proxy_guard` |
| 检查间隔（秒） | `ProxyGuardDuration` | CV `proxy_guard_duration` |
| 绕过地址 | `BypassList` | ClashSuki 同名属性 |
| PAC 脚本 | `PacScript` | ClashSuki 同名属性 |

### TUN（12 项）

| 名称 | 标识 | 依据 |
| --- | --- | --- |
| 启用虚拟网卡 | `IsTunEnabled` | ClashSuki 同名属性 |
| 协议栈 | `StackIndex` | ClashSuki 同名属性 |
| 设备名称 | `DeviceName` | ClashSuki 同名属性 |
| MTU | `Mtu` | ClashSuki 同名属性 |
| 自动路由 | `AutoRoute` | ClashSuki 同名属性 |
| 自动重定向 | `AutoRedirect` | Mihomo `tun.auto-redirect` |
| 严格路由 | `StrictRoute` | ClashSuki 同名属性 |
| 自动选择接口 | `AutoDetectInterface` | ClashSuki 同名属性 |
| 路由排除地址 | `RouteExcludeAddress` | ClashSuki 同名属性 |
| DNS 劫持 | `DnsHijack` | ClashSuki 同名属性 |
| 后台服务 | `TunService` | CV 服务管理 |
| 防火墙规则 | `TunAction1` | ClashSuki 防火墙管理 |

### 通用设置（60 项）

| 名称 | 标识 | 依据 |
| --- | --- | --- |
| 启动页面 | `StartPage` | CV `start_page` |
| 托盘点击行为 | `TrayEvent` | CV `tray_event` |
| 通知位置 | `NoticePosition` | CV `notice_position` |
| 托盘图标 | `TrayIcon` | ClashSuki 同名属性 |
| 显示托盘网速 | `EnableTraySpeed` | CV `enable_tray_speed` |
| 托盘代理组布局 | `TrayProxyGroups` | CV `tray_proxy_groups_display_mode` |
| 托盘内联显示出站模式 | `TrayInlineModes` | CV `tray_inline_outbound_modes` |
| 自定义托盘图标 | `CustomTrayIcons` | CV `common_tray_icon / sysproxy_tray_icon / tun_tray_icon` |
| 自动检查更新 | `AutoCheckUpdate` | CV `auto_check_update` |
| 自动检测延迟 | `EnableAutoDelay` | CV `enable_auto_delay_detection` |
| 检测间隔（分钟） | `AutoDelayInterval` | CV `auto_delay_detection_interval_minutes` |
| 应用日志级别 | `AppLogLevel` | CV `app_log_level` |
| 单个日志大小（KB） | `AppLogMaxSize` | CV `app_log_max_size` |
| 日志文件数量 | `AppLogMaxCount` | CV `app_log_max_count` |
| 日志清理 | `AutoLogClean` | CV `auto_log_clean` |
| 本地备份 | `LocalBackupHistory` | CV 本地备份 |
| 远程备份 | `RemoteBackupHistory` | CV WebDAV 备份 |
| 定时备份 | `AutoBackupSchedule` | CV `enable_auto_backup_schedule` |
| 备份间隔（小时） | `AutoBackupInterval` | CV `auto_backup_interval_hours` |
| 配置变更时备份 | `AutoBackupOnChange` | CV `auto_backup_on_change` |
| 服务器地址 | `WebDavUrl` | CV WebDAV |
| 用户名 | `WebDavUsername` | CV WebDAV |
| 密码 | `WebDavPassword` | CV WebDAV |
| 远程备份 | `WebDavAction` | CV WebDAV |
| 运行配置 | `RuntimeConfig` | CV 运行配置查看 |
| 目录 | `ConfigDirectories` | CV 目录菜单 |
| 应用更新 | `CheckUpdates` | CV 应用更新 |
| 诊断 | `DeveloperTools` | CV 开发工具／诊断入口；当前未接入 |
| 后台服务 | `ServiceMode` | CV 服务管理 |
| 应用版本 | `AppVersion` | 应用版本信息 |
| 开机自启 | `AutoRun` | ClashSuki 同名属性 |
| 静默启动 | `SilentStart` | ClashSuki 同名属性 |
| 关闭时最小化到托盘 | `CloseToTray` | ClashSuki 同名属性 |
| 环境变量类型 | `EnvTypeIndex` | ClashSuki 同名属性 |
| 主题 | `ThemeIndex` | ClashSuki 同名属性 |
| 窗口背景 | `BackdropIndex` | ClashSuki 同名属性 |
| 退出应用 | `SettingsAction6` | 退出应用 |
| GitHub 下载代理 | `GitHubProxy` | ClashSuki 同名属性 |
| 订阅 User-Agent | `UserAgent` | ClashSuki 同名属性 |
| 订阅超时（秒） | `SubscriptionTimeout` | ClashSuki 同名属性 |
| 订阅下载代理 | `ProfileUseProxy` | ClashSuki 同名属性 |
| 测速地址 | `DelayTestUrl` | ClashSuki 同名属性 |
| 测速并发数 | `DelayTestConcurrency` | ClashSuki 同名属性 |
| 测速超时（毫秒） | `DelayTestTimeout` | ClashSuki 同名属性 |
| 同步运行配置 | `SyncRuntimeConfigToGist` | ClashSuki 同名属性 |
| GitHub Token | `GitHubToken` | ClashSuki 同名属性 |
| Gist ID | `GistId` | ClashSuki 同名属性 |
| 加密 Gist 配置 | `GistAgeEncrypt` | ClashSuki 同名属性 |
| Age 接收方公钥 | `GistAgeRecipient` | ClashSuki 同名属性 |
| Age 私钥 | `GistAgeSecretKey` | ClashSuki 同名属性 |
| 内核进程优先级 | `PriorityIndex` | ClashSuki 同名属性 |
| 独立工作目录 | `DiffWorkDir` | ClashSuki 同名属性 |
| 热重载订阅 | `UseHotReloadProfile` | ClashSuki 同名属性 |
| 热重载前关闭连接 | `HotReloadProfileAutoCloseConnection` | ClashSuki 同名属性 |
| 切换代理后关闭连接 | `AutoCloseConnection` | ClashSuki 同名属性 |
| 启动前检查配置 | `TestProfileOnStart` | ClashSuki 同名属性 |
| 指定 Wi-Fi 直连 | `PauseSsidText` | ClashSuki 同名属性 |
| 直连时使用系统 DNS | `DisableDnsOnPauseSsid` | ClashSuki 同名属性 |
| 备份与恢复 | `SettingsAction28` | CV 备份／恢复 |
| 重置程序 | `ResetProgram` | 用户明确要求：设置维护集中删除全部配置 |

### 嗅探（12 项）

| 名称 | 标识 | 依据 |
| --- | --- | --- |
| 覆写嗅探配置 | `SnifferOverrideEnabled` | ClashSuki 同名属性 |
| 启用嗅探 | `SnifferEnabled` | ClashSuki 同名属性 |
| 覆盖目标地址 | `OverrideDestination` | ClashSuki 同名属性 |
| 强制 DNS 映射 | `ForceDnsMapping` | ClashSuki 同名属性 |
| 嗅探纯 IP 连接 | `ParsePureIp` | ClashSuki 同名属性 |
| HTTP 端口 | `HttpPorts` | ClashSuki 同名属性 |
| TLS 端口 | `TlsPorts` | ClashSuki 同名属性 |
| QUIC 端口 | `QuicPorts` | ClashSuki 同名属性 |
| 跳过域名 | `SkipDomain` | ClashSuki 同名属性 |
| 强制嗅探域名 | `ForceDomain` | ClashSuki 同名属性 |
| 跳过目标地址 | `SkipDstAddress` | ClashSuki 同名属性 |
| 跳过来源地址 | `SkipSrcAddress` | ClashSuki 同名属性 |

### DNS（25 项）

| 名称 | 标识 | 依据 |
| --- | --- | --- |
| 使用全局 DNS 配置 | `DnsOverrideEnabled` | ClashSuki 同名属性 |
| 启用 DNS | `DnsEnabled` | ClashSuki 同名属性 |
| 监听地址 | `DnsListen` | Mihomo `dns.listen` |
| 增强模式 | `EnhancedModeIndex` | ClashSuki 同名属性 |
| 系统 Hosts | `UseSystemHosts` | ClashSuki 同名属性 |
| 自定义 Hosts | `UseHosts` | ClashSuki 同名属性 |
| IPv6 | `DnsIpv6` | ClashSuki 同名属性 |
| 优先 HTTP/3 | `PreferH3` | Mihomo `dns.prefer-h3` |
| 直连 DNS 遵循解析策略 | `DirectNameserverFollowPolicy` | Mihomo `dns.direct-nameserver-follow-policy` |
| 尊重规则 | `RespectRules` | ClashSuki 同名属性 |
| Fake-IP 地址池 | `FakeIpRange` | ClashSuki 同名属性 |
| IPv6 Fake-IP 地址池 | `FakeIpRange6` | Mihomo `dns.fake-ip-range6` |
| Fake-IP 过滤 | `FakeIpFilter` | ClashSuki 同名属性 |
| 过滤模式 | `FakeIpFilterModeIndex` | ClashSuki 同名属性 |
| 主 DNS | `Nameserver` | ClashSuki 同名属性 |
| 备用 DNS | `Fallback` | ClashSuki 同名属性 |
| 引导 DNS | `DefaultNameserver` | ClashSuki 同名属性 |
| 直连 DNS | `DirectNameserver` | ClashSuki 同名属性 |
| 代理节点 DNS | `ProxyServerNameserver` | ClashSuki 同名属性 |
| GeoIP | `FallbackGeoIp` | ClashSuki 同名属性 |
| 地区代码 | `FallbackGeoIpCode` | ClashSuki 同名属性 |
| IP 地址段 | `FallbackIpCidr` | ClashSuki 同名属性 |
| 域名规则 | `FallbackDomain` | ClashSuki 同名属性 |
| 域名解析策略 | `NameserverPolicy` | Mihomo `dns.nameserver-policy` |
| 自定义 Hosts | `Hosts` | ClashSuki 同名属性 |

### 内核（32 项）

| 名称 | 标识 | 依据 |
| --- | --- | --- |
| 运行状态 | `CoreAction0` | 内核生命周期状态 |
| 当前版本 | `CoreAction1` | 内核版本查询 |
| 发行通道 | `CoreReleaseIndex` | ClashSuki 同名属性 |
| 指定版本 | `CoreSpecificVersion` | ClashSuki 同名属性 |
| 运行配置 | `CoreValidate` | CV 配置校验 |
| Fake-IP 缓存 | `CoreAction6` | CV Fake-IP 清理 |
| 混合端口 | `MixedPort` | ClashSuki 同名属性 |
| SOCKS 端口 | `SocksPort` | ClashSuki 同名属性 |
| HTTP 端口 | `HttpPort` | ClashSuki 同名属性 |
| 重定向端口 | `RedirPort` | ClashSuki 同名属性 |
| TProxy 端口 | `TproxyPort` | ClashSuki 同名属性 |
| TCP / UDP 隧道 | `TunnelAction` | 用户指定的内核隧道管理 |
| 启用外部控制 | `EnableExternalController` | ClashSuki 同名属性 |
| 控制器地址 | `ExternalController` | ClashSuki 同名属性 |
| 访问密钥 | `Secret` | ClashSuki 同名属性 |
| 允许私有网络访问 | `CorsPrivateNetwork` | Mihomo `external-controller-cors.allow-private-network` |
| CORS 允许来源 | `CorsOrigins` | Mihomo `external-controller-cors.allow-origins` |
| 新增面板 | `NewWebUiName` | ClashSuki 同名属性 |
| 允许局域网 | `AllowLan` | ClashSuki 同名属性 |
| 允许的地址 | `LanAllowedIps` | ClashSuki 同名属性 |
| 禁止的地址 | `LanDisallowedIps` | ClashSuki 同名属性 |
| 用户认证 | `Authentication` | ClashSuki 同名属性 |
| 免认证地址 | `SkipAuthPrefixes` | ClashSuki 同名属性 |
| IPv6 | `Ipv6` | ClashSuki 同名属性 |
| 统一延迟 | `UnifiedDelay` | ClashSuki 同名属性 |
| TCP 并发 | `TcpConcurrent` | ClashSuki 同名属性 |
| 记住节点选择 | `StoreSelected` | ClashSuki 同名属性 |
| 保留 Fake-IP 映射 | `StoreFakeIp` | ClashSuki 同名属性 |
| 日志级别 | `LogLevelIndex` | ClashSuki 同名属性 |
| 进程查找模式 | `FindProcessIndex` | ClashSuki 同名属性 |
| 保留天数 | `MaxLogDays` | ClashSuki 同名属性 |
| 单文件大小（MB） | `MaxLogFileSizeMb` | ClashSuki 同名属性 |

### 外部资源（7 项）

| 名称 | 标识 | 依据 |
| --- | --- | --- |
| GeoIP 数据库 | `GeoIpUrl` | ClashSuki 同名属性 |
| GeoSite 数据库 | `GeoSiteUrl` | ClashSuki 同名属性 |
| MMDB 数据库 | `MmdbUrl` | ClashSuki 同名属性 |
| ASN 数据库 | `AsnUrl` | ClashSuki 同名属性 |
| GeoData 格式 | `GeoMode` | Mihomo `geodata-mode` |
| 自动更新 | `GeoAutoUpdate` | ClashSuki 同名属性 |
| 更新间隔（小时） | `GeoUpdateIntervalValue` | ClashSuki 同名属性 |

## 设置页以外的检查

| 目的地 | 检查结果 |
| --- | --- |
| 标题栏 | 系统代理、TUN 是用户要求的共用操作入口，不新增独立设置 |
| 仪表盘 | 出站模式是内核命令；时间／排行 Tab 属于视图切换 |
| 代理组 | 测速、排序、展开、筛选和显示选项属于会话；链式代理为用户已指定的真实配置流程 |
| 订阅／覆写 | 启用、作用域、更新间隔、User-Agent、增强和规则字段来自来源模型／参考项目 |
| 连接／规则 | 列显示、排序、过滤、活跃／已关闭、详情属会话状态 |
| 日志 | 来源、级别、搜索、暂停、清空属会话状态；不新增持久设置 |
| 弹窗／编辑器 | 启用、作用域、no-resolve、隧道协议与目标、WebUI 名称和 URL 属于所编辑对象；展开编辑不是新增设置 |

## 验收边界

本清单是源码、参考调用链和现有需求核查，不替代全部页面的实机验收；未声称已核实全部历史授权。普通测试隔离真实系统代理、服务与路由。Windows／Linux 真实桌面与权限集成仍需实机验证。

已验证：202 项测试通过、16 项按真实内核／资源条件跳过；cargo check 和严格 Clippy 通过。独立原生适配器检查含 Linux API 编译与解析测试，共 6 项通过。macOS 隔离真实窗口已确认：内核页显示“不存在内核”、无启动内核设置、TUN 路由与 DNS 页仅保留真实劫持字段；系统代理模式位于右上角，两种模式均能打开对应编辑器并用 Escape 关闭。没有在此次窗口检查中启用系统代理或 TUN；深浅主题／最小窗口全矩阵和其他平台实机仍未验收。

2026-10-29：按用户要求移除所有页面／集合的重置入口，只保留具有程序默认值的设置多行文本恢复默认；用户订阅／覆写源码无恢复默认；全局重置现位于设置维护，并实现停止后台写入后删除配置及退出。

2026-10-29 核查更正：应用日志按天清理已有 `integration::service::program_policy` → `logging::configure_program/cleanup` 消费链，不应列为未接入。“绕过校验”和“使用默认绕过”是 CV 已有功能，此前助手未经用户确认移除，不能归因于用户要求；目前待恢复，详见 CV 差距 C02。

2026-10-29 最新界面修正：移除代理组侧边快速跳转及关联悬停设置。订阅信息开关置于末尾；代理链沿用覆写式行，仅拖动排序，类型靠右且删除最右，不显示延迟或上下箭头；完整链 YAML 与可视化双向同步。
