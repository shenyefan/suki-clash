# Clash Verge Rev 功能对照与差距清单

上游版本另见 [版本与同步记录](cv-upstream-sync.md)（2026-11-05）：本地已快进为 dev 2.6.0（b9ab3b20），官方稳定版 v2.5.10。历史 U01 服务 SSH 私钥暂存、U02 服务 Geo 反向同步、U03 全局模式普通组展示已补齐；本次新增 U05 macOS 离线/换网代理恢复已修复，U06 GLOBAL 初次默认出口是尚未采用的策略差异。

本次 27 个提交、128 文件的增量对照见 [11-05 更新差异复核](cv-update-comparison-2026-11-05.md)，前次 39 提交记录保留在 [11-03 复核](cv-update-comparison-2026-11-03.md)。WebDAV 已按用户决定移除；自动备份继续暂缓；Muse AI 列入用户决定的仪表盘能力；交付采用 Velopack。没有把新参考文件自动算作原逐文件审计覆盖。

更新日期：2026-11-05。历史逐文件对照基线为 CV 2.5.9 快照 eefd025；当前参考目录为 dev 2.6.0／b9ab3b20，后续增量按各次复核记录，不自动计入原全文审计覆盖。对照当前 Suki 工作区，不能据此推断已发布版本。参考目录使用自身 Git HEAD，不使用父仓库 SHA。

第一批修复（2026-11-02）：R29/R34/R30/R04/R47/R42/R45及L02/L04已完成代码整改，仍待真实平台验收。下列各行中这些编号的历史问题以[修复清单](cv-repair-checklist.md)状态为准；其余问题未自动关闭。根项目246项通过、16项忽略，check／严格Clippy通过。

第二批修复（2026-11-02）：R02/R03/R39/R44/R38/R40/R41/R33/R01/R09/R55/R21/R46/R43已完成代码整改，具体恢复语义与未验收范围见[修复清单](cv-repair-checklist.md)。下文这些编号的缺口描述保留历史，不覆盖修复清单最新状态；其他问题未自动关闭。

第三批修复（2026-11-02）：R14/R15/R57/R58/R59/R10/R12/R11/R13/R16/R17/R19/R22代码整改完成，最新状态与检查见[修复清单](cv-repair-checklist.md)。相关历史问题描述不覆盖该清单；其余问题未自动关闭。

第四批修复（2026-11-02）：R05/R32/R35/R18/R37/R36/R61/R60代码整改完成，根项目277项通过、16项忽略，严格Clippy通过。下表C20/C24/C25/C26和相关行已按实际修复更新；仍有19项R、3项L及S01，平台验收另列。详见[修复清单](cv-repair-checklist.md)。

第五批修复（2026-11-03）：R06/R07/R08/R52/R20/R56/R26/R27/R28代码整改完成；剩余10项R、3项L及S01，功能缺项和平台验收仍单列。检查和原生隔离窗口范围见[修复清单](cv-repair-checklist.md)。

第六批修复（2026-11-03）：L01/L03/L05及R23/R24/R25/R31/R49/R50/R51/R48/R53/R54完成。61项R和5项L全部完成代码整改；不表示CV功能全部补齐或平台验收完成。该记录为第六批时状态；本批已完成S01和增强／嵌套组补充，诊断等范围按下述新决定。

## 本批范围决定与结果（2026-11-03）

S01的8项无消费者设置入口／默认值／枚举依赖已清理，保留已实现的自动断开连接和用户指定绕过／同步。增强表单、来源候选、批量URI导入和嵌套组展示已补齐。Gist完整备份使用当前GitHub API、安全凭据、可选Age加密和带清单摘要的归档；不上传凭据库或本机同步设置，恢复通过既有生命周期工作流。根项目321项通过／16项条件忽略，严格Clippy通过；真实GitHub／凭据／服务恢复验收另列。

| 分类 | 后续范围 |
| --- | --- |
| 暂缓 | 通知诊断复制、完整配置合成报告、仪表盘定制、本地图标入口；自动备份和多语言继续暂缓；WebDAV明确不支持 |
| 待用户决定的仪表盘能力 | 网络测试、解锁检测；独立提出，不归入暂缓 |
| 后续成熟工具集成 | 应用升级、打包、签名／公证及发布；独立提出 |
| 仍需验收 | 真实Gist／安全凭据、三平台服务／TUN／代理／退出恢复、原生窗口与大列表性能；Provider物化／系统DNS语义已确认并适配；高级Provider表达式物化的明确限制见下述补充记录 |

## 设置精简与接入复核（2026-11-03）

按用户决定，删除 WebDAV、托盘自定义图标及颜色模式、代理组工具位置、通用设置重复服务入口、版本复制及开发者工具。同步与维护合为一个页签；移除未实现的自动备份空入口。来源选择改为官方多选下拉，User-Agent 默认首先标识 Clash Verge，其次标识 Suki Clash。另删除连接详情“复制详情”按钮（右键菜单复制保留）和开机自启“修正注册”按钮；托盘使用彩色应用图标，仪表盘排行顺序为域名、进程、代理。全部设置 Toggle 和仪表盘以外 Switch 的执行链见[开关接入复核](settings-switch-wiring.md)，源码接入不代替真实平台验收。

## 范围与状态

本清单覆盖 CV 8 个导航目的地、92 个注册命令、75 个 IVerge 字段、前端直接调用的 Mihomo API、增强管线、平台生命周期与交付。命令/字段附录逐名核对，避免仅按页面漏项。代码中有字段、按钮、菜单或文案不表示执行已接入。

- **未接入**：缺真实执行或用户功能；有设置入口也按未接入记录。
- **部分**：主路径或部分子能力已有，剩余项逐项列出。
- **已有主体**：已接入当前主体；不能替代相应平台/边界验收。
- **按要求不做 / 设计差异**：用户确认的产品边界，不计作待补功能。
- **待验收**：证据不足；不等于代码完全缺失，也不标记完成。
- **暂缓**：用户明确暂不接入，保留对照差距，不纳入当前实施。
- **待产品决定**：独立提出的未来仪表盘能力，由用户决定具体操作，不等同暂缓。
- **工具集成**：用户决定后续采用成熟工具承接，不属于当前自行实现范围。

