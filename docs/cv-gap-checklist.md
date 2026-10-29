# Clash Verge Rev 功能对照与差距清单

更新日期：2026-10-29。参考本地 `clash-verge-rev-dev/`（tauri.conf.json 标记 2.5.9），不是线上最新版声明。对照本轮 Suki 实现（含系统代理、重置、代理来源／搜索／测速、代理链、诊断与界面修正），不能据此推断已发布版本。参考目录没有独立 Git 身份，不使用父仓库 SHA 标记 CV 版本。

## 范围与状态

本清单覆盖 CV 8 个导航目的地、92 个注册命令、75 个 IVerge 字段、前端直接调用的 Mihomo API、增强管线、平台生命周期与交付。命令/字段附录逐名核对，避免仅按页面漏项。代码中有字段、按钮、菜单或文案不表示执行已接入。

- **未接入**：缺真实执行或用户功能；有设置入口也按未接入记录。
- **部分**：主路径或部分子能力已有，剩余项逐项列出。
- **已有主体**：已接入当前主体；不能替代相应平台/边界验收。
- **按要求不做 / 设计差异**：用户确认的产品边界，不计作待补功能。
- **待验收**：证据不足；不等于代码完全缺失，也不标记完成。
- **暂缓**：用户明确暂不接入，保留对照差距，不纳入当前实施。

## 当前重点缺项

| 类别 | 尚缺能力 |
| --- | --- |
| 系统网络 | TUN 实际执行、提权服务、UWP 回环／防火墙、系统代理崩溃恢复与真实平台验证；误删的两个绕过开关待恢复 |
| 桌面生命周期 | 托盘、开机自启、静默启动、关闭到托盘、单实例、外部 URI 导入、应用重启 |
| 备份与更新 | 完整备份／恢复、WebDAV、自动备份、应用升级及签名交付 |
| 代理与订阅 | Provider 链来源实例化、既有 dialer 链反解、测速/连接清理专用集成回归、下一次更新时间核对 |
| 观测与工具 | 完整合成报告、IP 元数据专用回归、网站测试、内存采集及完整诊断；解锁检测暂缓 |
| 验收与其他差异 | Windows/Linux 原生运行与真实凭据读写、服务日志及故障恢复；多语言暂缓，不增加双列表 |

下表 C01–C40 共 40 个能力项，不是 40 个独立缺失功能，也不能换算成完成百分比。已有主体、部分覆盖、未接入和待验收分别保留。

## 功能差距总表

编号用于关联后面的命令/设置附录。CV 路径均相对参考目录，Suki 路径相对本仓库。

