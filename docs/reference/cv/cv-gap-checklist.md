# Clash Verge Rev 功能对照与差距清单

更新日期：2026-11-09。对照本地 Suki 0.5.4 工作区与既有功能记录；本次定向核查状态冲突及其执行入口，没有重新进行全仓逐文件审计，也没有查询最新上游或发布资产。

用户于 2026-11-09 确认：文档此前列出的实机验收部分均已完成。因此这些条目不再列为待办；完成依据为用户确认，本次文档整理没有重新运行测试或实机操作。各功能文档中的旧日期、测试数量、当时未验收范围保留为历史记录，不覆盖此次确认。此确认不将明确未实现的功能或已知能力限制变为已实现，也不用于推断某个 Release 的附件、证书或签名状态。

## 当前结论与状态口径

R01–R61、L01–L05 及 S01 已完成代码整改；增强表单、批量 URI、嵌套组、Gist／本地完整备份、内核内存、应用更新与 Velopack 交付均已有实现。旧文档把这些能力继续列为“未接入”，或仅因实机验收而标为“部分”，现已更正。已确认的产品边界不计入待补功能。

| 状态 | 含义 |
| --- | --- |
| 已实现 | 已有对应实现；既有实机验收按本次用户确认完成，不表示与 CV 每个接口／策略完全相同 |
| 设计差异／按要求不做 | 已确认的产品边界，不作为缺失功能 |
| 暂缓 | 尚未实现且明确暂缓，不因本次验收确认自动关闭 |
| 待产品决定 | 未来能力，尚待确定具体功能范围 |
| 能力限制／部分 | 存在具体不支持的子能力，必须说明边界，不能只写“待验收” |

## 仍保留的范围与限制

| 类别 | 当前范围 |
| --- | --- |
| 暂缓 | C13 自动备份、C19 完整合成报告、C30 仪表盘定制、C34 多语言、C35 本地图标导入／复制、C38 通知诊断复制 |
| 待产品决定 | C27 仪表盘网络测试、C33 仪表盘解锁检测（含此前登记的 Muse AI 等候选） |
| 已知限制 | C25／C26 高级 regexp2、override-expr 及部分 .NET 特殊替换不支持链物化；普通 Provider 仍交由 Mihomo 执行。C09 下载仅在退出时取消，没有独立取消下载动作 |
| 设计差异 | WebDAV 不支持；全局 DNS、严格下载／TLS、失败保留有效状态、固定导航与设置裁剪、GPUI 托盘浮层、固定组头工具位置及 GLOBAL 默认出口策略均按本项目契约 |
| 后续现场排查 | 11-09 日志中服务／内核管道持续 231 的服务端原因尚未定位；服务日志空闲读取优化仍未接入。这是具体问题及优化项，不继续列作整个平台未验收 |

## 参考基线与证据边界

历史全文阅读基线为 CV 2.5.9／`eefd025`，936 个文本文件的范围见[文件台账](cv-file-review.md)及 CSV。11-03、11-05 增量基线分别为 `5b7e876`、`b9ab3b20`，见[版本与同步记录](cv-upstream-sync.md)。本机参考目录实际 HEAD 为 `cd5156fb8a1363b1d9b3bb09fa034e8068350fa3`，工作树干净；它用于 [11-09 定向检查](../../reviews/logging/lock-screen-requests-2026-11-09.md)，不能将原审计覆盖或旧 JSON 摘要直接套到这个检出。

U01–U03 和 U05 已适配，U04 是配置提交时机参考，U06 保留内核初次默认出口，不自动改选。逐批修复和历史测试保留在[修复清单](../../repairs/cv-repair-checklist.md)及[实施记录](../../repairs/cv-implementation-progress.md)。

## 功能差距总表

编号用于关联后面的命令/设置附录。CV 路径均相对参考目录，Suki 路径相对本仓库。