## 本次源码复核

已完成范围的逐项复核见 [2026-10-31 复核清单](cv-parity-review-2026-10-31.md)。发现 61 项代码差距、5 项桌面生命周期差距及 1 组入口／范围问题；该段为修复前发现，已确认缺陷的最新关闭状态以修复清单为准；新增能力与明确限制另列。R01–R61、L01–L05 为本轮源码复核结果；包含开机注册、服务独立启动、关闭隐藏、托盘退出、重置、重启和注销／关机，历史验证记录只对应当时快照。

应用登录自启动与 OS 提权服务启动分别管理；服务存在不代表 GUI 或内核已自启动。正常窗口关闭／托盘退出等 19 个场景及实际清理边界见复核报告的生命周期场景表。

已登记936个文本的修复前全文阅读完成，修复后变更／检查另见修复清单；覆盖基线以 [文件台账](cv-file-review.md) 为准；之前的能力覆盖表不代表所有文件已全文审查。

## 本地备份与待确认项补全（2026-11-03）

本地完整历史／导入导出已接，三类待确认项均已闭合证据并处理，见[补全记录](local-backup-and-parity.md)。Provider常规覆盖和重命名已实现；高级regexp2／override-expr／部分特殊替换仍明确拒绝链物化，属于已确认的能力限制，不能归为全部完成。macOS采用Mihomo原生临时DNS，不改用户服务DNS。下载／TLS按既定严格策略保留设计差异；自动测速等专项回归已补。

## Velopack首次接入（2026-11-03）

应用更新与交付工具已选用Velopack并接通首版执行和GitHub配置，见[交付说明](velopack.md)。这是代码／配置接入，不是已发布或已完成跨平台安装验证；不自动加入Linux deb/rpm、更新渠道切换等额外设计。

## 仪表盘数据对照（2026-11-03）

顶部仅外部 IP，详细信息移至下方；加入真实内核内存。移除日／月、小时分布及代理／直连自定义统计；按用户后续决定恢复仅当前生命周期的代理／域名／进程排行，不持久化，累计量显示内核计数而非本应用采样账本。数据来源、保留策略见[仪表盘数据说明](dashboard-data.md)。DNS 仅当前模式，不编造请求计数。

## 当前重点缺项

| 类别 | 尚缺能力 |
| --- | --- |
| 系统网络 | 第一、二批已修复TUN标识、服务归属与补偿、代理分路等代码缺口；剩余真实服务／TUN／UWP／防火墙及系统网络平台验收 |
| 桌面生命周期 | 第六批已修登录注册协调、初始静默和原生重开；剩余托盘视觉、跨平台登录任务及正式包URI验收 |
| 备份与更新 | Gist及独立本地备份管理已有，WebDAV明确不支持；自动备份暂缓，应用升级与交付改列工具集成 |
| 代理与订阅 | Provider同步及恢复第四批已修；第五批修复测速参数／原值／首页组选取与URL共享、搜索筛选；增强表单与嵌套组展示本批已补；剩余真实来源重绑验收 |
| 观测与工具 | 完整合成报告／诊断复制暂缓；网络测试、解锁检测独立待用户决定；仪表盘定制暂缓，真实查询服务验收另列 |
| 验收与其他差异 | Windows/Linux 原生运行与真实凭据读写、服务日志及故障恢复；多语言暂缓，不增加双列表 |

下表 C01–C40 共 40 个能力项，不是 40 个独立缺失功能，也不能换算成完成百分比。已有主体、部分覆盖、未接入和待验收分别保留。

## 功能差距总表

编号用于关联后面的命令/设置附录。CV 路径均相对参考目录，Suki 路径相对本仓库。

