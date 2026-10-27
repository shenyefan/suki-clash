# Clash Verge Rev 功能对照与差距清单

更新日期：2026-10-28。参考本地 `clash-verge-rev-dev/`（tauri.conf.json 标记 2.5.9），不是线上最新版声明。对照当前 Suki 未提交工作区，不能据此推断已发布版本。参考目录没有独立 Git 身份，不使用父仓库 SHA 标记 CV 版本。

## 范围与状态

本清单覆盖 CV 8 个导航目的地、92 个注册命令、75 个 IVerge 字段、前端直接调用的 Mihomo API、增强管线、平台生命周期与交付。命令/字段附录逐名核对，避免仅按页面漏项。代码中有字段、按钮、菜单或文案不表示执行已接入。

- **未接入**：缺真实执行或用户功能；有设置入口也按未接入记录。
- **部分**：主路径或部分子能力已有，剩余项逐项列出。
- **已有主体**：已接入当前主体；不能替代相应平台/边界验收。
- **按要求不做 / 设计差异**：用户确认的产品边界，不计作待补功能。
- **待验收**：证据不足；不等于代码完全缺失，也不标记完成。

## 功能差距总表

编号用于关联后面的命令/设置附录。CV 路径均相对参考目录，Suki 路径相对本仓库。

| 编号 | 状态 | 能力 | 具体差距及现状 | CV 证据 | Suki 证据 |
| --- | --- | --- | --- | --- | --- |
| C01 | 部分 | 系统信息与诊断 | 缺少完整 OS/权限/服务状态、应用运行时长、诊断包导出、主机信息展示。已有核心版本和部分运行状态。 | [system-info-card.tsx](../clash-verge-rev-dev/src/components/home/system-info-card.tsx) | [view.rs](../src/overview/view.rs) |
| C02 | 未接入 | 系统代理 | 手动代理、PAC、本机系统代理与自动代理回读、bypass 默认/自定义策略及校验、代理守卫和周期、退出恢复、网络服务变化后重应用均缺少执行。保存开关不等于系统生效。 | [sysopt.rs](../clash-verge-rev-dev/src-tauri/src/core/sysopt.rs) | [service.rs](../src/core/service.rs) |
| C03 | 未接入 | TUN 与系统网络 | 参数可保存，但合成/启动明确拒绝启用；缺少提权、网卡/路由/DNS 劫持、平台栈、排除项的实际系统集成和恢复。 | [tun.rs](../clash-verge-rev-dev/src-tauri/src/enhance/tun.rs) | [flow.rs](../src/configuration/flow.rs) |
| C04 | 未接入 | 平台服务 | 安装、卸载、重装、修复、所有权交接、服务与子进程切换、失败通知和用户选择恢复路径。已有普通子进程管理，不能替代服务模式。 | [service.rs](../clash-verge-rev-dev/src-tauri/src/core/service.rs) | [service.rs](../src/core/service.rs) |
| C05 | 未接入 | Windows 集成 | UWP 回环工具执行、防火墙及权限集成；现有选择界面没有真实执行端。 | [win_uwp.rs](../clash-verge-rev-dev/src-tauri/src/core/win_uwp.rs) | [mod.rs](../src/settings/mod.rs) |
| C06 | 未接入 | 启动与窗口生命周期 | 系统自启、静默启动、关闭到托盘、启动脚本、应用重启/重置执行。开始页面已读取，不属于缺失项。 | [autostart.rs](../clash-verge-rev-dev/src-tauri/src/core/autostart.rs) | [main.rs](../src/main.rs) |
| C07 | 未接入 | 单实例与外部导入 | 第二实例唤起首实例、URI scheme 注册/订阅导入、认证本地控制服务和 PAC 服务。内核目录锁只是内核所有权保护。 | [server.rs](../clash-verge-rev-dev/src-tauri/src/utils/server.rs) | [main.rs](../src/main.rs) |
| C08 | 未接入 | 托盘 | 托盘实体、点击行为、系统代理/TUN/出站状态图标、速度、定制图标、菜单图标与顺序、订阅/模式/组/节点选择、关闭连接、复制环境变量、目录、日志、重启与退出菜单。设置字段尚无托盘消费者。 | [tray](../clash-verge-rev-dev/src-tauri/src/core/tray) | [schema.rs](../src/preferences/schema.rs) |
| C09 | 未接入 | 应用升级 | 应用自动/手动检查、发行说明、下载/取消、安装和重启。已实现的是 Mihomo 内核升级，二者独立。 | [updater.rs](../clash-verge-rev-dev/src-tauri/src/core/updater.rs) | [integration.rs](../src/integration.rs) |
| C10 | 部分 | 打包交付 | 缺完整多平台安装包、签名/公证、更新验签与发布通道、安装卸载生命周期和自动发布验证。已有无签名 macOS 打包及 Windows 脚本。 | [tauri.conf.json](../clash-verge-rev-dev/src-tauri/tauri.conf.json) | [ci](../ci) |
| C11 | 未接入 | 完整本地备份 | 完整配置+订阅+覆写+DNS 的归档备份、历史列表/删除、导入/导出、全量恢复和运行状态重建。现有单页 JSON 快照不能作为完整备份。 | [backup.rs](../clash-verge-rev-dev/src-tauri/src/core/backup.rs) | [storage.rs](../src/storage.rs) |
| C12 | 未接入 | WebDAV | 凭据安全保存、连接配置、远程备份创建/列表/删除/下载/恢复及失败恢复。现有凭据字段明确拒绝保存。 | [backup.rs](../clash-verge-rev-dev/src-tauri/src/core/backup.rs) | [core_apply.rs](../src/settings/core_apply.rs) |
| C13 | 未接入 | 自动备份 | 定时备份、配置变化触发、保留数量及本地/远程历史策略执行；字段仅持久化。 | [auto_backup.rs](../clash-verge-rev-dev/src-tauri/src/core/auto_backup.rs) | [schema.rs](../src/preferences/schema.rs) |
| C14 | 已有主体 | JavaScript 增强 | 已接入 QuickJS、同步 main(config,name)、后台合成、真实引擎中断与取消、内存/栈/输入输出/日志预算、执行日志、模板重置与语法检查；执行失败保留原配置。Windows/Linux 原生验收仍属 C40。 | [script.rs](../clash-verge-rev-dev/src-tauri/src/enhance/script.rs) | [mod.rs](../src/configuration/mod.rs) |
| C15 | 已有主体 | 远程覆写 | 已接入 YAML/JavaScript 直接导入、自动名称、UA/鉴权/Age/超时、手动/定时更新和批次失败保留；编辑、开关、排序、删除/撤销均自动热应用，无应用按钮。 | [prfitem.rs](../clash-verge-rev-dev/src-tauri/src/config/prfitem.rs) | [mod.rs](../src/profiles/mod.rs) |
| C16 | 部分 | 订阅下载选项 | 缺逐订阅 with_proxy/self_proxy 策略；已有全局代理策略、UA、鉴权、超时及重试。CV 的忽略无效证书选项未实现，应作为显式产品/安全决策，不默认关闭证书验证。 | [prfitem.rs](../clash-verge-rev-dev/src-tauri/src/config/prfitem.rs) | [remote.rs](../src/profiles/remote.rs) |
| C17 | 部分 | 订阅元数据与调度展示 | 缺独立备注/描述、下一次计划更新时间展示；名称、URL、主页、流量、配额、到期、最后更新时间及真实自动更新已实现。 | [prfitem.rs](../clash-verge-rev-dev/src-tauri/src/config/prfitem.rs) | [model.rs](../src/profiles/model.rs) |
| C18 | 设计差异 | 全局 DNS | 按用户决定使用全局 YAML 与全局开关；单订阅差异通过覆写作用范围实现。无按订阅开关或来源确认，关闭后继承来源；保存、切换与重启统一合成。 | [dns.rs](../clash-verge-rev-dev/src-tauri/src/config/dns.rs) | [flow.rs](../src/configuration/flow.rs) |
| C19 | 部分 | 运行配置查看 | 缺完整合成 YAML 只读查看/复制及完整合成诊断工具；脚本日志已接入。候选配置与校验执行已存在，设置中的查看动作尚为通用占位。 | [runtime.rs](../clash-verge-rev-dev/src-tauri/src/cmd/runtime.rs) | [actions.rs](../src/settings/actions.rs) |
| C20 | 部分 | 代理来源身份 | 缺 CV 的来源记录 ID、成员 occurrence、Provider 与全局同名节点精确绑定、未解析成员重绑定。现有名称映射对歧义报错，不能当成完整支持。 | [proxy_view.rs](../clash-verge-rev-dev/src-tauri/src/core/proxy_view.rs) | [runtime.rs](../src/proxies/runtime.rs) |
| C21 | 部分 | 代理高级搜索 | 缺大小写/整词/正则、延迟比较与 timeout/error 表达式、type= 类型表达式及独立分组过滤。已有普通关键字、可用节点筛选和排序。 | [proxy](../clash-verge-rev-dev/src/components/proxy) | [model.rs](../src/proxies/model.rs) |
| C22 | 部分 | 分组工具与导航 | 缺可编辑的逐组测速地址、工具位置设置、组头布局/拖排、悬停跳转导航及延迟设置、吸顶/返回顶部等完整组合。已有定位、测速、排序、折叠与自适应列。 | [proxy](../clash-verge-rev-dev/src/components/proxy) | [view.rs](../src/proxies/view.rs) |
| C23 | 部分 | 测速自动化 | 缺自动当前节点检测开关/间隔的执行和首页策略组/节点快捷测试；已有手动节点/组/全部测速、并发去重、Provider healthcheck。固定 60 秒出口探测不等同于 CV 当前节点自动测速。 | [current-proxy-card.tsx](../clash-verge-rev-dev/src/components/home/current-proxy-card.tsx) | [monitor.rs](../src/core/monitor.rs) |
| C24 | 未接入 | 节点切换关闭连接 | auto_close_connection 缺执行；链式断开时关闭连接已实现，但不能替代普通节点切换策略。 | [proxy](../clash-verge-rev-dev/src/components/proxy) | [proxy_control.rs](../src/core/proxy_control.rs) |
| C25 | 部分 | 链式节点编排 | 已有真实 dialer-proxy 合成、校验、热应用、出口选择/回读、保存与重启恢复、断开/连接清理。缺可视多节点添加/删除/拖排、节点角色/延迟/协议、完整 Provider 来源链实例化和未解析节点重绑定；当前入口/出口下拉加自定义 YAML。 | [proxy-chain.tsx](../clash-verge-rev-dev/src/components/proxy/proxy-chain.tsx) | [chain.rs](../src/proxies/chain.rs) |
| C26 | 部分 | 链式状态对齐 | 缺从内核已有 dialer 链反解恢复界面、保持连接后画布状态、独立清空操作、断开时清空本地链元数据。CV 规则模式允许 Selector/URLTest，当前目标仅 Selector/GLOBAL；URLTest 的出口固定及恢复待补。 | [proxy-groups.tsx](../clash-verge-rev-dev/src/components/proxy/proxy-groups.tsx) | [chain.rs](../src/core/chain.rs) |
| C27 | 未接入 | 网站连通性测试 | Apple/GitHub/Google/YouTube 等批量测试、自定义站点增删改图标/拖排及测试全部。现有出口探测不是此能力。 | [test-card.tsx](../clash-verge-rev-dev/src/components/home/test-card.tsx) | [view.rs](../src/overview/view.rs) |
| C28 | 部分 | 出口 IP 信息 | 已有出口 IP、延迟和换源；缺国家/旗帜、ASN、ISP/组织、地理位置、经纬度、IP 隐藏/显示与独立刷新操作。 | [ip-info-card.tsx](../clash-verge-rev-dev/src/components/home/ip-info-card.tsx) | [monitor.rs](../src/core/monitor.rs) |
| C29 | 未接入 | 内存采集 | /memory 流、内核占用/限制信息与展示设置。 | [use-memory-data.ts](../clash-verge-rev-dev/src/hooks/use-memory-data.ts) | [monitor.rs](../src/core/monitor.rs) |
| C30 | 部分 | 首页卡片与采集偏好 | 缺卡片显隐/排序/布局自定义、失焦暂停绘制策略及对应开关；已有实时曲线、持久流量和采样排行。不同视觉风格本身不算缺失。 | [layout-viewer.tsx](../clash-verge-rev-dev/src/components/setting/mods/layout-viewer.tsx) | [view.rs](../src/overview/view.rs) |
| C31 | 部分 | 连接界面 | 已有实时/历史、关闭单项/全部、暂停、列调整、详情与大小写/整词/正则搜索；缺列表/表格双布局及 CV 更完整 metadata 展示（如 remoteDestination、完整进程路径）。已解析 processPath 不等于界面展示完整路径。 | [connections.tsx](../clash-verge-rev-dev/src/pages/connections.tsx) | [traffic](../src/traffic) |
| C32 | 已有主体 | 实时规则页 | 已接入 /rules、代次/修订绑定、真实顺序/内容/类型/策略、刷新、虚拟化、类型筛选、高级搜索、复制和详情；失败保留上次数据，不生成命中统计。 | [rules.tsx](../clash-verge-rev-dev/src/pages/rules.tsx) | [mod.rs](../src/traffic/mod.rs) |
| C33 | 未接入 | 解锁检测 | 完整解锁页、批量/单项检查、地区和结果、刷新：YouTube、Netflix、Disney+、Prime Video、Spotify、TikTok、Bilibili、Bahamut、ChatGPT、Claude、Gemini。 | [src](../clash-verge-rev-dev/crates/clash-verge-media-unlock/src) | [mod.rs](../src/app/mod.rs) |
| C34 | 未接入 | 多语言 | 语言资源、切换和动态文案。当前中文界面。 | [i18n.ts](../clash-verge-rev-dev/src/services/i18n.ts) | [main.rs](../src/main.rs) |
| C35 | 部分 | 图标与 WebUI | 已有 HTTP/data/SVG 图标缓存、默认图标、错误处理和 WebUI 集合配置；缺本地图标导入/复制的完整用户入口。WebUI 浏览器打开与外部访问需实机验证。 | [app.rs](../clash-verge-rev-dev/src-tauri/src/cmd/app.rs) | [icons.rs](../src/core/icons.rs) |
| C36 | 部分 | 内核网络/设置完整性 | 已读写监听端口、外部控制、DNS/嗅探及资源等；透明代理 OS 路由/防火墙未集成，服务专属状态/错误通知未接入。不能把原生 YAML 支持未知字段等同于对应平台功能已可用。 | [manager](../clash-verge-rev-dev/src-tauri/src/core/manager) | [launch.rs](../src/core/launch.rs) |
| C37 | 部分 | 日志的服务模式 | 当前子进程/校验 stdout+stderr、真实级别来源、独立程序日志、落盘和搜索齐全；未来服务接管需额外日志流/权限/断线监督。不是现有子进程日志缺失。 | [use-log-data.ts](../clash-verge-rev-dev/src/hooks/use-log-data.ts) | [output.rs](../src/core/output.rs) |
| C38 | 部分 | 故障恢复与通知 | 当前配置/进程校验和回滚已有；服务归属/修复/回退等待服务实现。JSON 损坏不再禁用整个存储/内核执行端，具体文件错误落盘；修正文件后重试。多文件同时损坏的完整聚合诊断和一键诊断包仍待补。 | [notify](../clash-verge-rev-dev/src-tauri/src/core/notify) | [service.rs](../src/integration/service.rs) |
| C39 | 部分 | 安全凭据跨平台 | macOS 订阅钥匙串已接；Windows/Linux 凭据存储未接，WebDAV 凭据尚未接。不得明文保存以伪装完成。 | [config](../clash-verge-rev-dev/src-tauri/src/config) | [credentials.rs](../src/profiles/credentials.rs) |
| C40 | 待验收 | 跨平台与性能验收 | Windows/Linux 原生运行、权限/服务/TUN/托盘/安装包；macOS 完整窗口工作流、大规模组/连接性能、真实钥匙串、应用更新与退出网络恢复均需单列验收。构建和虚拟化测试不能替代实机验证。 | [src-tauri](../clash-verge-rev-dev/src-tauri) | [Backend.md](../BACKEND.md) |
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
| `enable_system_proxy` | 未接入 | C02 |
| `enable_proxy_guard` | 未接入 | C02 |
| `enable_bypass_check` | 未接入 | C02 |
| `enable_dns_settings` | 已有主体 | C18 |
| `profile_dns_settings` | 设计差异 | C18 |
| `use_default_bypass` | 未接入 | C02 |
| `system_proxy_bypass` | 未接入 | C02 |
| `proxy_guard_duration` | 未接入 | C02 |
| `proxy_auto_config` | 未接入 | C02 |
| `pac_file_content` | 未接入 | C02 |
| `proxy_host` | 未接入 | C02 |
| `theme_setting` | 按要求不做 | X01 |
| `web_ui_list` | 部分 | C35 |
| `clash_core` | 已有主体 | B04 |
| `hotkeys` | 按要求不做 | X01 |
| `enable_global_hotkey` | 按要求不做 | X01 |
| `home_cards` | 部分 | C30 |
| `auto_close_connection` | 未接入 | C24 |
| `auto_check_update` | 未接入 | C09 |
| `default_latency_test` | 已有主体 | B03 |
| `default_latency_timeout` | 已有主体 | B03 |
| `enable_auto_delay_detection` | 未接入 | C23 |
| `auto_delay_detection_interval_minutes` | 未接入 | C23 |
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
| `enable_hover_jump_navigator` | 部分 | C22 |
| `hover_jump_navigator_delay` | 部分 | C22 |
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
| `get_sys_proxy` | 未接入 | C02 |
| `get_auto_proxy` | 未接入 | C02 |
| `get_embedded_server_port` | 未接入 | C02 |
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

本轮配置故障回归：165 项常规测试通过、15 项真实内核测试默认忽略。此前真实 Mihomo 14 项验证是上一轮证据，本轮未重新执行；Smart 在线升级仍未验收。未做原生窗口视觉与 Windows/Linux 实机验收。

## 对照指纹

用于识别本地参考快照，不代表远端最新版本。

- `clash-verge-rev-dev/src-tauri/src/config/verge.rs`：SHA-256 `acd20e809b2ddd46d637f40b42bc8df004a397b050cc0bcdac477b4871142a5a`
- `clash-verge-rev-dev/src-tauri/src/ipc.rs`：SHA-256 `a5b4cb5d36e2cdb13e482caa30bb37dd2a919d5b2bae11a9000eb118c10038e7`

本轮规则/覆写/DNS/嗅探变更详见 [flow.md](flow.md)。旧验证数字仅代表前一轮；本轮结果以 Backend.md 末尾记录为准。