| 编号 | 状态 | 能力 | 具体差距及现状 | CV 证据 | Suki 证据 |
| --- | --- | --- | --- | --- | --- |
| C01 | 已实现 | 系统信息与诊断 | 已接 OS 版本、架构、主机、实际权限、自启动回读和应用运行时长；仪表盘及详情已接。服务模式与后台服务状态已接。 | [system-info-card.tsx](../../../clash-verge-rev-dev/src/components/home/system-info-card.tsx) | [view.rs](../../../src/overview/view.rs) |
| C02 | 已实现 | 系统代理 | 三平台手动/PAC、回读、守卫、失败恢复与退出清理已接。macOS已接鉴权helper分路及离线清理；Windows默认及RAS逐连接检查点，KDE原值回退已修。权限拒绝停止自动处理，显式操作可重试。退出可取消与系统结束有界清理已接。 | [sysopt.rs](../../../clash-verge-rev-dev/src-tauri/src/core/sysopt.rs) | [macos.rs](../../../src/system_proxy/macos.rs) |
| C03 | 已实现 | TUN 与系统网络 | 常驻服务执行、TUN原生标识、DNS补齐、校验／热更新／回读及失败恢复已接；自动路由／重定向联动提交已修，日常开关不提权。 | [tun.rs](../../../clash-verge-rev-dev/src-tauri/src/enhance/tun.rs) | [TUN 与服务](../../features/network/tun-service.md) |
| C04 | 已实现 | 平台服务 | 维护入口已整合到单一服务程序，安装／卸载不再交付独立可执行文件；[实施记录](../../features/network/tun-service.md)。安装、修复、停止服务内核、卸载及显式转子进程已接；转换保留各阶段提交事实与独立补偿，Unix卸载先确认停止／注销再删除。令牌契约、默认服务归属、健康监督、资源恢复及防火墙部分成功已修。协议现为revision 9，应用与服务同步更新，不提供旧协议兼容。 | [service.rs](../../../clash-verge-rev-dev/src-tauri/src/core/service.rs) | [privileged.rs](../../../src/core/privileged.rs) |
| C05 | 已实现 | Windows 集成 | 采用 ClashSuki 的 FirewallAPI 原生枚举／回环方案，应用选择弹窗可搜索并编辑；未知／已卸载应用豁免保留。服务鉴权写入、并发前置检查、回读与失败恢复；防火墙仅重建服务批准内核及已认证用户应用目录内核的固定命名规则，失败恢复原规则，不接收任意程序路径。无需外部 enableLoopback.exe。 | [win_uwp.rs](../../../clash-verge-rev-dev/src-tauri/src/core/win_uwp.rs) | [windows_network.rs](../../../platform-service/src/windows_network.rs) |
| C06 | 已实现 | 启动与窗口生命周期 | 自启动、静默、关闭到托盘、启动脚本及清理后重启已接。启动协调GUI登录注册与期望值；设置进入与开关操作后自动回读 OS 实际状态并协调漂移，重置清GUI登录注册，不动helper注册。静默建窗隐藏且不聚焦，托盘失败显示主窗。Windows登录任务权限及回退保留原XML。 | [autostart.rs](../../../clash-verge-rev-dev/src-tauri/src/core/autostart.rs) | [autostart_windows.rs](../../../src/desktop/autostart_windows.rs) |
| C07 | 已实现 | 单实例与外部导入 | 当前用户实例锁、认证回环端点、第二实例唤起／订阅导入、明确URI名称、有界队列及协议注册已接。macOS官方on_reopen、单实例与托盘共用显示主窗口入口。 | [server.rs](../../../clash-verge-rev-dev/src-tauri/src/utils/server.rs) | [main.rs](../../../src/main.rs) |
| C08 | 已实现 | 托盘 | 原生状态图标配 GPUI 轻量浮层，不再挂载 OS 菜单；级联定位、虚拟节点列表、选中／滚动保留及统一命令回读已接。固定普通／系统代理／TUN 状态图标，不提供自定义托盘图标；macOS 可显示速率。组内节点无图标，目录／日志入口已删除。属于已确认的界面设计差异。 | [tray](../../../clash-verge-rev-dev/src-tauri/src/core/tray) | [tray](../../../src/tray) |
| C09 | 已实现 | 应用升级 | Velopack 检查、下载、校验、变更记录、安装确认及清理后重启已接，非安装包提供 GitHub 发布页。自动检查设置已按 S01 移除；下载仅退出时取消，未提供独立取消动作。Release 工作流要求各 channel 清单及完整包；历史版本附件情况不作为当前实现缺项，也未在本次联网复核。 | [updater.rs](../../../clash-verge-rev-dev/src-tauri/src/core/updater.rs) | [app_update.rs](../../../src/core/app_update.rs) |
| C10 | 已实现 | 打包与发布 | Velopack 打包及 GitHub 四组原生 runner 工作流已接，发布 7 类用户下载及 channel 更新清单／完整包；全矩阵成功后汇总为单一草稿 Release。当前不签名、不公证。 | [tauri.conf.json](../../../clash-verge-rev-dev/src-tauri/tauri.conf.json) | [bootstrap](../../../src/bootstrap/mod.rs) |
| C11 | 已实现 | 完整备份 | 完整当前配置／订阅／覆写／增强归档、校验及恢复已接入Gist备份。Gist可创建独立历史、分页、下载、恢复、删除与可选Age加密；本地完整历史、文件导入／导出、恢复和删除现已接入维护，共用同一归档与恢复工作流。 | [backup.rs](../../../clash-verge-rev-dev/src-tauri/src/core/backup.rs) | [backup](../../../src/backup) |
| C12 | 按要求不做 | WebDAV | 用户明确仅使用 Gist，完整删除 WebDAV 设置及操作入口，不纳入待实现范围。 | [backup.rs](../../../clash-verge-rev-dev/src-tauri/src/core/backup.rs) | [core_apply.rs](../../../src/settings/core_apply.rs) |
| C13 | 暂缓 | 自动备份 | 定时备份、配置变化触发、保留数量及本地/远程历史策略执行；未实现入口已移除，不以仅持久化的设置宣称支持。 | [auto_backup.rs](../../../clash-verge-rev-dev/src-tauri/src/core/auto_backup.rs) | [schema.rs](../../../src/preferences/schema.rs) |
| C14 | 已实现 | JavaScript 增强 | QuickJS同步main、后台合成、中断／取消、内存／栈／输入输出／日志预算及语法检查已接；执行失败保留有效配置，外部源码文件为唯一正文来源。用户内容无恢复默认。 | [script.rs](../../../clash-verge-rev-dev/src-tauri/src/enhance/script.rs) | [mod.rs](../../../src/configuration/mod.rs) |
| C15 | 已实现 | 增强与远程覆写 | 已接远程YAML／JS及增强编辑；本批补组use、健康检查、筛选／include-all／接口／路由／隐藏等字段及真实来源候选。固定类型中文展示、英文保存，未知字段无损。代理URI批量解析支持前置／追加，原子报行号错误；同名不同内容拒绝，不静默丢弃。按当前协议解析，不增加旧客户端兼容变体。 | [prfitem.rs](../../../clash-verge-rev-dev/src-tauri/src/config/prfitem.rs) | [mod.rs](../../../src/profiles/mod.rs) |
| C16 | 已实现 | 订阅下载选项 | 逐订阅系统／内核代理及忽略证书选项已接，开关互斥且位于表单末尾；按明确策略下载，无自动传输回退，TLS客户端隔离。明确间隔优先和远程结构校验已修。 | [prfitem.rs](../../../clash-verge-rev-dev/src-tauri/src/config/prfitem.rs) | [remote.rs](../../../src/profiles/remote.rs) |
| C17 | 已实现 | 订阅元数据与调度展示 | 按用户确认不再列待补：名称、URL、主页、流量、配额、到期、最后更新时间及自动更新已实现，明确1440／0间隔保留。备注／下一次计划时间不追加为本轮要求。 | [prfitem.rs](../../../clash-verge-rev-dev/src-tauri/src/config/prfitem.rs) | [model.rs](../../../src/profiles/model.rs) |
| C18 | 设计差异 | 全局 DNS | 按用户决定使用全局YAML与开关，单订阅差异通过覆写范围表达；无按订阅DNS开关或来源确认。关闭继承来源，保存／切换／重启统一合成，listen支持:53。 | [dns.rs](../../../clash-verge-rev-dev/src-tauri/src/config/dns.rs) | [flow.rs](../../../src/configuration/flow.rs) |
| C19 | 暂缓 | 运行配置与合成报告 | 运行配置查看／复制已有；完整配置合成报告按用户决定暂缓。 | [runtime.rs](../../../clash-verge-rev-dev/src-tauri/src/cmd/runtime.rs) | [actions.rs](../../../src/settings/actions.rs) |
| C20 | 已实现 | 代理来源身份 | 来源ID／occurrence、Provider作用范围、稳定缓存同步、来源证明和冷启动选择恢复已有。本批补嵌套组当前路径、实际叶协议与已回读能力；循环／缺失／歧义保持未知，保留选择身份。冷启动链缺确认缓存时明确拒绝。 | [proxy_view.rs](../../../clash-verge-rev-dev/src-tauri/src/core/proxy_view.rs) | [runtime.rs](../../../src/proxies/runtime.rs) |
| C21 | 已实现 | 代理高级搜索 | 已接大小写、整词、正则、delay 比较及 timeout/error、type=、独立分组过滤；无效正则不匹配并报错，数值相等与 timeout 区间分开。搜索选项位于搜索框内。第五批已清理失效组筛选，清除动作覆盖全部筛选；数值搜索保留原始延迟，超时／失败按明确状态筛选。 | [proxy](../../../clash-verge-rev-dev/src/components/proxy) | [model.rs](../../../src/proxies/model.rs) |
| C22 | 已实现 | 分组工具与导航 | 逐组测速地址按订阅保存，首页／手动／自动测速共用；组头单行、拖动排序、虚拟列表吸顶与返回顶部已有。工具固定在组头右侧，不提供位置设置；不透明底色、无侧边快速跳转均为产品决定。 | [proxy](../../../clash-verge-rev-dev/src/components/proxy) | [view.rs](../../../src/proxies/view.rs) |
| C23 | 已实现 | 测速自动化 | 已接自动当前节点检测开关/间隔执行与首页快捷测速，复用同一并发、去重和代次隔离通道；设置位于通用设置，快捷测试位于首页策略节点旁。专门的自动调度／间隔／取消／迟到结果回归已补。 第五批：首页组选择按订阅记忆，自动测速读取同一组选择与测速URL；输入范围与执行一致，批次按来源身份去重，保留原始延迟并明确区分超时／失败。 | [current-proxy-card.tsx](../../../clash-verge-rev-dev/src/components/home/current-proxy-card.tsx) | [monitor.rs](../../../src/core/monitor.rs) |
| C24 | 已实现 | 节点切换关闭连接 | AutoCloseConnection已接节点／取消固定、模式切换与关闭系统代理。旧节点只清chains含原节点的快照ID，全清同样按快照ID确认；并发新连接不误关／误报。初查、删除、回读错误均保留已提交事实，R60及C24文案补充已修；隔离并发回归通过。 | [proxy](../../../clash-verge-rev-dev/src/components/proxy) | [proxy_control.rs](../../../src/core/proxy_control.rs) |
| C25 | 能力限制 | 链式节点编排 | 入口／中继／出口拖排、YAML、来源／occurrence 绑定、缓存确认与物化出口已实现。常规 Provider 覆盖及重命名已接；高级 regexp2、override-expr 和部分 .NET 特殊替换明确拒绝链物化并保留旧链，详见本地备份及差异处理文档。 | [proxy-chain.tsx](../../../clash-verge-rev-dev/src/components/proxy/proxy-chain.tsx) | [chain.rs](../../../src/proxies/chain.rs) |
| C26 | 能力限制 | 链式状态对齐 | Selector／URLTest 应用、重启恢复、既有 dialer 反解、循环／缺失检查、固定出口与实际 now 回读及断开清理已实现。恢复仍要求已确认来源缓存，表达式限制同 C25。 | [proxy-groups.tsx](../../../clash-verge-rev-dev/src/components/proxy/proxy-groups.tsx) | [chain.rs](../../../src/core/chain.rs) |
| C27 | 待产品决定 | 仪表盘网络测试 | 从当前开发任务独立提出。用户计划之后融入仪表盘，具体测试对象、入口、频率及操作由用户决定；不列为暂缓，也不擅自实现。 | [test-card.tsx](../../../clash-verge-rev-dev/src/components/home/test-card.tsx) | [view.rs](../../../src/overview/view.rs) |
| C28 | 已实现 | 出口 IP 信息 | 国家／旗帜、ASN、组织、地理位置、经纬度、时区、隐藏／显示与独立刷新已有。进入或再次点击仪表盘刷新，失败保留旧信息及真实原因；已移除 IP 自动刷新偏好与旧周期策略。 | [ip-info-card.tsx](../../../clash-verge-rev-dev/src/components/home/ip-info-card.tsx) | [monitor.rs](../../../src/core/monitor.rs) |
| C29 | 已实现 | 仪表盘内存采集 | 本轮按用户决定接入私有 /memory WebSocket，显示 Mihomo inuse 字节值。复用有界刷新、有限重连、心跳、代次隔离及退出清理；断流保留上次真实值并标记非实时，换代清空。解析／断流／旧结果回归已有。 | [use-memory-data.ts](../../../clash-verge-rev-dev/src/hooks/use-memory-data.ts) | [monitor.rs](../../../src/core/monitor.rs) |
| C30 | 暂缓 | 仪表盘定制 | 仅布局定制／卡片组合暂缓。可见性策略已在 11-09 实现：可见时 15 秒代理读取、30 秒状态兜底，隐藏抑制常规通知、恢复刷新；后台采集及账本继续。不能将它继续列作定制功能的缺项。 | [layout-viewer.tsx](../../../clash-verge-rev-dev/src/components/setting/mods/layout-viewer.tsx) | [view.rs](../../../src/overview/view.rs) |
| C31 | 已实现 | 连接界面 | 保留单一表格，不增加双布局；已接 remoteDestination 回退、完整进程路径、格式化本地完整时间。详情按连接/流量/路由/进程与时间两列分组，可复制完整详情，长值换行。实时/历史、关闭、暂停、列设置和搜索沿用现有能力。 第五批已统一正则优先级及无效表达式行为；连接按完整时间戳排序，关闭后的详情和复制共用零速率投影。 | [connections.tsx](../../../clash-verge-rev-dev/src/pages/connections.tsx) | [traffic](../../../src/traffic) |
| C32 | 已实现 | 实时规则页 | 已接入 /rules、代次/修订绑定、真实顺序/内容/类型/策略、刷新、虚拟化、类型筛选、高级搜索、复制和详情；失败保留上次数据，不生成命中统计。 第五批已统一共用正则搜索：正则优先整词，无效表达式不回退字面量。 | [rules.tsx](../../../clash-verge-rev-dev/src/pages/rules.tsx) | [mod.rs](../../../src/traffic/mod.rs) |
| C33 | 待产品决定 | 仪表盘解锁检测 | 从暂缓清单独立提出，用户计划之后融入仪表盘；服务范围、检测操作及交互由用户决定，不自动加入独立解锁页或仪表盘。 | [src](../../../clash-verge-rev-dev/crates/clash-verge-media-unlock/src) | [mod.rs](../../../src/app/mod.rs) |
| C34 | 暂缓 | 多语言 | 语言资源、切换和动态文案。当前中文界面。 | [i18n.ts](../../../clash-verge-rev-dev/src/services/i18n.ts) | [main.rs](../../../src/main.rs) |
| C35 | 部分 | 图标与 WebUI | HTTP／data／SVG 缓存、默认图标及 WebUI 已实现；本地图标导入／复制入口明确暂缓。普通图标缓存只记录日志，失败保留默认图标与操作能力。 | [app.rs](../../../clash-verge-rev-dev/src-tauri/src/cmd/app.rs) | [icons.rs](../../../src/core/icons.rs) |
| C36 | 已实现 | 内核网络/设置完整性 | 端口、listeners、外部控制、DNS／嗅探、TUN、资源和Linux routing-mark已接。TUN标识、bind-address、路由联动、服务Geo暂存和升级回退已修。路由由Mihomo拥有，不另造脚本。 | [manager](../../../clash-verge-rev-dev/src-tauri/src/core/manager) | [launch.rs](../../../src/core/launch.rs) |
| C37 | 已实现 | 日志的服务模式 | Local复用私有/logs WebSocket及子进程补充；Service采用鉴权会话日志窗口补启动历史，250ms有界增量采集，按服务启动身份／代次／序号去重并标记覆盖缺口，保留stderr级别。连续三次失败暂停，目标变化重试；会话绑定拒绝迟到结果。 | [use-log-data.ts](../../../clash-verge-rev-dev/src/hooks/use-log-data.ts) | [output.rs](../../../src/core/output.rs) |
| C38 | 已实现 | 故障恢复与通知 | R01–R61／L01–L05代码修复及S01入口清理完成，已有诊断ZIP及故障上下文；2026-11-05补本次运行的脱敏程序／内核文件日志、内存尾部及导出状态，操作与原始错误原因进入程序日志，Toast不变。通知诊断复制按用户决定暂缓。 | [notify](../../../clash-verge-rev-dev/src-tauri/src/core/notify) | [service.rs](../../../src/core/service.rs) |
| C39 | 已实现 | 安全凭据跨平台 | 订阅／覆写凭据使用macOS钥匙串、Windows Credential Manager、Linux oo7 Secret Service；Keep／Replace／Clear明确意图，单项编辑不清另一项。WebDAV明确不支持。 | [config](../../../clash-verge-rev-dev/src-tauri/src/config) | [credentials.rs](../../../src/profiles/credentials.rs) |
| C40 | 已完成验收 | 跨平台与性能验收 | 此前列出的实机验收项按用户于 2026-11-09 的确认关闭。本次未重新运行这些验收；历史自动测试和跨平台构建记录保持原日期与范围。 | [src-tauri](../../../clash-verge-rev-dev/src-tauri) | [Velopack 打包与更新](../../build/velopack.md) |
| X01 | 按要求不做 | 偏好裁剪 | 字体、侧栏偏好／导航定制（保留 Logo 折叠入口）、全局快捷键、轻量模式、自定义调色板；不列为待开发任务。 | [verge.rs](../../../clash-verge-rev-dev/src-tauri/src/config/verge.rs) | [AGENTS.md](../../../AGENTS.md) |
| X02 | 设计差异 | 配置与失败契约 | 内核原生 YAML/程序偏好 JSON/默认值/数组、无 applied.json、无旧格式兼容与迁移；不复制 CV 自动换端口、改设置、静默修复/回退。新 dev 已删除内置 JS 目录及 meta_guard，hy_alpn 仍由 Rust 归一化；不作为本项目兼容任务加入。 | [enhance](../../../clash-verge-rev-dev/src-tauri/src/enhance/mod.rs) | [AGENTS.md](../../../AGENTS.md) |
| X03 | 设计差异 | 界面与会话状态 | GPUI 原生组件、布尔 Switch、自动保存、间隔 0 关闭、日志倒序及筛选/排序不持久化、资源独立页面、toast 顶部居中。无需复制 Tauri/MUI/WebView devtools。 | [pages](../../../clash-verge-rev-dev/src/pages) | [Design.md](../../../DESIGN.md) |
| B01 | 已实现 | 内核与配置工作流 | 生命周期、归属、就绪、私有IPC、候选校验／热更新／受控重启、回滚及设置恢复已有。可取消退出和OS结束有界清理已修；真实平台网络恢复见C03/C04/C36/C40。 | [manager](../../../clash-verge-rev-dev/src-tauri/src/core/manager) | [service.rs](../../../src/core/service.rs) |
| B02 | 已实现 | 订阅与增强 | 订阅导入、元数据、下载、定时更新、排序／删除／撤销／激活、增强及有序覆写已有；正文文件唯一来源、明确凭据意图、提交后UI同步、首份激活及URL脱敏已修。表单与批量能力已补齐，见 C15。 | [profiles.tsx](../../../clash-verge-rev-dev/src/pages/profiles.tsx) | [profiles.rs](../../../src/core/profiles.rs) |
| B03 | 已实现 | 代理选择与测速 | 真实分组／节点、模式、固定／取消固定、按订阅记忆、独立测速与缓存图标主体已有；R18冷启动有界恢复已修。第五批已修测速输入／执行范围、原始延迟、同批去重及首页组／URL共享，见C20–C26。 | [proxies.tsx](../../../clash-verge-rev-dev/src/pages/proxies.tsx) | [proxy_control.rs](../../../src/core/proxy_control.rs) |
| B04 | 已实现 | 资源与内核升级 | GeoIP/GeoSite/MMDB/ASN 更新、规则/代理集合读取更新、内核发行频道/版本下载安装和取消清理。不是应用升级。 | [provider-button.tsx](../../../clash-verge-rev-dev/src/components/rule/provider-button.tsx) | [resources.rs](../../../src/core/resources.rs) |
| B05 | 已实现 | 实时采集与列表 | traffic/connections 私有 WS、监督重连/代次隔离、有界批次、有界会话速率曲线、今日／本月已观察流量账本及小时统计、连接历史、虚拟表格与选中/滚动分离。内存、连接与规则分别见 C29/C31/C32。 | [connections.tsx](../../../clash-verge-rev-dev/src/pages/connections.tsx) | [streams.rs](../../../src/core/streams.rs) |
| B06 | 已实现 | 日志与通知 | 日志独立级别／保存、有界列表、倒序及搜索已有；第六批补齐URL及字段定点脱敏、文件错误内存降级、恢复后重试、完整UTF8记录轮转和服务历史游标。toast自动关闭含撤销，持续下载保留。通知诊断复制明确暂缓。 | [logs.tsx](../../../clash-verge-rev-dev/src/pages/logs.tsx) | [logging.rs](../../../src/logging.rs) |
| B07 | 已实现 | 设置和公共操作 | 原生YAML／程序JSON、默认值／坏文件保护、自动保存、主题／背景、目录／网卡已有。环境变量复制按HTTP／SOCKS有效端口生成并清不可用协议变量，无监听明确报错；状态卡搜索已修。S01入口清理已完成。 | [verge.rs](../../../clash-verge-rev-dev/src-tauri/src/config/verge.rs) | [mod.rs](../../../src/preferences/mod.rs) |
| B08 | 已实现 | Gist完整备份 | 设置→维护提供配置／创建／历史入口。完整当前配置及正文归档、内容摘要／版本／路径校验、私有Gist独立历史／分页／下载／恢复／删除，Age可选；Token与私钥安全存储，备份排除凭据库／本机同步，恢复保留本机登录注册。失败补偿文件、凭据和代理；删除／恢复／凭据清除执行前5秒可撤销。 | [GitHub Gist API](https://docs.github.com/en/rest/gists/gists?apiVersion=2026-03-10) | [backup](../../../src/backup) |

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
| 节点 delay、Provider healthcheck | B03；高级过滤/逐组地址／自动化已接 C21–C23 |
| /providers/proxies、/providers/rules 与更新 | B04，外部资源页 |
| /rules | C32，已接入 |
| /connections、关闭单项/全部 | B05；连接界面见 C31 |
| WS traffic、connections | B05；有重连/代次监督 |
| WS logs | 子进程复用私有 /logs 并补 stdout/stderr；服务日志 C37 |
| WS memory | C29 |
| updateGeo | B04 |

这里按真实能力映射，不要求引入 CV 的 Tauri 命令、状态库或 WebView。

## 设置字段完整核对（75 项）

字段来自 `src-tauri/src/config/verge.rs::IVerge`，按原始顺序列出。状态是该字段对应功能的覆盖程度，不表示要求照搬同名字段。复合字段详情见功能表。

| CV 字段 | 对照状态 | 功能编号 |
| --- | --- | --- |
| `app_log_level` | 已实现 | B06 |
| `app_log_max_size` | 已实现 | B06 |
| `app_log_max_count` | 已实现 | B06 |
| `language` | 暂缓 | C34 |
| `theme_mode` | 已实现 | B07 |
| `tray_event` | 已实现 | C08 |
| `env_type` | 已实现 | B07 |
| `start_page` | 已实现 | B07 |
| `startup_script` | 已实现 | C06 |
| `traffic_graph` | 设计差异 | C30 |
| `enable_memory_usage` | 设计差异 | C29：固定展示内存，不新增显示开关 |
| `enable_group_icon` | 已实现 | B03 |
| `pause_render_traffic_stats_on_blur` | 设计差异（隐藏抑制，非失焦暂停） | C30 |
| `common_tray_icon` | 设计差异 | C08 |
| `tray_icon` | 设计差异 | C08 |
| `menu_icon` | 设计差异 | 主窗口导航（不属于 C08） |
| `menu_order` | 设计差异 | 主窗口导航（不属于 C08） |
| `proxy_group_tools_position` | 设计差异 | C22 |
| `notice_position` | 已实现 | B06 |
| `collapse_navbar` | 按要求不做 | X01 |
| `sysproxy_tray_icon` | 设计差异 | C08 |
| `tun_tray_icon` | 设计差异 | C08 |
| `enable_tun_mode` | 已实现 | C03 |
| `enable_auto_launch` | 已实现 | C06 |
| `enable_silent_start` | 已实现 | C06 |
| `enable_system_proxy` | 已实现 | C02 |
| `enable_proxy_guard` | 已实现 | C02 |
| `enable_bypass_check` | 已实现 | C02 |
| `enable_dns_settings` | 已实现 | C18 |
| `profile_dns_settings` | 设计差异 | C18 |
| `use_default_bypass` | 已实现 | C02 |
| `system_proxy_bypass` | 已实现 | C02 |
| `proxy_guard_duration` | 已实现 | C02 |
| `proxy_auto_config` | 已实现 | C02 |
| `pac_file_content` | 已实现 | C02 |
| `proxy_host` | 已实现 | C02 |
| `theme_setting` | 按要求不做 | X01 |
| `web_ui_list` | 已实现 | C35 |
| `clash_core` | 已实现 | B04 |
| `hotkeys` | 按要求不做 | X01 |
| `enable_global_hotkey` | 按要求不做 | X01 |
| `home_cards` | 设计差异 | C30 |
| `auto_close_connection` | 已接执行 | 节点／模式／关闭系统代理均已接；目标ID及并发新连接隔离回归通过；实机验收按 11-09 用户确认完成 |
| `auto_check_update` | 按要求不做（S01 已移除） | C09 |
| `default_latency_test` | 已实现 | B03 |
| `default_latency_timeout` | 已实现 | B03 |
| `enable_auto_delay_detection` | 已接执行 | C23；专项回归已补 |
| `auto_delay_detection_interval_minutes` | 已接执行 | C23；专项回归已补 |
| `enable_builtin_enhanced` | 设计差异 | X02 |
| `proxy_layout_column` | 已实现 | B03 |
| `test_list` | 待产品决定 | C27 |
| `auto_log_clean` | 已实现 | B06 |
| `enable_auto_backup_schedule` | 暂缓 | C13 |
| `auto_backup_interval_hours` | 暂缓 | C13 |
| `auto_backup_on_change` | 暂缓 | C13 |
| `verge_redir_port` | 已实现 | C36 |
| `verge_redir_enabled` | 已实现 | C36 |
| `verge_tproxy_port` | 已实现 | C36 |
| `verge_tproxy_enabled` | 已实现 | C36 |
| `verge_mixed_port` | 已实现 | B01 |
| `verge_socks_port` | 已实现 | B01 |
| `verge_socks_enabled` | 已实现 | B01 |
| `verge_port` | 已实现 | B01 |
| `verge_http_enabled` | 已实现 | B01 |
| `webdav_url` | 按要求不做 | C12 |
| `webdav_username` | 按要求不做 | C12 |
| `webdav_password` | 按要求不做 | C12 |
| `enable_tray_speed` | 已实现 | C08 |
| `tray_proxy_groups_display_mode` | 已实现 | C08 |
| `tray_inline_outbound_modes` | 已实现 | C08 |
| `enable_auto_light_weight_mode` | 按要求不做 | X01 |
| `auto_light_weight_minutes` | 按要求不做 | X01 |
| `enable_hover_jump_navigator` | 按要求不做 | 最新决定不提供侧边快速跳转 |
| `hover_jump_navigator_delay` | 按要求不做 | 最新决定移除关联设置 |
| `enable_external_controller` | 已实现 | B01 |

## 注册命令完整核对（92 项）

来源为 `src-tauri/src/ipc.rs::collect_commands!`。同等能力通过 Suki 类型化执行端实现即可；附录不是新增 Tauri 接口计划。JS 与远程覆写已单独接入；订阅/增强命令的覆盖程度仍按 C14/C15 及各自测试判断。

| CV 命令 | 对照状态 | 功能编号 |
| --- | --- | --- |
| `get_system_info` | 已实现 | C01 |
| `get_app_uptime` | 已实现 | C01 |
| `export_diagnostic_info` | 已实现 | C01 |
| `probe_listener` | 已实现 | B01 |
| `save_proxy_ports` | 已实现 | B01 |
| `get_sys_proxy` | 已实现 | C02 |
| `get_auto_proxy` | 已实现 | C02 |
| `get_embedded_server_port` | 已实现 | C02 |
| `open_app_dir` | 已实现 | B07 |
| `open_logs_dir` | 已实现 | B07 |
| `open_core_dir` | 已实现 | B07 |
| `get_network_interfaces` | 已实现 | B07 |
| `get_system_hostname` | 已实现 | C01 |
| `restart_app` | 已实现 | C06 |
| `install_update` | 已实现 | C09 |
| `cancel_update_download` | 部分（仅退出取消，无独立动作） | C09 |
| `restart_core` | 已实现 | B01 |
| `upgrade_clash_core` | 已实现 | B04 |
| `get_runtime_state` | 已实现 | B01 |
| `get_pending_failures` | 已实现 | C38 |
| `get_sidecar_failure` | 已实现 | C38 |
| `entry_lightweight_mode` | 按要求不做 | X01 |
| `install_service` | 已实现 | C04 |
| `uninstall_service` | 已实现 | C04 |
| `reinstall_service` | 已实现 | C04 |
| `repair_service` | 已实现 | C04 |
| `continue_with_sidecar` | 已实现 | C04 |
| `sync_runtime_providers` | 已实现 | B04 |
| `get_clash_info` | 已实现 | B01 |
| `patch_clash_config` | 已实现 | B01 |
| `patch_clash_mode` | 已实现 | B01 |
| `get_clash_mode` | 已实现 | B01 |
| `change_clash_core` | 已实现 | B04 |
| `get_runtime_config` | 已实现 | C19 |
| `get_proxy_view` | 已实现 | C20 |
| `get_runtime_yaml` | 已实现 | C19 |
| `get_runtime_logs` | 暂缓（完整合成报告） | C19 |
| `get_runtime_proxy_chain_config` | 能力限制同 C25／C26 | C26 |
| `update_proxy_chain_config_in_runtime` | 能力限制同 C25／C26 | C26 |
| `invoke_uwp_tool` | 已实现 | C05 |
| `copy_clash_env` | 已实现 | B07 |
| `sync_tray_proxy_selection` | 已实现 | C08 |
| `record_selected_node` | 已实现 | B03 |
| `forget_selected_node` | 已实现 | B03 |
| `save_dns_config` | 已实现 | C18 |
| `apply_dns_config` | 已实现 | C18 |
| `set_dns_override` | 设计差异 | C18 |
| `take_dns_override_notice` | 设计差异 | C18 |
| `take_service_fallback_notice` | 已实现 | C38 |
| `get_core_startup_error` | 已实现 | B01 |
| `take_service_repair_notice` | 已实现 | C38 |
| `take_service_owner_notice` | 已实现 | C38 |
| `take_discarded_keys_notice` | 设计差异 | X02 |
| `get_dns_config_content` | 已实现 | C18 |
| `validate_dns_config` | 已实现 | C18 |
| `get_clash_logs` | 已实现 | B06 |
| `get_verge_config` | 已实现 | B07 |
| `patch_verge_config` | 已实现 | B07 |
| `test_delay` | 已实现 | B03 |
| `get_app_dir` | 已实现 | B07 |
| `copy_icon_file` | 暂缓 | C35 |
| `download_icon_cache` | 已实现 | C35 |
| `open_devtools` | 设计差异 | X03 |
| `exit_app` | 已实现 | B07 |
| `get_network_interfaces_info` | 已实现 | B07 |
| `get_profiles` | 已实现 | B02 |
| `enhance_profiles` | 已实现 | B02 |
| `patch_profiles_config` | 已实现 | B02 |
| `view_profile` | 已实现 | B02 |
| `patch_profile` | 已实现 | B02 |
| `create_profile` | 已实现 | B02 |
| `import_profile` | 已实现 | B02 |
| `reorder_profile` | 已实现 | B02 |
| `update_profile` | 已实现 | B02 |
| `delete_profile` | 已实现 | B02 |
| `read_profile_file` | 已实现 | B02 |
| `save_profile_file` | 已实现 | B02 |
| `get_next_update_time` | 无同名入口；按用户决定不纳入本轮待补 | C17 |
| `create_local_backup` | 已实现 | C11 |
| `list_local_backup` | 已实现 | C11 |
| `delete_local_backup` | 已实现 | C11 |
| `restore_local_backup` | 已实现 | C11 |
| `import_local_backup` | 已实现 | C11 |
| `export_local_backup` | 已实现 | C11 |
| `create_webdav_backup` | 按要求不做 | C12 |
| `save_webdav_config` | 按要求不做 | C12 |
| `list_webdav_backup` | 按要求不做 | C12 |
| `delete_webdav_backup` | 按要求不做 | C12 |
| `restore_webdav_backup` | 按要求不做 | C12 |
| `get_unlock_items` | 待产品决定 | C33 |
| `check_media_unlock` | 待产品决定 | C33 |
| `check_media_unlock_item` | 待产品决定 | C33 |

## 不归入 CV 待补范围的本项目功能与裁剪

- Gist 完整备份及 Age、本地完整历史已实现，详见 [Gist](../../features/backup/gist-backup.md)及[本地备份](../../features/backup/local-backup-and-parity.md)。
- Wi-Fi／SSID、进程优先级、独立工作目录和无消费者的可选热加载设置已按 S01 删除，不再记录为占位实现；已实现的自动断开与统一热加载工作流保留。
- 今日／本月与 24 小时流量账本已按 11-08 产品决定实现，保存 `cache/traffic.json`，仅保留本月已观察增量；代理／域名／进程排行仍仅保留当前内核生命周期。见[仪表盘统计](../../features/runtime/dashboard-data.md)。

## 历史验证记录

以下保留早期修复记录的实际检查范围，其中“尚未验收”只表示当时状态；当前实机验收结论见本文开头的 2026-11-09 用户确认。

## 本轮已补齐及验证范围

- 系统代理：手动／PAC 切换、回环 PAC 服务、应用内原生平台源码、回读／恢复、守卫、退出清理主体；关闭失败后开关跟随实际状态；macOS 回读重新取得服务配置。归属恢复记录已补；尚未完成三平台真实网络及强制退出恢复验收。
- 启动与重置：启动检查内核，缺失显示“不存在内核”；重置程序有真实配置删除与退出流程。此前单独新增的“启动时运行内核”开关已按用户明确要求删除。
- 代理测速：逐节点立即更新数值／颜色，批次结束才排序。
- 增强和撤销：添加位于规则／代理／代理组分类行右上角；增强条目、面板、隧道、文本恢复默认及会话清空补充撤销。所有非进度 toast（含撤销按钮）自动关闭。
- 历史阶段隔离回归（非当前工作区复验）：208 项通过、16 项按真实内核／资源条件跳过；严格 Clippy、相关格式和差异检查通过。本次核查清单未重新运行应用测试。
- macOS 隔离窗口只覆盖本轮相关页面与交互；不等于全部主题／窗口尺寸或真实系统代理启停验收。Windows/Linux 真实桌面、权限、服务、TUN、托盘和安装包仍未验收。

## 需要纠正的历史归因

“绕过校验”和“使用默认绕过”在本地 CV 已存在；用户没有明确要求移除。此前助手简化系统代理界面时自行删除，随后误称“按用户要求移除”，该归因不成立。两项现已恢复开关及校验／合并默认绕过的执行逻辑，不归入 X01/X03 的产品裁剪。

历史来源与已清理入口的逐项记录见 [设置来源核查](../../features/settings/settings-origin-audit.md)。CV 的可选布局、额外偏好、备注、多语言等差异仅作为对照事实；本清单不授权自行新增设置。明确排除的 X01–X03 不计作缺失任务。

## 历史审计指纹

用于识别原审计参考快照，不对应当前检出；当前摘要及固定 SHA 见 [版本基线](cv-reference-version.json)。

- `clash-verge-rev-dev/src-tauri/src/config/verge.rs`：SHA-256 `acd20e809b2ddd46d637f40b42bc8df004a397b050cc0bcdac477b4871142a5a`
- `clash-verge-rev-dev/src-tauri/src/ipc.rs`：SHA-256 `a5b4cb5d36e2cdb13e482caa30bb37dd2a919d5b2bae11a9000eb118c10038e7`

配置工作流变更详见 [flow.md](../../features/configuration/flow.md)；简洁实现要求统一维护在 [开发约定](../../../AGENTS.md)。历史 Backend.md 已从当前工作区移除，不再作为验收记录入口。

## 当前补齐进度与界面修正

该阶段的实施范围保留在[实施记录](../../repairs/cv-implementation-progress.md)。C36／C37 后续已接入，实机验收按 11-09 用户确认完成；当前暂缓、待决定与具体限制见本文开头。

- 代理组取消侧边快速跳转和悬停设置；保留组内定位与拖排。
- 用户订阅／覆写源码不显示恢复默认；设置多行默认值与用户数据区分。
- 订阅信息开关移到末尾并左右对齐，长表单保持一个滚动区域。
- 连接详情分组、完整路径、本地完整日期与时区、复制；不增加列表布局。
- 代理链沿用覆写式单行列表，只拖动排序；类型靠右、删除最右，无延迟及箭头；完整链 YAML 与可视模式双向同步。

历史阶段验证（非当前工作区复验）：214 项普通测试通过、16 项按真实内核／资源条件忽略；格式化、严格 Clippy 和差异检查通过。新增回归覆盖代理链拖动、YAML 双向切换、错误草稿保留、顺序和删除一致性、末尾开关对齐、连接完整路径/时间/IPv6、TLS 策略隔离、来源 occurrence 及诊断脱敏。macOS 隔离窗口检查了订阅开关末尾布局、无侧边跳转、代理链右侧类型/删除及完整 YAML 视口、连接详情分组；数据来自独立测试目录，未操作用户真实系统代理、路由或凭据。该阶段 C36/C37 尚未接入；后续状态以当前总表和修复清单为准，历史记录保留，不宣称整批任务完成。

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

本轮 TUN／服务验证与交付边界见 [TUN 与常驻服务](../../features/network/tun-service.md)。服务模式不静默降级普通子进程，未进行真实管理员安装或更改本机路由。

2026-10-31：按用户要求，本轮只完成实现及编译检查，未运行测试或进行真实 OS 修改。新增服务协议为当前 revision 6，不保留旧协议兼容；已安装旧构建的用户需显式修复服务。


维护与日志补充见 [日志](../../features/logging/logging.md)和[诊断与托盘](../../features/logging/diagnostics-tray.md)。标题开关、UWP真实入口、运行日志导出及完整错误记录已接；Windows/Linux原生托盘、服务和网络操作仍需实机验收，不将源码核对或macOS测试写作跨平台验收通过。

2026-11-07 开机启动跟进：[开机启动与服务错误复核](../../repairs/runtime-readiness.md)。区分服务尚未可连接与明确拒绝，统一有界等待、失败原因保留和启动服务日志采集；Windows 冷启动及实际 TUN 驱动仍需实机复测。