| 编号 | 状态 | 能力 | 具体差距及现状 | CV 证据 | Suki 证据 |
| --- | --- | --- | --- | --- | --- |
| C01 | 部分 | 系统信息与诊断 | 已接 OS 版本、架构、主机、实际权限、自启动回读和应用运行时长；仪表盘及详情已接。服务模式与后台服务状态已接；真实平台服务诊断及交接验收仍待完成。 | [system-info-card.tsx](../clash-verge-rev-dev/src/components/home/system-info-card.tsx) | [view.rs](../src/overview/view.rs) |
| C02 | 部分 | 系统代理 | 三平台手动/PAC、回读、守卫、失败恢复与退出清理已接。macOS已接鉴权helper分路及离线清理；Windows默认及RAS逐连接检查点，KDE原值回退已修。权限拒绝停止自动处理，显式操作可重试。退出可取消与系统结束有界清理已接。剩余真实三平台网络／权限／退出恢复验收。 | [sysopt.rs](../clash-verge-rev-dev/src-tauri/src/core/sysopt.rs) | [macos.rs](../src/system_proxy/macos.rs) |
| C03 | 部分 | TUN 与系统网络 | 常驻服务执行、TUN原生标识、DNS补齐、校验／热更新／回读及失败恢复已接；自动路由／重定向联动提交已修，日常开关不提权。真实网卡、路由、DNS与平台验收待完成。 | [tun.rs](../clash-verge-rev-dev/src-tauri/src/enhance/tun.rs) | [TUN 与服务](tun-service.md) |
| C04 | 部分 | 平台服务 | 维护入口已整合到单一服务程序，安装／卸载不再交付独立可执行文件；[实施记录](service-single-executable-2026-11-05.md)。安装、修复、停止服务内核、卸载及显式转子进程已接；转换保留各阶段提交事实与独立补偿，Unix卸载先确认停止／注销再删除。令牌契约、默认服务归属、健康监督、资源恢复及防火墙部分成功已修。协议现为revision 9，应用与服务同步更新，不提供旧协议兼容。剩余真实安装／交接及故障集成验收。 | [service.rs](../clash-verge-rev-dev/src-tauri/src/core/service.rs) | [privileged.rs](../src/core/privileged.rs) |
| C05 | 已有主体 | Windows 集成 | 采用 ClashSuki 的 FirewallAPI 原生枚举／回环方案，应用选择弹窗可搜索并编辑；未知／已卸载应用豁免保留。服务鉴权写入、并发前置检查、回读与失败恢复；防火墙仅重建服务批准内核及已认证用户应用目录内核的固定命名规则，失败恢复原规则，不接收任意程序路径。无需外部 enableLoopback.exe。剩余为 Windows 实机验收。 | [win_uwp.rs](../clash-verge-rev-dev/src-tauri/src/core/win_uwp.rs) | [windows_network.rs](../platform-service/src/windows_network.rs) |
| C06 | 部分 | 启动与窗口生命周期 | 自启动、静默、关闭到托盘、启动脚本及清理后重启已接。启动协调GUI登录注册与期望值；设置显示OS实际状态、刷新和修正漂移，重置清GUI登录注册，不动helper注册。静默建窗隐藏且不聚焦，托盘失败显示主窗。Windows登录任务权限及回退保留原XML。剩余三平台原生生命周期验收。 | [autostart.rs](../clash-verge-rev-dev/src-tauri/src/core/autostart.rs) | [autostart_windows.rs](../src/desktop/autostart_windows.rs) |
| C07 | 部分 | 单实例与外部导入 | 当前用户实例锁、认证回环端点、第二实例唤起／订阅导入、明确URI名称、有界队列及协议注册已接。macOS官方on_reopen、单实例与托盘共用显示主窗口入口。cargo run不代替正式应用包URI注册验收；正式跨平台激活仍待验收。 | [server.rs](../clash-verge-rev-dev/src-tauri/src/utils/server.rs) | [main.rs](../src/main.rs) |
| C08 | 部分 | 托盘 | OS原生附着菜单、点击行为、普通／系统代理／TUN三态和自定义图标、macOS网速、模式／订阅／组／节点及维护动作已接。组图标按组／来源／代次绑定，拒绝迟到；同目标原生写入失败暂停，明确点击可重试。无独立窗口或自绘滚动条，TUN优先级与CV一致。Quit清本应用效果并保留空闲helper。2026-11-05移除托盘鼠标事件中的菜单重建，避免Windows在TrackPopupMenu期间销毁菜单。剩余原生菜单完整视觉、跨平台和故障验收。 | [tray](../clash-verge-rev-dev/src-tauri/src/core/tray) | [tray](../src/tray) |
| C09 | 部分 | 应用升级 | 已接Velopack检查／下载／校验／安装执行链；当前Release仅公开安装器和压缩包，不公开自动更新清单，新版本需手动升级，已安装版检查会报清单获取错误。非安装包打开GitHub发布页。自动更新源与端到端更新仍待处理。现有Mihomo内核升级仍独立。 | [updater.rs](../clash-verge-rev-dev/src-tauri/src/core/updater.rs) | [integration.rs](../src/integration.rs) |
| C10 | 部分 | 打包签名与发布 | GitHub仅构建Windows x64／ARM64、macOS Apple Silicon、Linux x64。Release仅包含7个安装器／压缩包，内部更新清单和构建元数据保留在Actions artifacts。全矩阵成功后单一草稿Release，描述包含下载表、实际签名状态及自动变更记录；支持可选签名／公证，无证书标未签名。真实Actions构建与签名／安装验收仍待完成，不宣称已发布。 | [tauri.conf.json](../clash-verge-rev-dev/src-tauri/tauri.conf.json) | [bootstrap](../src/bootstrap/mod.rs) |
| C11 | 部分 | 完整备份 | 完整当前配置／订阅／覆写／增强归档、校验及恢复已接入Gist备份。Gist可创建独立历史、分页、下载、恢复、删除与可选Age加密；本地完整历史、文件导入／导出、恢复和删除现已接入维护，共用同一归档与恢复工作流。 | [backup.rs](../clash-verge-rev-dev/src-tauri/src/core/backup.rs) | [storage.rs](../src/storage.rs) |
| C12 | 产品取舍 | WebDAV | 用户明确仅使用 Gist，完整删除 WebDAV 设置及操作入口，不纳入待实现范围。 | [backup.rs](../clash-verge-rev-dev/src-tauri/src/core/backup.rs) | [core_apply.rs](../src/settings/core_apply.rs) |
| C13 | 暂缓 | 自动备份 | 定时备份、配置变化触发、保留数量及本地/远程历史策略执行；未实现入口已移除，不以仅持久化的设置宣称支持。 | [auto_backup.rs](../clash-verge-rev-dev/src-tauri/src/core/auto_backup.rs) | [schema.rs](../src/preferences/schema.rs) |
| C14 | 部分 | JavaScript 增强 | QuickJS同步main、后台合成、中断／取消、内存／栈／输入输出／日志预算及语法检查已接；执行失败保留有效配置，外部源码文件为唯一正文来源。用户内容无恢复默认。真实跨平台引擎验收待补。 | [script.rs](../clash-verge-rev-dev/src-tauri/src/enhance/script.rs) | [mod.rs](../src/configuration/mod.rs) |
| C15 | 已有主体 | 增强与远程覆写 | 已接远程YAML／JS及增强编辑；本批补组use、健康检查、筛选／include-all／接口／路由／隐藏等字段及真实来源候选。固定类型中文展示、英文保存，未知字段无损。代理URI批量解析支持前置／追加，原子报行号错误；同名不同内容拒绝，不静默丢弃。按当前协议解析，不增加旧客户端兼容变体。 | [prfitem.rs](../clash-verge-rev-dev/src-tauri/src/config/prfitem.rs) | [mod.rs](../src/profiles/mod.rs) |
| C16 | 部分 | 订阅下载选项 | 逐订阅系统／内核代理及忽略证书选项已接，开关互斥且位于表单末尾；按明确策略下载，无自动传输回退，TLS客户端隔离。明确间隔优先和远程结构校验已修。剩余真实跨平台网络／证书验收。 | [prfitem.rs](../clash-verge-rev-dev/src-tauri/src/config/prfitem.rs) | [remote.rs](../src/profiles/remote.rs) |
| C17 | 已有主体 | 订阅元数据与调度展示 | 按用户确认不再列待补：名称、URL、主页、流量、配额、到期、最后更新时间及自动更新已实现，明确1440／0间隔保留。备注／下一次计划时间不追加为本轮要求。 | [prfitem.rs](../clash-verge-rev-dev/src-tauri/src/config/prfitem.rs) | [model.rs](../src/profiles/model.rs) |
| C18 | 设计差异 | 全局 DNS | 按用户决定使用全局YAML与开关，单订阅差异通过覆写范围表达；无按订阅DNS开关或来源确认。关闭继承来源，保存／切换／重启统一合成，listen支持:53。 | [dns.rs](../clash-verge-rev-dev/src-tauri/src/config/dns.rs) | [flow.rs](../src/configuration/flow.rs) |
| C19 | 暂缓 | 运行配置与合成报告 | 运行配置查看／复制已有；完整配置合成报告按用户决定暂缓。 | [runtime.rs](../clash-verge-rev-dev/src-tauri/src/cmd/runtime.rs) | [actions.rs](../src/settings/actions.rs) |
| C20 | 部分 | 代理来源身份 | 来源ID／occurrence、Provider作用范围、稳定缓存同步、来源证明和冷启动选择恢复已有。本批补嵌套组当前路径、实际叶协议与已回读能力；循环／缺失／歧义保持未知，保留选择身份。剩余真实服务缓存／慢来源／重绑验收；冷启动链缺确认缓存时明确拒绝。 | [proxy_view.rs](../clash-verge-rev-dev/src-tauri/src/core/proxy_view.rs) | [runtime.rs](../src/proxies/runtime.rs) |
| C21 | 部分 | 代理高级搜索 | 已接大小写、整词、正则、delay 比较及 timeout/error、type=、独立分组过滤；无效正则不匹配并报错，数值相等与 timeout 区间分开。搜索选项位于搜索框内。第五批已清理失效组筛选，清除动作覆盖全部筛选；数值搜索保留原始延迟，超时／失败按明确状态筛选。 | [proxy](../clash-verge-rev-dev/src/components/proxy) | [model.rs](../src/proxies/model.rs) |
| C22 | 部分 | 分组工具与导航 | 逐组测速地址、工具左右位置、组头单行、拖动排序、虚拟列表吸顶和返回顶部已接。组头与节点统一不透明主题底色，吸顶完整遮住下方文字；单条延迟立即显示，批次结束排序。按要求无侧边快速跳转。吸顶交互仍需完整窗口验收。 第五批：逐组URL按订阅保存在统一程序设置，页面仅保留输入草稿，首页与手动／自动测速共用。 | [proxy](../clash-verge-rev-dev/src/components/proxy) | [view.rs](../src/proxies/view.rs) |
| C23 | 部分 | 测速自动化 | 已接自动当前节点检测开关/间隔执行与首页快捷测速，复用同一并发、去重和代次隔离通道；设置位于通用设置，快捷测试位于首页策略节点旁。专门的自动调度／间隔／取消／迟到结果回归已补，完整实机链路待验收。 第五批：首页组选择按订阅记忆，自动测速读取同一组选择与测速URL；输入范围与执行一致，批次按来源身份去重，保留原始延迟并明确区分超时／失败。 | [current-proxy-card.tsx](../clash-verge-rev-dev/src/components/home/current-proxy-card.tsx) | [monitor.rs](../src/core/monitor.rs) |
| C24 | 部分 | 节点切换关闭连接 | AutoCloseConnection已接节点／取消固定、模式切换与关闭系统代理。旧节点只清chains含原节点的快照ID，全清同样按快照ID确认；并发新连接不误关／误报。初查、删除、回读错误均保留已提交事实，R60及C24文案补充已修；隔离并发回归通过，真实流量链路待验收。 | [proxy](../clash-verge-rev-dev/src/components/proxy) | [proxy_control.rs](../src/core/proxy_control.rs) |
| C25 | 部分 | 链式节点编排 | 入口／中继／出口单行拖排、类型靠右、删除最右、清空撤销与完整YAML已接，无箭头／延迟。Provider按来源及occurrence实例化；有界缓存同步后走原候选流重绑，未知来源拒绝；仅use/include-all的合法目标组可加入物化出口。R05/R32/R37已修；真实平台及来源更新端到端验收仍待补。 | [proxy-chain.tsx](../clash-verge-rev-dev/src/components/proxy/proxy-chain.tsx) | [chain.rs](../src/proxies/chain.rs) |
| C26 | 部分 | 链式状态对齐 | Selector／URLTest应用、重启恢复、既有dialer反解、循环／缺失检查已有；固定出口与实际now分别报告。断开后按目标连接ID核对，已提交后清理失败仍同步编辑器expected／epoch；受控重启后重新取当前Client。R36已修，持久链按确认的缓存来源恢复；真实服务交接验收仍待完成。 | [proxy-groups.tsx](../clash-verge-rev-dev/src/components/proxy/proxy-groups.tsx) | [chain.rs](../src/core/chain.rs) |
| C27 | 待产品决定 | 仪表盘网络测试 | 从当前开发任务独立提出。用户计划之后融入仪表盘，具体测试对象、入口、频率及操作由用户决定；不列为暂缓，也不擅自实现。 | [test-card.tsx](../clash-verge-rev-dev/src/components/home/test-card.tsx) | [view.rs](../src/overview/view.rs) |
| C28 | 部分 | 出口 IP 信息 | 已接国家/旗帜、ASN、组织、地理位置、经纬度、IP 隐藏/显示、独立刷新及进入仪表盘时刷新。刷新走独立监测通道，不重启内核；失败保留上次信息。元数据／偏好／失败保留专项回归已补，真实查询服务与生命周期验收仍待补。 第五批：自动刷新偏好是第五批历史实现，现按用户决定移除，改为进入页面刷新，时区解析与展示已接，失败保留旧元数据及实际原因；后台可见性绘制策略仍归C30。 | [ip-info-card.tsx](../clash-verge-rev-dev/src/components/home/ip-info-card.tsx) | [monitor.rs](../src/core/monitor.rs) |
| C29 | 已有主体 | 仪表盘内存采集 | 本轮按用户决定接入私有 /memory WebSocket，显示 Mihomo inuse 字节值。复用有界刷新、有限重连、心跳、代次隔离及退出清理；断流保留上次真实值并标记非实时，换代清空。解析／断流／旧结果回归已有，真实内核运行验收待补。 | [use-memory-data.ts](../clash-verge-rev-dev/src/hooks/use-memory-data.ts) | [monitor.rs](../src/core/monitor.rs) |
| C30 | 暂缓 | 仪表盘定制 | 仪表盘定制、布局／卡片组合与可见性绘制策略按用户决定暂缓；不据此新增网络／内存／解锁功能。 | [layout-viewer.tsx](../clash-verge-rev-dev/src/components/setting/mods/layout-viewer.tsx) | [view.rs](../src/overview/view.rs) |
| C31 | 部分 | 连接界面 | 保留单一表格，不增加双布局；已接 remoteDestination 回退、完整进程路径、格式化本地完整时间。详情按连接/流量/路由/进程与时间两列分组，可复制完整详情，长值换行。实时/历史、关闭、暂停、列设置和搜索沿用现有能力。 第五批已统一正则优先级及无效表达式行为；连接按完整时间戳排序，关闭后的详情和复制共用零速率投影。 | [connections.tsx](../clash-verge-rev-dev/src/pages/connections.tsx) | [traffic](../src/traffic) |
| C32 | 部分 | 实时规则页 | 已接入 /rules、代次/修订绑定、真实顺序/内容/类型/策略、刷新、虚拟化、类型筛选、高级搜索、复制和详情；失败保留上次数据，不生成命中统计。 第五批已统一共用正则搜索：正则优先整词，无效表达式不回退字面量。 | [rules.tsx](../clash-verge-rev-dev/src/pages/rules.tsx) | [mod.rs](../src/traffic/mod.rs) |
| C33 | 待产品决定 | 仪表盘解锁检测 | 从暂缓清单独立提出，用户计划之后融入仪表盘；服务范围、检测操作及交互由用户决定，不自动加入独立解锁页或仪表盘。 | [src](../clash-verge-rev-dev/crates/clash-verge-media-unlock/src) | [mod.rs](../src/app/mod.rs) |
| C34 | 暂缓 | 多语言 | 语言资源、切换和动态文案。当前中文界面。 | [i18n.ts](../clash-verge-rev-dev/src/services/i18n.ts) | [main.rs](../src/main.rs) |
| C35 | 部分 | 图标与WebUI | HTTP/data/SVG缓存、默认图标和WebUI已有。本地图标导入／复制入口按用户决定暂缓；WebUI真实浏览器与外部访问验收另列。 | [app.rs](../clash-verge-rev-dev/src-tauri/src/cmd/app.rs) | [icons.rs](../src/core/icons.rs) |
| C36 | 部分 | 内核网络/设置完整性 | 端口、listeners、外部控制、DNS／嗅探、TUN、资源和Linux routing-mark已接。TUN标识、bind-address、路由联动、服务Geo暂存和升级回退已修。路由由Mihomo拥有，不另造脚本。剩余真实权限／网卡／防火墙／DNS及故障验收。 | [manager](../clash-verge-rev-dev/src-tauri/src/core/manager) | [launch.rs](../src/core/launch.rs) |
| C37 | 部分 | 日志的服务模式 | Local复用私有/logs WebSocket及子进程补充；Service采用鉴权会话日志窗口补启动历史，250ms有界增量采集，按服务启动身份／代次／序号去重并标记覆盖缺口，保留stderr级别。连续三次失败暂停，目标变化重试；会话绑定拒绝迟到结果。剩余真实服务日志与重启／长时间覆盖验收。 | [use-log-data.ts](../clash-verge-rev-dev/src/hooks/use-log-data.ts) | [output.rs](../src/core/output.rs) |
| C38 | 部分 | 故障恢复与通知 | R01–R61／L01–L05代码修复及S01入口清理完成，已有诊断ZIP及故障上下文；2026-11-05补本次运行的脱敏程序／内核文件日志、内存尾部及导出状态，操作与原始错误原因进入程序日志，Toast不变。通知诊断复制按用户决定暂缓；真实平台故障恢复验收保留。 | [notify](../clash-verge-rev-dev/src-tauri/src/core/notify) | [service.rs](../src/core/service.rs) |
| C39 | 部分 | 安全凭据跨平台 | 订阅／覆写凭据使用macOS钥匙串、Windows Credential Manager、Linux oo7 Secret Service；Keep／Replace／Clear明确意图，单项编辑不清另一项。跨平台适配器编译已有，真实桌面凭据读写验收待补。WebDAV明确不支持。 | [config](../clash-verge-rev-dev/src-tauri/src/config) | [credentials.rs](../src/profiles/credentials.rs) |
| C40 | 待验收 | 跨平台与性能验收 | Windows/Linux 原生运行、权限/服务/TUN/托盘/安装包；macOS 完整窗口工作流、大规模组/连接性能、真实钥匙串、应用更新与退出网络恢复均需单列验收。构建和虚拟化测试不能替代实机验证。 | [src-tauri](../clash-verge-rev-dev/src-tauri) | [系统代理验证范围](system-proxy.md#验证) |
| X01 | 按要求不做 | 偏好裁剪 | 字体、侧栏偏好/折叠导航定制、全局快捷键、轻量模式、自定义调色板；不列为待开发任务。 | [verge.rs](../clash-verge-rev-dev/src-tauri/src/config/verge.rs) | [AGENTS.md](../AGENTS.md) |
| X02 | 设计差异 | 配置与失败契约 | 内核原生 YAML/程序偏好 JSON/默认值/数组、无 applied.json、无旧格式兼容与迁移；不复制 CV 自动换端口、改设置、静默修复/回退。新 dev 已删除内置 JS 目录及 meta_guard，hy_alpn 仍由 Rust 归一化；不作为本项目兼容任务加入。 | [enhance](../clash-verge-rev-dev/src-tauri/src/enhance/mod.rs) | [AGENTS.md](../AGENTS.md) |
| X03 | 设计差异 | 界面与会话状态 | GPUI 原生组件、布尔 Switch、自动保存、间隔 0 关闭、日志倒序及筛选/排序不持久化、资源独立页面、toast 顶部居中。无需复制 Tauri/MUI/WebView devtools。 | [pages](../clash-verge-rev-dev/src/pages) | [Design.md](../DESIGN.md) |
| B01 | 已有主体 | 内核与配置工作流 | 生命周期、归属、就绪、私有IPC、候选校验／热更新／受控重启、回滚及设置恢复已有。可取消退出和OS结束有界清理已修；真实平台网络恢复见C03/C04/C36/C40。 | [manager](../clash-verge-rev-dev/src-tauri/src/core/manager) | [service.rs](../src/core/service.rs) |
| B02 | 已有主体 | 订阅与增强 | 订阅导入、元数据、下载、定时更新、排序／删除／撤销／激活、增强及有序覆写已有；正文文件唯一来源、明确凭据意图、提交后UI同步、首份激活及URL脱敏已修。表单和批量能力剩余见C15。 | [profiles.tsx](../clash-verge-rev-dev/src/pages/profiles.tsx) | [profiles.rs](../src/core/profiles.rs) |
| B03 | 已有主体 | 代理选择与测速 | 真实分组／节点、模式、固定／取消固定、按订阅记忆、独立测速与缓存图标主体已有；R18冷启动有界恢复已修。第五批已修测速输入／执行范围、原始延迟、同批去重及首页组／URL共享，见C20–C26；真实来源与网络验收仍单列。 | [proxies.tsx](../clash-verge-rev-dev/src/pages/proxies.tsx) | [proxy_control.rs](../src/core/proxy_control.rs) |
| B04 | 已有主体 | 资源与内核升级 | GeoIP/GeoSite/MMDB/ASN 更新、规则/代理集合读取更新、内核发行频道/版本下载安装和取消清理。不是应用升级。 | [provider-button.tsx](../clash-verge-rev-dev/src/components/rule/provider-button.tsx) | [resources.rs](../src/core/resources.rs) |
| B05 | 已有主体 | 实时采集与列表 | traffic/connections 私有 WS、监督重连/代次隔离、有界批次、有界会话速率曲线及内核累计量、连接历史、虚拟表格与选中/滚动分离。缺项见 C29/C31/C32。 | [connections.tsx](../clash-verge-rev-dev/src/pages/connections.tsx) | [streams.rs](../src/core/streams.rs) |
| B06 | 已有主体 | 日志与通知 | 日志独立级别／保存、有界列表、倒序及搜索已有；第六批补齐URL及字段定点脱敏、文件错误内存降级、恢复后重试、完整UTF8记录轮转和服务历史游标。toast自动关闭含撤销，持续下载保留。通知诊断复制尚缺。 | [logs.tsx](../clash-verge-rev-dev/src/pages/logs.tsx) | [logging.rs](../src/logging.rs) |
| B07 | 已有主体 | 设置和公共操作 | 原生YAML／程序JSON、默认值／坏文件保护、自动保存、主题／背景、目录／网卡已有。环境变量复制按HTTP／SOCKS有效端口生成并清不可用协议变量，无监听明确报错；状态卡搜索已修。S01入口清理已完成。 | [verge.rs](../clash-verge-rev-dev/src-tauri/src/config/verge.rs) | [mod.rs](../src/preferences/mod.rs) |
| B08 | 已有主体 | Gist完整备份 | 设置→同步提供配置／创建／历史入口。完整当前配置及正文归档、内容摘要／版本／路径校验、私有Gist独立历史／分页／下载／恢复／删除，Age可选；Token与私钥安全存储，备份排除凭据库／本机同步，恢复保留本机登录注册。失败补偿文件、凭据和代理；删除／恢复／凭据清除执行前5秒可撤销。仅隔离模拟验证，未真实上传。 | [GitHub Gist API](https://docs.github.com/en/rest/gists/gists?apiVersion=2026-03-10) | [backup](../src/backup) |

## 导航目的地核对

| CV 目的地 | Suki 状态与缺口 |
| --- | --- |
| home | 真实流量、统计、出口探测已有；C01/C23/C27–C30 |
| proxies | 真实节点与测速已有；C20–C26/C35 |
| profiles | 主体 CRUD/更新/增强已有；C14–C18 |
| connections | 实时采集与关闭已有；C31 |
| rules | 规则集合更新已有；实时规则页主体已接入，见 C32 |
| logs | 子进程/程序采集保存已有；服务模式 C37；会话语义遵循 X03 |
| unlock | C33改列未来仪表盘能力，用户决定操作，不追加独立页面 |
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
| `tray_event` | 部分 | C08 |
| `env_type` | 已有主体 | B07 |
| `start_page` | 已有主体 | B07 |
| `startup_script` | 已有主体 | C06 |
| `traffic_graph` | 部分 | C30 |
| `enable_memory_usage` | 设计差异 | C29：固定展示内存，不新增显示开关 |
| `enable_group_icon` | 已有主体 | B03 |
| `pause_render_traffic_stats_on_blur` | 部分 | C30 |
| `common_tray_icon` | 部分 | C08 |
| `tray_icon` | 部分 | C08 |
| `menu_icon` | 未接入 | 主窗口导航（不属于 C08） |
| `menu_order` | 未接入 | 主窗口导航（不属于 C08） |
| `proxy_group_tools_position` | 部分 | C22 |
| `notice_position` | 已有主体 | B06 |
| `collapse_navbar` | 按要求不做 | X01 |
| `sysproxy_tray_icon` | 部分 | C08 |
| `tun_tray_icon` | 已有主体（自定义 TUN 状态图片） | C08 |
| `enable_tun_mode` | 部分 | C03 |
| `enable_auto_launch` | 已有主体 | C06 |
| `enable_silent_start` | 已有主体 | C06 |
| `enable_system_proxy` | 已有主体 | C02 |
| `enable_proxy_guard` | 已有主体 | C02 |
| `enable_bypass_check` | 已有主体（已恢复） | C02 |
| `enable_dns_settings` | 已有主体 | C18 |
| `profile_dns_settings` | 设计差异 | C18 |
| `use_default_bypass` | 已有主体（已恢复） | C02 |
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
| `auto_close_connection` | 已接执行 | 节点／模式／关闭系统代理均已接；目标ID及并发新连接隔离回归通过，真实流量待验收 |
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
| `enable_tray_speed` | 部分 | C08 |
| `tray_proxy_groups_display_mode` | 部分 | C08 |
| `tray_inline_outbound_modes` | 部分 | C08 |
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
| `restart_app` | 已有主体 | C06 |
| `install_update` | 未接入 | C09 |
| `cancel_update_download` | 未接入 | C09 |
| `restart_core` | 已有主体 | B01 |
| `upgrade_clash_core` | 已有主体 | B04 |
| `get_runtime_state` | 已有主体 | B01 |
| `get_pending_failures` | 部分 | C38 |
| `get_sidecar_failure` | 部分 | C38 |
| `entry_lightweight_mode` | 按要求不做 | X01 |
| `install_service` | 部分 | C04 |
| `uninstall_service` | 已有主体 | C04 |
| `reinstall_service` | 部分 | C04 |
| `repair_service` | 部分 | C04 |
| `continue_with_sidecar` | 已有主体 | C04 |
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
| `invoke_uwp_tool` | 已有主体 | C05 |
| `copy_clash_env` | 已有主体 | B07 |
| `sync_tray_proxy_selection` | 部分 | C08 |
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
| `get_next_update_time` | 无同名入口；按用户决定不纳入本轮待补 | C17 |
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

- GitHub Gist完整备份及Age：本批按用户要求实现；CV参考主要是WebDAV，不能将Gist计为CV原有能力。详见[Gist说明](gist-backup.md)。
- Wi-Fi/SSID 暂停、进程优先级、差异工作目录、部分热加载/自动断开选项：目前仅 schema 或占位动作；没有执行消费者时不得标为完成。其名称不在上述 IVerge 清单，不作为 CV 差距凭空追加。
- 热加载/校验工作流已执行，不表示同名可选设置已控制该行为；选项执行与固定策略分别验收。

## 实施与验收顺序

1. 完善设置故障隔离与原始诊断，维持界面、日志、存储/内核命令可用；已增加坏 JSON 与修正重试回归。
2. 系统代理与 PAC、TUN、平台服务/权限及退出恢复，随后托盘/自启/单实例。
3. JS/远程覆写、全局 DNS、实时规则主体已接入；继续完整来源身份/测速工具/链式编排。
4. 自动备份调度、应用升级/签名交付；WebDAV不纳入产品范围。
5. 网站/解锁/IP/内存/诊断及界面工具补全，分平台完成验收。

## 本轮已补齐及验证范围

- 系统代理：手动／PAC 切换、回环 PAC 服务、应用内原生平台源码、回读／恢复、守卫、退出清理主体；关闭失败后开关跟随实际状态；macOS 回读重新取得服务配置。归属恢复记录已补；尚未完成三平台真实网络及强制退出恢复验收。
- 启动与重置：启动检查内核，缺失显示“不存在内核”；重置程序有真实配置删除与退出流程。此前单独新增的“启动时运行内核”开关已按用户明确要求删除。
- 代理测速：逐节点立即更新数值／颜色，批次结束才排序。
- 增强和撤销：添加位于规则／代理／代理组分类行右上角；增强条目、面板、隧道、文本恢复默认及会话清空补充撤销。所有非进度 toast（含撤销按钮）自动关闭。
- 历史阶段隔离回归（非当前工作区复验）：208 项通过、16 项按真实内核／资源条件跳过；严格 Clippy、相关格式和差异检查通过。本次核查清单未重新运行应用测试。
- macOS 隔离窗口只覆盖本轮相关页面与交互；不等于全部主题／窗口尺寸或真实系统代理启停验收。Windows/Linux 真实桌面、权限、服务、TUN、托盘和安装包仍未验收。

## 需要纠正的历史归因

“绕过校验”和“使用默认绕过”在本地 CV 已存在；用户没有明确要求移除。此前助手简化系统代理界面时自行删除，随后误称“按用户要求移除”，该归因不成立。两项现已恢复开关及校验／合并默认绕过的执行逻辑，不归入 X01/X03 的产品裁剪。

来源设置与未接入入口的逐项清单见 [设置来源核查](settings-origin-audit.md)。CV 的可选布局、额外偏好、备注、多语言等差异仅作为对照事实；本清单不授权自行新增设置。明确排除的 X01–X03 不计作缺失任务。

## 历史审计指纹

用于识别原审计参考快照，不对应当前检出；当前摘要及固定 SHA 见 [版本基线](cv-reference-version.json)。

- `clash-verge-rev-dev/src-tauri/src/config/verge.rs`：SHA-256 `acd20e809b2ddd46d637f40b42bc8df004a397b050cc0bcdac477b4871142a5a`
- `clash-verge-rev-dev/src-tauri/src/ipc.rs`：SHA-256 `a5b4cb5d36e2cdb13e482caa30bb37dd2a919d5b2bae11a9000eb118c10038e7`

配置工作流变更详见 [flow.md](flow.md)；简洁实现审核见 [code-audit.md](code-audit.md)。历史 Backend.md 已从当前工作区移除，不再作为验收记录入口。

## 当前补齐进度与界面修正

本轮完整范围和仍待实施部分见 [实施记录](cv-implementation-progress.md)，C36/C37的服务与平台网络仍需真实集成验收，功能补充项亦未清零，不能把缺陷修复完成当作全部C项完成。

- 代理组取消侧边快速跳转和悬停设置；保留组内定位与拖排。
- 用户订阅／覆写源码不显示恢复默认；设置多行默认值与用户数据区分。
- 订阅信息开关移到末尾并左右对齐，长表单保持一个滚动区域。
- 连接详情分组、完整路径、本地完整日期与时区、复制；不增加列表布局。
- 代理链沿用覆写式单行列表，只拖动排序；类型靠右、删除最右，无延迟及箭头；完整链 YAML 与可视模式双向同步。

历史阶段验证（非当前工作区复验）：214 项普通测试通过、16 项按真实内核／资源条件忽略；格式化、严格 Clippy 和差异检查通过。新增回归覆盖代理链拖动、YAML 双向切换、错误草稿保留、顺序和删除一致性、末尾开关对齐、连接完整路径/时间/IPv6、TLS 策略隔离、来源 occurrence 及诊断脱敏。macOS 隔离窗口检查了订阅开关末尾布局、无侧边跳转、代理链右侧类型/删除及完整 YAML 视口、连接详情分组；数据来自独立测试目录，未操作用户真实系统代理、路由或凭据。该阶段 C36/C37 尚未接入；后续状态以当前总表和复核报告为准，历史记录保留，不宣称整批任务完成。

代理链界面跟进：节点选择占满工具行剩余宽度；模式切换与订阅增强相同，使用 FileCode outline 按钮“YAML 编辑／表单编辑”；应用原生组件统一采用简体中文资源，覆盖所有下拉框默认占位、搜索与空结果提示，不增加语言设置。

2026-10-30 跟进：修复托盘点击回调重复借用实体导致的崩溃，改为原生附着菜单，动作离开托盘实体更新后再分发。系统代理定时轮询不再隐式重试失败的启动应用；networksetup 保留脱敏原因，并识别退出码为 0 的授权失败输出。既有 PAC exit status 4 的底层原因尚无原始输出佐证，未宣称已修复真实 PAC 写入。隔离测试 219 通过、16 忽略；真实网络未修改。

## 2026-10-30 当前补齐与 CV 核查

本轮针对 C01/C02/C06/C07/C08/C20/C25/C26/C22/C37/C38。以 CV 本地 2.5.9 调用链及其 service-ipc v2.7.6 为基准；服务参考源码仅读取，不借用或操作 CV 已安装服务。

- PAC 参数错误：CV 普通 sysproxy 路径会把空 URL 转成字面量双引号传入 networksetup。当前改为 CV 特权路径同类的 SystemConfiguration 原生字典写入；保留其他字段，空 URL 删除键。锁等待有两秒上限，权限失败不自动重试。
- 失败恢复：操作前捕获、写后回读、失败还原检查点；持久化前确认 OS 结果。恢复记录损坏保留文件。恢复／读写失败暂停自动代理处理，显式应用后恢复；不按错误自动换端口或改写设置。
- 流连接：连续失败最多三次后暂停，目标代次／配置变化才重新连接；同类失败不逐条刷 toast。保留上次数据与未知状态。
- 明确未完成：受限提权服务、服务归属交接、安装／修复／服务回退，以及这些路径的平台集成验收。当前实现不能标成全部 C 项完成。

### 同类错误处理审查

| 工作流 | 出错后行为 | 重新执行入口 |
| --- | --- | --- |
| 系统代理初始化／崩溃清理／读写／守卫 | 暂停自动处理，保留错误、归属记录和实际／未知状态；不按轮询重试 | 明确的启用／重新应用，或重新启动 |
| 规则自动读取 | 每个内核代次／配置版本只自动尝试一次；失败保留已有规则 | 明确读取或配置代次变化 |
| Provider 资源自动读取 | 失败停止周期刷新，保留已有数据及错误 | 明确资源请求／内核代次变化 |
| 资源页自动提交失败 | 本代次记为已尝试，避免每个 Session 更新重复请求和 toast | 内核代次变化 |
| 控制状态读取、连接／流量／日志流 | 连续失败三次暂停，不在重连开始时先清空错误制造重复通知 | 内核代次／配置变化 |
| 图标缓存 | 相同来源缓存失败结果，不持续下载；正常缓存仅日志 | 来源变化或新会话 |
| 用户下载／配置应用／订阅更新 | 有界超时，失败保留上次有效文件和运行状态；不切换其他传输方式 | 用户重试；已启用的订阅计划在下一计划周期执行 |

UI：代理组普通／吸顶组头和节点底色不透明，不叠用主窗口透明材质；已排查应用自有固定叠层，其他表格表头由原生组件独立布局，未复制吸顶遮挡逻辑。

2026-10-30 阶段验证（非当前工作区复验）：232 项通过、16 项条件忽略；严格 Clippy、编译、格式与差异检查通过。新增回归证明代理守卫权限失败后连续轮询不再请求、显式应用可恢复；连接／流量／日志连接失败三次暂停，目标变化恢复，重复日志原因只发布一次；认证实例端点拒绝错误令牌／无效导入／超长请求。真实 Mihomo Selector／URLTest 链式恢复回归通过。macOS 隔离窗口已确认不透明组头遮挡下方内容，恢复的绕过开关可见；未修改真实系统代理或安装服务。

本轮 TUN／服务验证与交付边界见 [TUN 与常驻服务](tun-service.md)。服务模式不静默降级普通子进程，未进行真实管理员安装或更改本机路由。

2026-10-31：按用户要求，本轮只完成实现及编译检查，未运行测试或进行真实 OS 修改。新增服务协议为当前 revision 6，不保留旧协议兼容；已安装旧构建的用户需显式修复服务。


2026-11-05 维护与日志补充：[托盘、诊断和操作日志复核](tray-diagnostics-review-2026-11-05.md)。标题开关、UWP真实入口、运行日志导出及完整错误记录已接；Windows/Linux原生托盘、服务和网络操作仍需实机验收，不将源码核对或macOS测试写作跨平台验收通过。