| 编号 | 状态 | 能力 | 具体差距及现状 | CV 证据 | Suki 证据 |
| --- | --- | --- | --- | --- | --- |
| C01 | 部分 | 系统信息与诊断 | 缺少完整 OS/权限/服务状态、应用运行时长和主机信息展示。已有核心版本、部分运行状态及维护页脱敏诊断 ZIP 导出。 | [system-info-card.tsx](../clash-verge-rev-dev/src/components/home/system-info-card.tsx) | [view.rs](../src/overview/view.rs) |
| C02 | 部分 | 系统代理 | 已接入三平台手动/PAC 适配器、真实状态回读、bypass、守卫、失败恢复和停止/退出清理；缺 CV 的“绕过校验／使用默认绕过”开关（此前未经用户确认误删，不能标为按要求不做）；还缺三平台实机验收、崩溃恢复及提权服务。具体平台范围见 [系统代理](system-proxy.md)。 | [sysopt.rs](../clash-verge-rev-dev/src-tauri/src/core/sysopt.rs) | [service.rs](../src/core/service.rs) |
| C03 | 未接入 | TUN 与系统网络 | 参数可保存，但合成/启动明确拒绝启用；缺少提权、网卡/路由/DNS 劫持、平台栈、排除项的实际系统集成和恢复。 | [tun.rs](../clash-verge-rev-dev/src-tauri/src/enhance/tun.rs) | [flow.rs](../src/configuration/flow.rs) |
| C04 | 未接入 | 平台服务 | 安装、卸载、重装、修复、所有权交接、服务与子进程切换、失败通知和用户选择恢复路径。已有普通子进程管理，不能替代服务模式。 | [service.rs](../clash-verge-rev-dev/src-tauri/src/core/service.rs) | [service.rs](../src/core/service.rs) |
| C05 | 未接入 | Windows 集成 | UWP 回环工具执行、防火墙及权限集成；当前仅有未接入动作或命令入口，不构成实际支持。 | [win_uwp.rs](../clash-verge-rev-dev/src-tauri/src/core/win_uwp.rs) | [mod.rs](../src/settings/mod.rs) |
| C06 | 部分 | 启动与窗口生命周期 | 缺系统自启、静默启动、关闭到托盘、启动脚本及应用重启。已实现开始页面读取、启动检查内核及缺失状态、维护页重置配置并退出（执行前提供撤销窗口）。 | [autostart.rs](../clash-verge-rev-dev/src-tauri/src/core/autostart.rs) | [main.rs](../src/main.rs) |
| C07 | 部分 | 单实例与外部导入 | 缺第二实例唤起首实例、URI scheme 注册/订阅导入及认证本地控制服务。PAC 本地回环服务已在 C02 接入；内核目录锁只是内核所有权保护。 | [server.rs](../clash-verge-rev-dev/src-tauri/src/utils/server.rs) | [main.rs](../src/main.rs) |
| C08 | 未接入 | 托盘 | 托盘实体、点击行为、系统代理/TUN/出站状态图标、速度、定制图标、菜单图标与顺序、订阅/模式/组/节点选择、关闭连接、复制环境变量、目录、日志、重启与退出菜单。设置字段尚无托盘消费者。 | [tray](../clash-verge-rev-dev/src-tauri/src/core/tray) | [schema.rs](../src/preferences/schema.rs) |
| C09 | 未接入 | 应用升级 | 应用自动/手动检查、发行说明、下载/取消、安装和重启。已实现的是 Mihomo 内核升级，二者独立。 | [updater.rs](../clash-verge-rev-dev/src-tauri/src/core/updater.rs) | [integration.rs](../src/integration.rs) |
| C10 | 部分 | 打包交付 | 缺完整多平台安装包、签名/公证、更新验签与发布通道、安装卸载生命周期和自动发布验证。运行资源引导代码已有；当前工作区未见原清单提及的 ci/ 和 .gitlab-ci.yml，不能继续把多平台打包脚本记为现存能力。 | [tauri.conf.json](../clash-verge-rev-dev/src-tauri/tauri.conf.json) | [bootstrap](../src/bootstrap/mod.rs) |
| C11 | 未接入 | 完整本地备份 | 完整配置+订阅+覆写+DNS 的归档备份、历史列表/删除、导入/导出、全量恢复和运行状态重建。现有单页 JSON 快照不能作为完整备份。 | [backup.rs](../clash-verge-rev-dev/src-tauri/src/core/backup.rs) | [storage.rs](../src/storage.rs) |
| C12 | 暂缓 | WebDAV | 凭据安全保存、连接配置、远程备份创建/列表/删除/下载/恢复及失败恢复。现有凭据字段明确拒绝保存。 | [backup.rs](../clash-verge-rev-dev/src-tauri/src/core/backup.rs) | [core_apply.rs](../src/settings/core_apply.rs) |
| C13 | 暂缓 | 自动备份 | 定时备份、配置变化触发、保留数量及本地/远程历史策略执行；字段仅持久化。 | [auto_backup.rs](../clash-verge-rev-dev/src-tauri/src/core/auto_backup.rs) | [schema.rs](../src/preferences/schema.rs) |
| C14 | 已有主体 | JavaScript 增强 | 已接入 QuickJS、同步 main(config,name)、后台合成、真实引擎中断与取消、内存/栈/输入输出/日志预算、执行日志及语法检查；执行失败保留原配置。用户自定义源码不提供恢复默认。Windows/Linux 原生验收仍属 C40。 | [script.rs](../clash-verge-rev-dev/src-tauri/src/enhance/script.rs) | [mod.rs](../src/configuration/mod.rs) |
| C15 | 已有主体 | 远程覆写 | 已接入 YAML/JavaScript 直接导入、自动名称、UA/鉴权/Age/超时、手动/定时更新和批次失败保留；编辑、开关、排序、删除/撤销均自动热应用，无应用按钮。 | [prfitem.rs](../clash-verge-rev-dev/src-tauri/src/config/prfitem.rs) | [mod.rs](../src/profiles/mod.rs) |
| C16 | 已有主体 | 订阅下载选项 | 已接逐订阅 with_proxy/self_proxy/忽略证书错误，self_proxy 优先于系统代理、默认直连；TLS 客户端按策略隔离。订阅信息表单在文本字段之后排列开关，不自动降级为另一条传输路径。跨平台真实网络与证书环境仍需验收。 | [prfitem.rs](../clash-verge-rev-dev/src-tauri/src/config/prfitem.rs) | [remote.rs](../src/profiles/remote.rs) |
| C17 | 已有主体 | 订阅元数据与调度展示 | 按用户确认，本轮不再列为待补；名称、URL、主页、流量、配额、到期、最后更新时间及真实自动更新已实现。历史清单中的备注／下一次计划时间不作为本轮新增要求。 | [prfitem.rs](../clash-verge-rev-dev/src-tauri/src/config/prfitem.rs) | [model.rs](../src/profiles/model.rs) |
| C18 | 设计差异 | 全局 DNS | 按用户决定使用全局 YAML 与全局开关；单订阅差异通过覆写作用范围实现。无按订阅开关或来源确认，关闭后继承来源；保存、切换与重启统一合成。 | [dns.rs](../clash-verge-rev-dev/src-tauri/src/config/dns.rs) | [flow.rs](../src/configuration/flow.rs) |
| C19 | 部分 | 运行配置查看 | 设置工具可查看及复制真实合成 YAML，运行中取已应用内容，停止时按当前来源合成且不要求已安装内核；脚本执行日志沿用原入口。尚缺与 CV 完整运行合成报告对应的专用诊断展示。 | [runtime.rs](../clash-verge-rev-dev/src-tauri/src/cmd/runtime.rs) | [actions.rs](../src/settings/actions.rs) |
| C20 | 部分 | 代理来源身份 | 已接来源 record ID、成员 occurrence、use/include-all/exclude-type 绑定、核心同名优先、Provider 独立 healthcheck 和未解析重绑定；来源与 occurrence 回归通过。链式 Provider 完整实例化仍未补齐，不将名称解析等同于完整链来源支持。 | [proxy_view.rs](../clash-verge-rev-dev/src-tauri/src/core/proxy_view.rs) | [runtime.rs](../src/proxies/runtime.rs) |
| C21 | 已有主体 | 代理高级搜索 | 已接大小写、整词、正则、delay 比较及 timeout/error、type=、独立分组过滤；无效正则不匹配并报错，数值相等与 timeout 区间分开。搜索选项位于搜索框内。 | [proxy](../clash-verge-rev-dev/src/components/proxy) | [model.rs](../src/proxies/model.rs) |
| C22 | 部分 | 分组工具与导航 | 已接逐组测速地址、工具左右位置、组头单行、拖动排序；单条结果及颜色立即显示，批次结束更新排序。按最新要求不提供侧边快速跳转及悬停相关设置；不再计为待补。原生虚拟列表吸顶、返回顶部仍未接。 | [proxy](../clash-verge-rev-dev/src/components/proxy) | [view.rs](../src/proxies/view.rs) |
| C23 | 部分 | 测速自动化 | 已接自动当前节点检测开关/间隔执行与首页快捷测速，复用同一并发、去重和代次隔离通道；设置位于通用设置，快捷测试位于首页策略节点旁。专门的自动调度/取消回归及完整实机链路仍待补。 | [current-proxy-card.tsx](../clash-verge-rev-dev/src/components/home/current-proxy-card.tsx) | [monitor.rs](../src/core/monitor.rs) |
| C24 | 部分 | 节点切换关闭连接 | 已接 auto_close_connection：切换成功后仅关闭 chains 含原节点的连接，并回读确认；清理失败保留“节点已切换”事实。针对切换与新建并发连接的专用集成回归仍待补。 | [proxy](../clash-verge-rev-dev/src/components/proxy) | [proxy_control.rs](../src/core/proxy_control.rs) |
| C25 | 部分 | 链式节点编排 | 已接多节点添加、删除/撤销、清空/撤销及拖动排序；可视化沿用覆写单行列表，显示入口／中继／出口文字标识，节点类型在右、删除最右，不显示延迟、箭头或上下移动按钮。YAML 采用完整 proxies 节点序列，底栏切换，与可视模式双向同步节点定义及顺序。Provider 来源完整实例化及未解析候选重绑定仍待补。 | [proxy-chain.tsx](../clash-verge-rev-dev/src/components/proxy/proxy-chain.tsx) | [chain.rs](../src/proxies/chain.rs) |
| C26 | 部分 | 链式状态对齐 | 已接 Selector/URLTest 目标、连接后保留编辑状态、独立清空及断开清空元数据；现有热应用、出口固定/回读与恢复共用工作流。尚缺从来源既有 dialer 链反解，以及 URLTest 断开/恢复的真实内核集成回归。 | [proxy-groups.tsx](../clash-verge-rev-dev/src/components/proxy/proxy-groups.tsx) | [chain.rs](../src/core/chain.rs) |
| C27 | 未接入 | 网站连通性测试 | Apple/GitHub/Google/YouTube 等批量测试、自定义站点增删改图标/拖排及测试全部。现有出口探测不是此能力。 | [test-card.tsx](../clash-verge-rev-dev/src/components/home/test-card.tsx) | [view.rs](../src/overview/view.rs) |
| C28 | 部分 | 出口 IP 信息 | 已接国家/旗帜、ASN、组织、地理位置、经纬度、IP 隐藏/显示、独立刷新及 300 秒自动刷新控制。刷新走独立监测通道，不重启内核；失败保留上次信息。元数据边界及刷新监督专用回归、真实服务验收仍待补。 | [ip-info-card.tsx](../clash-verge-rev-dev/src/components/home/ip-info-card.tsx) | [monitor.rs](../src/core/monitor.rs) |
| C29 | 未接入 | 内存采集 | /memory 流、内核占用/限制信息与展示设置。 | [use-memory-data.ts](../clash-verge-rev-dev/src/hooks/use-memory-data.ts) | [monitor.rs](../src/core/monitor.rs) |
| C30 | 部分 | 首页卡片与采集偏好 | 缺卡片显隐/排序/布局自定义、失焦暂停绘制策略及对应开关；已有实时曲线、持久流量和采样排行。不同视觉风格本身不算缺失。 | [layout-viewer.tsx](../clash-verge-rev-dev/src/components/setting/mods/layout-viewer.tsx) | [view.rs](../src/overview/view.rs) |
| C31 | 已有主体 | 连接界面 | 保留单一表格，不增加双布局；已接 remoteDestination 回退、完整进程路径、格式化本地完整时间。详情按连接/流量/路由/进程与时间两列分组，可复制完整详情，长值换行。实时/历史、关闭、暂停、列设置和搜索沿用现有能力。 | [connections.tsx](../clash-verge-rev-dev/src/pages/connections.tsx) | [traffic](../src/traffic) |
| C32 | 已有主体 | 实时规则页 | 已接入 /rules、代次/修订绑定、真实顺序/内容/类型/策略、刷新、虚拟化、类型筛选、高级搜索、复制和详情；失败保留上次数据，不生成命中统计。 | [rules.tsx](../clash-verge-rev-dev/src/pages/rules.tsx) | [mod.rs](../src/traffic/mod.rs) |
| C33 | 暂缓 | 解锁检测 | 完整解锁页、批量/单项检查、地区和结果、刷新：YouTube、Netflix、Disney+、Prime Video、Spotify、TikTok、Bilibili、Bahamut、ChatGPT、Claude、Gemini。 | [src](../clash-verge-rev-dev/crates/clash-verge-media-unlock/src) | [mod.rs](../src/app/mod.rs) |
| C34 | 暂缓 | 多语言 | 语言资源、切换和动态文案。当前中文界面。 | [i18n.ts](../clash-verge-rev-dev/src/services/i18n.ts) | [main.rs](../src/main.rs) |
| C35 | 部分 | 图标与 WebUI | 已有 HTTP/data/SVG 图标缓存、默认图标、错误处理和 WebUI 集合配置；缺本地图标导入/复制的完整用户入口。WebUI 浏览器打开与外部访问需实机验证。 | [app.rs](../clash-verge-rev-dev/src-tauri/src/cmd/app.rs) | [icons.rs](../src/core/icons.rs) |
| C36 | 部分 | 内核网络/设置完整性 | 已读写监听端口、外部控制、DNS/嗅探及资源等；透明代理 OS 路由/防火墙未集成，服务专属状态/错误通知未接入。不能把原生 YAML 支持未知字段等同于对应平台功能已可用。 | [manager](../clash-verge-rev-dev/src-tauri/src/core/manager) | [launch.rs](../src/core/launch.rs) |
| C37 | 部分 | 日志的服务模式 | 当前子进程/校验 stdout+stderr、真实级别来源、独立程序日志、落盘和搜索齐全；未来服务接管需额外日志流/权限/断线监督。不是现有子进程日志缺失。 | [use-log-data.ts](../clash-verge-rev-dev/src/hooks/use-log-data.ts) | [output.rs](../src/core/output.rs) |
| C38 | 部分 | 故障恢复与通知 | 已接多设置分组校验聚合与设置维护中的诊断 ZIP 导出，诊断包含环境、文件状态及检查结果，不导出订阅/日志正文与凭据；坏文件保留，修正后可重试。服务归属/修复/回退通知仍依赖未接入的服务执行端。 | [notify](../clash-verge-rev-dev/src-tauri/src/core/notify) | [service.rs](../src/integration/service.rs) |
| C39 | 部分 | 安全凭据跨平台 | 订阅/覆写凭据已接 macOS 钥匙串、Windows Credential Manager、Linux oo7 Secret Service；Windows 目标及 Linux 适配器独立编译通过，不写明文设置。未进行 Windows/Linux 真实桌面凭据读写验收；WebDAV 按用户要求暂缓。 | [config](../clash-verge-rev-dev/src-tauri/src/config) | [credentials.rs](../src/profiles/credentials.rs) |
| C40 | 待验收 | 跨平台与性能验收 | Windows/Linux 原生运行、权限/服务/TUN/托盘/安装包；macOS 完整窗口工作流、大规模组/连接性能、真实钥匙串、应用更新与退出网络恢复均需单列验收。构建和虚拟化测试不能替代实机验证。 | [src-tauri](../clash-verge-rev-dev/src-tauri) | [系统代理验证范围](system-proxy.md#验证) |
| X01 | 按要求不做 | 偏好裁剪 | 字体、侧栏偏好/折叠导航定制、全局快捷键、轻量模式、自定义调色板；不列为待开发任务。 | [verge.rs](../clash-verge-rev-dev/src-tauri/src/config/verge.rs) | [AGENTS.md](../AGENTS.md) |
| X02 | 设计差异 | 配置与失败契约 | 内核原生 YAML/程序偏好 JSON/默认值/数组、无 applied.json、无旧格式兼容与迁移；不复制 CV 自动换端口、改设置、静默修复/回退。内置 meta_guard/meta_hy_alpn 脚本涉及旧字段修正，也不作为兼容任务加入。 | [builtin](../clash-verge-rev-dev/src-tauri/src/enhance/builtin) | [AGENTS.md](../AGENTS.md) |
| X03 | 设计差异 | 界面与会话状态 | GPUI 原生组件、布尔 Switch、自动保存、间隔 0 关闭、日志倒序及筛选/排序不持久化、资源独立页面、toast 顶部居中。无需复制 Tauri/MUI/WebView devtools。 | [pages](../clash-verge-rev-dev/src/pages) | [Design.md](../DESIGN.md) |
| B01 | 已有主体 | 内核与配置工作流 | 子进程启停/重启/退出、所有权、就绪确认、私有 HTTP IPC、校验、热加载/受控重启、回滚和当前设置恢复；尚缺 C03/C04/C36。 | [manager](../clash-verge-rev-dev/src-tauri/src/core/manager) | [service.rs](../src/core/service.rs) |
| B02 | 已有主体 | 订阅与增强 | URL/文件/拖放/新建、直接导入、名称元数据、UA/鉴权/超时/Age、定时/批量更新、编辑/排序/删除/撤销/激活、YAML 增强及覆写顺序。缺项见 C14–C18。 | [profiles.tsx](../clash-verge-rev-dev/src/pages/profiles.tsx) | [profiles.rs](../src/core/profiles.rs) |
| B03 | 已有主体 | 代理选择与测速 | 真实分组/节点、模式、固定/取消固定、按订阅记忆选择和恢复、独立并发测速、结果状态和缓存图标；缺项见 C20–C26。 | [proxies.tsx](../clash-verge-rev-dev/src/pages/proxies.tsx) | [proxy_control.rs](../src/core/proxy_control.rs) |
| B04 | 已有主体 | 资源与内核升级 | GeoIP/GeoSite/MMDB/ASN 更新、规则/代理集合读取更新、内核发行频道/版本下载安装和取消清理。不是应用升级。 | [provider-button.tsx](../clash-verge-rev-dev/src/components/rule/provider-button.tsx) | [resources.rs](../src/core/resources.rs) |
| B05 | 已有主体 | 实时采集与列表 | traffic/connections 私有 WS、监督重连/代次隔离、有界批次、流量统计持久化、连接历史、虚拟表格与选中/滚动分离。缺项见 C29/C31/C32。 | [connections.tsx](../clash-verge-rev-dev/src/pages/connections.tsx) | [streams.rs](../src/core/streams.rs) |
| B06 | 已有主体 | 日志与通知 | 程序/内核独立级别和保存、程序模块、内核解析、轮转/清理、脱敏、有界列表、倒序、正则/整词/大小写搜索；持续下载进度、单位自适应、toast 在弹窗上方。 | [logs.tsx](../clash-verge-rev-dev/src/pages/logs.tsx) | [logging.rs](../src/logging.rs) |
| B07 | 已有主体 | 设置和公共操作 | 内核原生 YAML 与程序偏好 JSON、默认值、校验/不覆盖坏文件、自动保存；开始页面、主题/背景、环境变量复制、目录打开、网卡枚举。各字段仍须按实际消费者判断。 | [verge.rs](../clash-verge-rev-dev/src-tauri/src/config/verge.rs) | [mod.rs](../src/preferences/mod.rs) |

## 导航目的地核对

| CV 目的地 | Suki 状态与缺口 |
| --- | --- |
| home | 真实流量、统计、出口探测已有；C01/C23/C27–C30 |
| proxies | 真实节点与测速已有；C20–C26/C35 |
| profiles | 主体 CRUD/更新/增强已有；C14–C18 |
| connections | 实时采集与关闭已有；C31 |
| rules | 规则集合更新已有；实时规则页主体已接入，见 C32 |
| logs | 子进程/程序采集保存已有；服务模式 C37；会话语义遵循 X03 |
| unlock | C33，全页未接入 |
| settings | 逐字段见附录；重点 C02–C13/C18/C34/C36/C39 |

Suki 的内核、外部资源、覆写、DNS、嗅探、系统代理和 TUN 分设页面属于信息组织差异，不要求恢复 CV 的页面层级。

## Mihomo API 与采集核对

| CV 使用的能力 | Suki 对照 |
| --- | --- |
| /configs、版本、模式 | B01/B03；运行配置工具 C19 |
| /proxies、组选择、取消固定 | B03；来源 occurrence C20 |
| 节点 delay、Provider healthcheck | B03；高级过滤/自定义逐组地址/自动化 C21–C23 |
| /providers/proxies、/providers/rules 与更新 | B04，外部资源页 |
| /rules | C32，已接入 |
| /connections、关闭单项/全部 | B05；界面剩余项 C31 |
| WS traffic、connections | B05；有重连/代次监督 |
| WS logs | 自管子进程采用 stdout/stderr；服务日志 C37 |
| WS memory | C29 |
| updateGeo | B04 |

这里按真实能力映射，不要求引入 CV 的 Tauri 命令、状态库或 WebView。

## 设置字段完整核对（75 项）

字段来自 `src-tauri/src/config/verge.rs::IVerge`，按原始顺序列出。状态是该字段对应功能的覆盖程度，不表示要求照搬同名字段。复合字段详情见功能表。

| CV 字段 | 对照状态 | 功能编号 |
| --- | --- | --- |
| `app_log_level` | 已有主体 | B06 |
| `app_log_max_size` | 已有主体 | B06 |
| `app_log_max_count` | 已有主体 | B06 |
| `language` | 未接入 | C34 |
| `theme_mode` | 已有主体 | B07 |
| `tray_event` | 未接入 | C08 |
| `env_type` | 已有主体 | B07 |
| `start_page` | 已有主体 | B07 |
| `startup_script` | 未接入 | C06 |
| `traffic_graph` | 部分 | C30 |
| `enable_memory_usage` | 未接入 | C29 |
| `enable_group_icon` | 已有主体 | B03 |
| `pause_render_traffic_stats_on_blur` | 部分 | C30 |
| `common_tray_icon` | 未接入 | C08 |
| `tray_icon` | 未接入 | C08 |
| `menu_icon` | 未接入 | C08 |
| `menu_order` | 未接入 | C08 |
| `proxy_group_tools_position` | 部分 | C22 |
| `notice_position` | 已有主体 | B06 |
| `collapse_navbar` | 按要求不做 | X01 |
| `sysproxy_tray_icon` | 未接入 | C08 |
| `tun_tray_icon` | 未接入 | C08 |
| `enable_tun_mode` | 未接入 | C03 |
| `enable_auto_launch` | 未接入 | C06 |
| `enable_silent_start` | 未接入 | C06 |
| `enable_system_proxy` | 已有主体 | C02 |
| `enable_proxy_guard` | 已有主体 | C02 |
| `enable_bypass_check` | 未接入（误删，未获用户确认） | C02 |
| `enable_dns_settings` | 已有主体 | C18 |
| `profile_dns_settings` | 设计差异 | C18 |
| `use_default_bypass` | 未接入（误删，未获用户确认） | C02 |
| `system_proxy_bypass` | 已有主体 | C02 |
| `proxy_guard_duration` | 已有主体 | C02 |
| `proxy_auto_config` | 已有主体 | C02 |
| `pac_file_content` | 已有主体 | C02 |
| `proxy_host` | 已有主体 | C02 |
| `theme_setting` | 按要求不做 | X01 |
| `web_ui_list` | 部分 | C35 |
| `clash_core` | 已有主体 | B04 |
| `hotkeys` | 按要求不做 | X01 |
| `enable_global_hotkey` | 按要求不做 | X01 |
| `home_cards` | 部分 | C30 |
| `auto_close_connection` | 已接执行 | C24 专用回归仍待补 |
| `auto_check_update` | 未接入 | C09 |
| `default_latency_test` | 已有主体 | B03 |
| `default_latency_timeout` | 已有主体 | B03 |
| `enable_auto_delay_detection` | 已接执行 | C23 专用回归仍待补 |
| `auto_delay_detection_interval_minutes` | 已接执行 | C23 专用回归仍待补 |
| `enable_builtin_enhanced` | 设计差异 | X02 |
| `proxy_layout_column` | 已有主体 | B03 |
| `test_list` | 未接入 | C27 |
| `auto_log_clean` | 已有主体 | B06 |
| `enable_auto_backup_schedule` | 未接入 | C13 |
| `auto_backup_interval_hours` | 未接入 | C13 |
| `auto_backup_on_change` | 未接入 | C13 |
| `verge_redir_port` | 部分 | C36 |
| `verge_redir_enabled` | 部分 | C36 |
| `verge_tproxy_port` | 部分 | C36 |
| `verge_tproxy_enabled` | 部分 | C36 |
| `verge_mixed_port` | 已有主体 | B01 |
| `verge_socks_port` | 已有主体 | B01 |
| `verge_socks_enabled` | 已有主体 | B01 |
| `verge_port` | 已有主体 | B01 |
| `verge_http_enabled` | 已有主体 | B01 |
| `webdav_url` | 未接入 | C12 |
| `webdav_username` | 未接入 | C12 |
| `webdav_password` | 未接入 | C12 |
| `enable_tray_speed` | 未接入 | C08 |
| `tray_proxy_groups_display_mode` | 未接入 | C08 |
| `tray_inline_outbound_modes` | 未接入 | C08 |
| `enable_auto_light_weight_mode` | 按要求不做 | X01 |
| `auto_light_weight_minutes` | 按要求不做 | X01 |
| `enable_hover_jump_navigator` | 按要求不做 | 最新决定不提供侧边快速跳转 |
| `hover_jump_navigator_delay` | 按要求不做 | 最新决定移除关联设置 |
| `enable_external_controller` | 已有主体 | B01 |

## 注册命令完整核对（92 项）

来源为 `src-tauri/src/ipc.rs::collect_commands!`。同等能力通过 Suki 类型化执行端实现即可；附录不是新增 Tauri 接口计划。JS 与远程覆写已单独接入；订阅/增强命令的覆盖程度仍按 C14/C15 及各自测试判断。

| CV 命令 | 对照状态 | 功能编号 |
| --- | --- | --- |
| `get_system_info` | 部分 | C01 |
| `get_app_uptime` | 部分 | C01 |
| `export_diagnostic_info` | 部分 | C01 |
| `probe_listener` | 已有主体 | B01 |
| `save_proxy_ports` | 已有主体 | B01 |
| `get_sys_proxy` | 已有主体 | C02 |
| `get_auto_proxy` | 已有主体 | C02 |
| `get_embedded_server_port` | 已有主体 | C02 |
| `open_app_dir` | 已有主体 | B07 |
| `open_logs_dir` | 已有主体 | B07 |
| `open_core_dir` | 已有主体 | B07 |
| `get_network_interfaces` | 已有主体 | B07 |
| `get_system_hostname` | 部分 | C01 |
| `restart_app` | 未接入 | C06 |
| `install_update` | 未接入 | C09 |
| `cancel_update_download` | 未接入 | C09 |
| `restart_core` | 已有主体 | B01 |
| `upgrade_clash_core` | 已有主体 | B04 |
| `get_runtime_state` | 已有主体 | B01 |
| `get_pending_failures` | 部分 | C38 |
| `get_sidecar_failure` | 部分 | C38 |
| `entry_lightweight_mode` | 按要求不做 | X01 |
| `install_service` | 未接入 | C04 |
| `uninstall_service` | 未接入 | C04 |
| `reinstall_service` | 未接入 | C04 |
| `repair_service` | 未接入 | C04 |
| `continue_with_sidecar` | 未接入 | C04 |
| `sync_runtime_providers` | 已有主体 | B04 |
| `get_clash_info` | 已有主体 | B01 |
| `patch_clash_config` | 已有主体 | B01 |
| `patch_clash_mode` | 已有主体 | B01 |
| `get_clash_mode` | 已有主体 | B01 |
| `change_clash_core` | 已有主体 | B04 |
| `get_runtime_config` | 部分 | C19 |
| `get_proxy_view` | 部分 | C20 |
| `get_runtime_yaml` | 部分 | C19 |
| `get_runtime_logs` | 部分 | C19 |
| `get_runtime_proxy_chain_config` | 部分 | C26 |
| `update_proxy_chain_config_in_runtime` | 部分 | C26 |
| `invoke_uwp_tool` | 未接入 | C05 |
| `copy_clash_env` | 已有主体 | B07 |
| `sync_tray_proxy_selection` | 未接入 | C08 |
| `record_selected_node` | 已有主体 | B03 |
| `forget_selected_node` | 已有主体 | B03 |
| `save_dns_config` | 已有主体 | C18 |
| `apply_dns_config` | 已有主体 | C18 |
| `set_dns_override` | 设计差异 | C18 |
| `take_dns_override_notice` | 设计差异 | C18 |
| `take_service_fallback_notice` | 部分 | C38 |
| `get_core_startup_error` | 已有主体 | B01 |
| `take_service_repair_notice` | 部分 | C38 |
| `take_service_owner_notice` | 部分 | C38 |
| `take_discarded_keys_notice` | 设计差异 | X02 |
| `get_dns_config_content` | 已有主体 | C18 |
| `validate_dns_config` | 已有主体 | C18 |
| `get_clash_logs` | 已有主体 | B06 |
| `get_verge_config` | 已有主体 | B07 |
| `patch_verge_config` | 已有主体 | B07 |
| `test_delay` | 已有主体 | B03 |
| `get_app_dir` | 已有主体 | B07 |
| `copy_icon_file` | 部分 | C35 |
| `download_icon_cache` | 部分 | C35 |
| `open_devtools` | 设计差异 | X03 |
| `exit_app` | 已有主体 | B07 |
| `get_network_interfaces_info` | 已有主体 | B07 |
| `get_profiles` | 已有主体 | B02 |
| `enhance_profiles` | 已有主体 | B02 |
| `patch_profiles_config` | 已有主体 | B02 |
| `view_profile` | 已有主体 | B02 |
| `patch_profile` | 已有主体 | B02 |
| `create_profile` | 已有主体 | B02 |
| `import_profile` | 已有主体 | B02 |
| `reorder_profile` | 已有主体 | B02 |
| `update_profile` | 已有主体 | B02 |
| `delete_profile` | 已有主体 | B02 |
| `read_profile_file` | 已有主体 | B02 |
| `save_profile_file` | 已有主体 | B02 |
| `get_next_update_time` | 未接入 | C17 |
| `create_local_backup` | 未接入 | C11 |
| `list_local_backup` | 未接入 | C11 |
| `delete_local_backup` | 未接入 | C11 |
| `restore_local_backup` | 未接入 | C11 |
| `import_local_backup` | 未接入 | C11 |
| `export_local_backup` | 未接入 | C11 |
| `create_webdav_backup` | 未接入 | C12 |
| `save_webdav_config` | 未接入 | C12 |
| `list_webdav_backup` | 未接入 | C12 |
| `delete_webdav_backup` | 未接入 | C12 |
| `restore_webdav_backup` | 未接入 | C12 |
| `get_unlock_items` | 未接入 | C33 |
| `check_media_unlock` | 未接入 | C33 |
| `check_media_unlock_item` | 未接入 | C33 |

## 不归入 CV 差距的本项目占位功能

- GitHub Gist 同步、Age 同步密钥与运行配置上传：是 Suki 当前保留的计划入口，CV 本次参考主要是 WebDAV，不能把 Gist 计为 CV 已有能力。
- Wi-Fi/SSID 暂停、进程优先级、差异工作目录、部分热加载/自动断开选项：目前仅 schema 或占位动作；没有执行消费者时不得标为完成。其名称不在上述 IVerge 清单，不作为 CV 差距凭空追加。
- 热加载/校验工作流已执行，不表示同名可选设置已控制该行为；选项执行与固定策略分别验收。

## 实施与验收顺序

1. 完善设置故障隔离与原始诊断，维持界面、日志、存储/内核命令可用；已增加坏 JSON 与修正重试回归。
2. 系统代理与 PAC、TUN、平台服务/权限及退出恢复，随后托盘/自启/单实例。
3. JS/远程覆写、全局 DNS、实时规则主体已接入；继续完整来源身份/测速工具/链式编排。
4. 完整备份/WebDAV/调度、应用升级/签名交付。
5. 网站/解锁/IP/内存/诊断及界面工具补全，分平台完成验收。

## 本轮已补齐及验证范围

- 系统代理：手动／PAC 切换、回环 PAC 服务、应用内原生平台源码、回读／恢复、守卫、退出清理主体；关闭失败后开关跟随实际状态；macOS 回读重新取得服务配置。尚未完成三平台真实网络验收和崩溃恢复。
- 启动与重置：启动检查内核，缺失显示“不存在内核”；重置程序有真实配置删除与退出流程。此前单独新增的“启动时运行内核”开关已按用户明确要求删除。
- 代理测速：逐节点立即更新数值／颜色，批次结束才排序。
- 增强和撤销：添加位于规则／代理／代理组分类行右上角；增强条目、面板、隧道、文本恢复默认及会话清空补充撤销。所有非进度 toast（含撤销按钮）自动关闭。
- 最后一次隔离回归：208 项通过、16 项按真实内核／资源条件跳过；严格 Clippy、相关格式和差异检查通过。本次核查清单未重新运行应用测试。
- macOS 隔离窗口只覆盖本轮相关页面与交互；不等于全部主题／窗口尺寸或真实系统代理启停验收。Windows/Linux 真实桌面、权限、服务、TUN、托盘和安装包仍未验收。

## 需要纠正的历史归因

“绕过校验”和“使用默认绕过”在本地 CV 已存在；用户没有明确要求移除。此前助手简化系统代理界面时自行删除，随后误称“按用户要求移除”，该归因不成立。两项当前缺失，列入 C02 待恢复，不得归入 X01/X03 的已确认产品裁剪。本轮仅核查及纠正清单，尚未恢复这两个开关及其行为。

来源设置与未接入入口的逐项清单见 [设置来源核查](settings-origin-audit.md)。CV 的可选布局、额外偏好、备注、多语言等差异仅作为对照事实；本清单不授权自行新增设置。明确排除的 X01–X03 不计作缺失任务。

## 对照指纹

用于识别本地参考快照，不代表远端最新版本。

- `clash-verge-rev-dev/src-tauri/src/config/verge.rs`：SHA-256 `acd20e809b2ddd46d637f40b42bc8df004a397b050cc0bcdac477b4871142a5a`
- `clash-verge-rev-dev/src-tauri/src/ipc.rs`：SHA-256 `a5b4cb5d36e2cdb13e482caa30bb37dd2a919d5b2bae11a9000eb118c10038e7`

配置工作流变更详见 [flow.md](flow.md)；简洁实现审核见 [code-audit.md](code-audit.md)。历史 Backend.md 已从当前工作区移除，不再作为验收记录入口。

## 当前补齐进度与界面修正

本轮完整范围和仍待实施部分见 [实施记录](cv-implementation-progress.md)，C36/C37 服务与平台网络未完成，不能把当前分批改动当作全部 C 项完成。

- 代理组取消侧边快速跳转和悬停设置；保留组内定位与拖排。
- 用户订阅／覆写源码不显示恢复默认；设置多行默认值与用户数据区分。
- 订阅信息开关移到末尾并左右对齐，长表单保持一个滚动区域。
- 连接详情分组、完整路径、本地完整日期与时区、复制；不增加列表布局。
- 代理链沿用覆写式单行列表，只拖动排序；类型靠右、删除最右，无延迟及箭头；完整链 YAML 与可视模式双向同步。

最新验证：214 项普通测试通过、16 项按真实内核／资源条件忽略；格式化、严格 Clippy 和差异检查通过。新增回归覆盖代理链拖动、YAML 双向切换、错误草稿保留、顺序和删除一致性、末尾开关对齐、连接完整路径/时间/IPv6、TLS 策略隔离、来源 occurrence 及诊断脱敏。macOS 隔离窗口检查了订阅开关末尾布局、无侧边跳转、代理链右侧类型/删除及完整 YAML 视口、连接详情分组；数据来自独立测试目录，未操作用户真实系统代理、路由或凭据。C36/C37 等未接入范围仍保留，不宣称整批任务完成。

代理链界面跟进：节点选择占满工具行剩余宽度；模式切换与订阅增强相同，使用 FileCode outline 按钮“YAML 编辑／表单编辑”；应用原生组件统一采用简体中文资源，覆盖所有下拉框默认占位、搜索与空结果提示，不增加语言设置。
