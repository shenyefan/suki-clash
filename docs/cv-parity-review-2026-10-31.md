# 已实现功能与 CV 的逐项源码复核

开始：2026-10-31；续查更新：2026-11-02。对象为当前工作区（基于 `6ea405e`，含未提交实现），不是该提交的静态内容，也不是发布验收。参考本地 CV 2.5.9；未查询线上最新版。本轮只复核与整理文档，未修改产品源码，未运行测试或改变本机网络、服务、凭据。续查纳入正常启动、隐藏／显示、登录启动、退出、重启、注销与服务独立生命周期，不只审查错误分支。

结论：不能认定已完成内容与 CV 全面对齐。主路径已有，但发现 **61 项代码差距（R01–R61，其中 R09–R61 为逐文件续查新增）**，续查增加 **5 项桌面生命周期差距（L01–L05）**，另有 **1 组入口／授权范围问题（S01）**；既有缺功能与平台验收单独列出。下表中的“主体对齐”只指列出的源码路径与行为，不能代替逐像素或平台验收。

修复前最优先：**R29 服务会话令牌长度冲突会阻断服务启动**；其次为 R30 macOS helper 代理分路、R34 正常服务选择，以及 R38/R39 的服务资源发布／升级回退。不能沿用此前“服务模式已完整实现、只待验证”的结论。

实施任务见 [修复清单](cv-repair-checklist.md)：保留全部问题编号，按依赖与优先级分批，列出修复边界和完成条件；待确认语义、缺功能与平台验收单独记录。

## 第一批修复状态（2026-11-02）

R29、R34、R30、R04、R47、R42、R45及L02、L04已完成代码修复，具体实现与检查见[修复清单](cv-repair-checklist.md)。根项目246项测试通过、16项忽略，check／严格Clippy通过；真实服务、权限、退出与卸载平台验收尚未完成。其余54项R、3项L及S01仍待处理。

下文问题描述保留为修复前证据，不再将上述九项当作当前未改代码。全文台账记录修复前源码基线，新增／调整模块不自动继承其哈希覆盖。

## 第二批修复状态（2026-11-02）

R02/R03/R39/R44/R38/R40/R41/R33/R01/R09/R55/R21/R46/R43已完成代码修复，检查及实际平台限制见[修复清单](cv-repair-checklist.md)。根项目251项通过、16项忽略，根与服务严格Clippy通过。剩余40项R、3项L及S01；以下相关问题描述为修复前历史证据，不作为这些项仍未修改的声明。

## 第三批修复状态（2026-11-02）

R14/R15/R57/R58/R59/R10/R12/R11/R13/R16/R17/R19/R22已完成代码修复，具体存储契约、凭据意图、交互检查与未验收范围见[修复清单](cv-repair-checklist.md)。根项目263项通过、16项忽略；剩余27项R、3项L及S01。相关条目继续保留修复前证据，不再表示这些代码未修改。

## 第四批修复状态（2026-11-02）

R05/R32/R35/R18/R37/R36/R61/R60已完成代码修复；Provider缓存有界同步、来源证明、迟到选择恢复与连接快照回读的实现和限制见[修复清单](cv-repair-checklist.md)。根项目277项通过、16项忽略，严格Clippy通过。剩余19项R、3项L及S01。下文对应描述继续作为修复前证据保留；真实平台和完整端到端验收不因勾选完成而免除。

## 需要修正的代码差距

| 编号／优先级 | 触发与当前结果 | CV 或本项目约定 | 证据与修正边界 |
| --- | --- | --- | --- |
| R01／高 | TUN 偏好载入先 decode，`StackIndex` 得到 `System/gVisor/Mixed/Mips`，`tun::apply` 直接写字符串，可能将显示标签写入原生 YAML。 | CV 使用 `system/gvisor/mixed/mips`；本项目要求持久化稳定英文标识。不能据此推断 Mihomo 一定拒绝大小写。 | [载入](../src/configuration/preferences.rs#L100)、[写入](../src/configuration/tun.rs#L77)、[映射](../src/preferences/choices.rs#L7)；对照 [CV TUN](../clash-verge-rev-dev/src/components/setting/mods/tun-viewer.tsx)。需在原生配置边界编码，影响 C03/C36/B07。 |
| R02／高 | 服务管理已停止旧内核／清理代理，安装或修复失败后重新启动旧内核；该失败分支直接返回，没有重新应用系统代理。可能出现旧内核恢复、系统代理仍关闭。 | CV 将内核就绪与随后应用代理串联；本项目同样要求分别报告和补偿 OS 效果。 | [platform_action](../src/core/service.rs#L1459) 与 [CV 生命周期](../clash-verge-rev-dev/src-tauri/src/core/manager/lifecycle.rs#L379)。需补代理恢复及其独立失败结果，影响 C04/C38。 |
| R03／高 | 服务操作成功后，新内核恢复失败直接返回；转子进程启动成功但设置保存失败也直接返回，缺少对应补偿。不能笼统声称所有失败均恢复旧设置、旧内核与代理。 | CV 有显式 sidecar 转换清理与 rollback_failed_sidecar_transition；本项目要求记录提交点、残留状态和恢复结果。 | [恢复／保存分支](../src/core/service.rs#L1517)、[CV 转换失败清理](../clash-verge-rev-dev/src-tauri/src/core/manager/lifecycle.rs#L438)。需按安装／卸载／转子进程分别定义可恢复边界，不能卸载后盲目宣称服务已恢复，影响 C04/C38。 |
| R04／高 | 服务短暂失联、watchdog 恢复，或子进程 API 单次读取失败时，健康检查将阶段改为 Failed；之后健康检查只在 Ready 执行。即使同归属服务内核恢复，也没有继续观察成功的路径；服务 try_wait 恒为 None。 | 应用必须能观察服务的实际恢复结果；有限重试不等于永久放弃实际状态回读。 | [健康检查](../src/core/service.rs#L612)、[服务状态](../src/core/privileged.rs#L93)、[try_wait](../src/core/service.rs#L253)、[服务恢复状态](../platform-service/src/core/status.rs)。需有界恢复监督／显式重试入口，保持身份校验，不启动第二内核，影响 C01/C04/C38。 |
| R05／高 | 服务 Provider 文件同步只在 ChainCandidates 命令中调用；持久链启动合成先读取应用目录缓存。服务下载或刷新 Provider 后，不打开编辑器可能仍以过期／缺失缓存重建代理链。 | CV 在服务启动和配置应用后调度 runtime provider sync。 | [唯一调用](../src/core/service.rs#L548)、[本地定义读取](../src/proxies/chain_sources.rs#L12)、[CV 启动同步](../clash-verge-rev-dev/src-tauri/src/core/manager/state.rs#L374)、[CV 应用同步](../clash-verge-rev-dev/src-tauri/src/core/manager/config.rs#L417)。需补生命周期同步触发及完成关系，影响 C20/C25/C26。 |
| R06／中 | 测速并发和超时允许保存 1..u32::MAX，执行时却分别 clamp 到 1..10、100..60000ms。例如保存并发 36，实际仅 10。 | CV 的设置和测速执行应作为同一契约核对；本项目禁止静默修改用户设置的含义。有执行上限应在输入边界说明并拒绝越界。 | [设置校验](../src/preferences/schema.rs#L2120)、[执行限制](../src/core/delays.rs#L57)。需统一输入／执行范围；默认值也需明确依据，影响 B03/B07。 |
| R07／中 | 代理模型丢弃大于等于配置超时的正延迟，搜索再把缺失值转换为 3000 或错误哨兵。真实历史延迟 6000ms 在超时设置 5000ms 时无法按原数值过滤。 | CV getDelayFix 保留正延迟，过滤直接使用原数值；颜色／超时呈现不应破坏搜索数据。 | [截断](../src/proxies/model.rs#L227)、[搜索值](../src/proxies/model.rs#L44)、[CV 原值](../clash-verge-rev-dev/src/services/delay.ts#L248)、[CV 过滤](../clash-verge-rev-dev/src/components/proxy/use-filter-sort.ts#L29)。需分开原始结果与展示分类，影响 C21/B03。续查：搜索靠错误正文包含“超时”推断类别，而[node-delay渲染](../src/proxies/node_list.rs#L674)将任意failure统一显示“失败”，导致明确超时错误与普通错误混同；未测速使用灰色Gauge图标符合当前DESIGN，不列为差距。对照[CV统一分类](../clash-verge-rev-dev/src/utils/delay.ts#L17)，需使用明确测量状态，不逐层解析文案。 |
| R08／中 | Rule 模式首页／自动测速固定取第一个非 GLOBAL 组，不能选择与按订阅记忆首页组；自动请求 urls 为空，代理页编辑的逐组测试地址由页面实体持有，没有共享到自动测速。 | CV 首页可选组、按订阅记忆，自动检测使用选中组；DelayManager 统一保存逐组 URL。 | [当前组](../src/proxies/runtime.rs#L293)、[地址所有者](../src/proxies/mod.rs#L22)、[自动请求](../src/core/delays.rs#L276)、[CV 首页](../clash-verge-rev-dev/src/components/home/current-proxy-card.tsx#L680)、[CV URL](../clash-verge-rev-dev/src/services/delay.ts#L158)。需统一领域所有权，影响 C22/C23。 |

补充错误文案边界：C24 按旧节点 chains 清理的策略与 CV 一致，但 [cleanup_previous_proxy](../src/core/connections.rs#L5) 的设置读取、首次查询及回读 HTTP 错误直接传播，未在所有失败点保留“节点已切换”的事实。不是节点选择失败，不应让用户误以为选择已撤销。它作为 R02/R03 同类提交事实问题跟进，不另计独立功能差距。

## 入口与范围问题（S01）

[设置来源核查](settings-origin-audit.md) 已承认 Wi-Fi/SSID 暂停、进程优先级、独立工作目录、热重载订阅及相关自动断开选项缺执行消费者；[schema](../src/preferences/schema.rs#L1715) 仍提供这些入口。全文核对后，AutoCheckUpdate、TestProfileOnStart 也只见设置定义，未见执行读取；应用升级仍属未实现范围，配置校验则始终执行。它们来自 ClashSuki，不等于用户授权新增，也不等于功能完成。应逐项清理无效入口并追溯明确授权，不能为了消除占位而自行扩大实现范围。Gist／同步按用户要求保留，单独标未接入，不混入删除项。

Provider override（如节点重命名／属性覆盖）在本地链式实例化中未处理；是否构成超出 CV 的缺口尚需核对其链式物化语义。本次不将其当成已确认问题，也不据此新增方案。

补充功能差异：C15增强可视化目前未覆盖CV的Provider use、healthcheck、筛选／include-all等组字段和当前来源候选（未知字段仍通过原map保留、可用YAML编辑，不能说运行时丢失）；类型下拉显示英文标识与用户明确中文标签要求仍有差距。C38通知尚缺CV右键复制完整诊断的入口，见[CV复制原文](../clash-verge-rev-dev/src/components/layout/notice-manager.tsx#L248)与[Suki通知](../src/ui/notifications.rs#L35)，不代表日志没有原错误。

## 已实现范围覆盖表

此表覆盖原清单所有已实现／部分实现的 C 项和 B01–B07；未实现与暂缓另列。每项检查其实际执行入口，不以页面存在判定完成。

| 范围 | 对照后的结论 | Suki／CV 主要证据 |
| --- | --- | --- |
| C01 系统信息 | OS／架构／主机／权限／自启动与运行时长主体已有；服务实际状态恢复有 R04。 | 历史 `src/desktop/system.rs`（入口已移除）、[system-info-card](../clash-verge-rev-dev/src/components/home/system-info-card.tsx) |
| C02 系统代理 | 手动／PAC、绕过两开关、守卫／回读／归属恢复主体已有；macOS 空 PAC URL 删除键为针对已出现错误的改进。未认定三平台完全相同或已实机验收。 | [system_proxy](../src/system_proxy/mod.rs)、[CV sysopt](../clash-verge-rev-dev/src-tauri/src/core/sysopt.rs)、[说明](system-proxy.md) |
| C03/C04/C36 TUN／服务／内核设置 | 常驻服务、权限能力、Mihomo 路由、结构化重建路径已有；R01–R04 表明仍有代码差距。 | [launch](../src/core/launch.rs)、[privileged](../src/core/privileged.rs)、[CV manager](../clash-verge-rev-dev/src-tauri/src/core/manager) |
| C05 Windows | 原生 UWP 枚举／修改、未知豁免保留、防火墙限制及回读／恢复按 ClashSuki 方案实现；不冒充 CV 外部工具，尚缺 Windows 实机证据。 | [windows_network](../platform-service/src/windows_network.rs)、[ClashSuki 来源记录](tun-service.md)、[CV UWP](../clash-verge-rev-dev/src-tauri/src/core/win_uwp.rs) |
| C06 启动窗口 | 登录任务用户／管理员权限、启动脚本路径已有；正常关闭／静默／重启及注册协调仍有 L01/L02/L05，不仅缺实机验收。 | [desktop](../src/desktop/mod.rs)、[autostart_windows](../src/desktop/autostart_windows.rs)、[CV autostart](../clash-verge-rev-dev/src-tauri/src/core/autostart.rs) |
| C07 单实例导入 | 当前用户锁、认证本地端点、唤起与导入、有界队列、协议注册路径已有；macOS 原生 Reopen 缺 L03，正式安装包 URI 验收未完成。 | [instance](../src/desktop/instance.rs)、[scheme](../src/desktop/scheme.rs)、[CV server](../clash-verge-rev-dev/src-tauri/src/utils/server.rs) |
| C08 托盘 | 已改 OS 原生附着菜单，模式／组／节点／订阅／网络开关／维护／退出接命令；含 TUN 自定义状态图片。Surge 布局为用户决定；关闭与退出清理见生命周期表及 L02–L04；系统决定原生模糊材质，不保证与主窗口逐像素相同。 | [menu](../src/tray/menu.rs)、[platform](../src/tray/platform.rs)、[CV tray](../clash-verge-rev-dev/src-tauri/src/core/tray) |
| C10 交付 | packaging 已有服务相关材料；仍不是完整多平台签名／公证／应用升级与发布交付。 | [packaging](../packaging)、[CV tauri](../clash-verge-rev-dev/src-tauri/tauri.conf.json) |
| C14 脚本 | main(config,name)、顺序执行和执行日志主体对齐；QuickJS 中断／内存／栈／日志预算是有明确终止与资源边界的改进。 | [script](../src/configuration/script.rs)、[CV script](../clash-verge-rev-dev/src-tauri/src/enhance/script.rs) |
| C15/C16/C17 订阅覆写 | 定时／手动更新、元数据、凭据、候选校验／提交与失败保留主路径已有；逐订阅 self_proxy > with_proxy > direct 与 CV 一致，忽略证书只作用于所选请求。C17 按用户已确认范围。 | [profiles](../src/core/profiles.rs)、[overrides](../src/core/overrides.rs)、[CV PrfItem](../clash-verge-rev-dev/src-tauri/src/config/prfitem.rs) |
| C18 全局 DNS | 当前代码已按增强 → 全局 DNS → 有序覆写，不存在后置全局 DNS 再覆盖用户覆写的问题；这是明确用户约定，不套用 CV 所有权结构。 | [合成](../src/core/launch.rs#L380)、[apply_dns](../src/configuration/flow.rs#L177)、[约定](../AGENTS.md#L72) |
| C19 配置查看 | 运行中取已应用 YAML，停止时预览合成、不依赖已装内核，查看／复制已有；完整合成报告仍缺。 | [actions](../src/settings/actions.rs)、[CV runtime](../clash-verge-rev-dev/src-tauri/src/cmd/runtime.rs) |
| C20 来源 | 核心同名优先、Provider use/include-all/exclude-type、同名 occurrence、独立 healthcheck 主体对齐；服务缓存触发缺口 R05。 | [runtime](../src/proxies/runtime.rs)、[CV proxy_view](../clash-verge-rev-dev/src-tauri/src/core/proxy_view.rs) |
| C21/C22/C23 搜索与测速工具 | 搜索标志／正则／delay/type 与组过滤、组工具／拖排／吸顶／返回顶部、自动检测入口已有；R06–R08 尚未对齐。 | [model](../src/proxies/model.rs)、[delays](../src/core/delays.rs)、[CV filter](../clash-verge-rev-dev/src/components/proxy/use-filter-sort.ts) |
| C24 切节点清连接 | 只清理 chains 含原节点，符合 CV；并发有界和回读是补强；提交后错误事实文案仍需统一。 | [connections](../src/core/connections.rs)、[CV selection](../clash-verge-rev-dev/src/hooks/use-proxy-selection.ts) |
| C25/C26 代理链 | dialer 合成／校验／热应用／出口选择／回读／保存／断开主路径及来源绑定已有；布局按用户要求，服务缓存恢复仍有 R05。 | [chain](../src/core/chain.rs)、[chain_config](../src/proxies/chain_config.rs)、[CV chain](../clash-verge-rev-dev/src/components/proxy/proxy-chain.tsx) |
| C28 IP | 元数据、隐藏、独立刷新与 300 秒自动刷新已有；失败保留上次值，不因刷新重启。边界与外部请求验收不冒充通过。 | [monitor](../src/core/monitor.rs)、[CV IP](../clash-verge-rev-dev/src/components/home/ip-info-card.tsx) |
| C30 首页 | 曲线／持久流量／采样排行已有；卡片布局与失焦绘制策略缺失，首页选组缺口见 R08。 | [overview](../src/overview/view.rs)、[CV layout](../clash-verge-rev-dev/src/components/setting/mods/layout-viewer.tsx) |
| C31/C32 连接／规则 | 连接元数据与格式化详情、关闭／历史／暂停／搜索、规则顺序／筛选／复制／详情主体已有；单表格为用户决定，无虚构命中数。 | [traffic](../src/traffic)、[CV connections](../clash-verge-rev-dev/src/pages/connections.tsx)、[CV rules](../clash-verge-rev-dev/src/pages/rules.tsx) |
| C35 图标／WebUI | 远程／data／SVG 缓存和 WebUI 配置已有；本地导入／复制入口仍缺。 | [icons](../src/core/icons.rs)、[CV app](../clash-verge-rev-dev/src-tauri/src/cmd/app.rs) |
| C37 日志 | 私有 WS、心跳、真实级别、脱敏、管道去重、代次隔离及三次失败暂停已有；暂停后恢复依赖目标／服务生命周期，受 R04 影响。 | [log_stream](../src/core/log_stream.rs)、[CV log hook](../clash-verge-rev-dev/src/hooks/use-log-data.ts) |
| C38 故障 | 设置错误聚合、诊断 ZIP、原始与恢复错误、有限重试主体已有；不能继续宣称仅缺验收，见 R02–R04、L02/L04。诊断不默认导出用户正文／凭据是明确安全边界。 | [diagnostics](../src/diagnostics.rs)、[service](../src/core/service.rs)、[CV lifecycle](../clash-verge-rev-dev/src-tauri/src/core/manager/lifecycle.rs) |
| C39 凭据 | 三平台原生安全存储边界与异步调用已有；目标编译不等于真实凭据读写。 | [credentials](../src/profiles/credentials.rs)、[CV config](../clash-verge-rev-dev/src-tauri/src/config) |
| B01/B02 配置和来源 | 合成／校验／应用／持久化／补偿主路径已有；B01 的服务管理例外 R02–R04 不能用普通热更回滚代替。 | [configuration](../src/configuration/mod.rs)、[profiles](../src/core/profiles.rs)、[CV enhance](../clash-verge-rev-dev/src-tauri/src/enhance/mod.rs) |
| B03/B04 节点和资源 | 独立测速／来源 healthcheck、Geo／Provider／内核下载更新与取消主路径已有；测速范围／缓存差距见 R05–R08。应用升级独立未接。 | [delays](../src/core/delays.rs)、[resources](../src/core/resources.rs)、[download](../src/core/download.rs) |
| B05/B06 实时流和日志 | 私有 traffic/connections、身份绑定、有界缓冲、有限重连／暂停、历史与日志有界；toast 普通／撤销自动关闭，仅进度常驻，符合用户要求。 | [streams](../src/core/streams.rs)、[logging](../src/logging.rs)、[notifications](../src/ui/notifications.rs) |
| B07 设置公共操作 | 原生 YAML／JSON 分离、坏文件保留与自动保存主体已有；R01/R06/S01 表明字段显示／保存／执行仍需统一。 | [preferences](../src/preferences/mod.rs)、[settings](../src/settings/mod.rs)、[CV verge](../clash-verge-rev-dev/src-tauri/src/config/verge.rs) |

## 逐文件续查新增差距（R09–R61）

本节来自本轮实际逐段阅读。文件覆盖状态见 [逐文件台账](cv-file-review.md) 和 [逐文件 CSV](cv-file-review.csv)；**已登记936个文本文件全文阅读完成，跨模块行为闭合与平台验收仍未全部完成**，不能据此称全量一比一验收完成。

| 编号／优先级 | 正常触发与实际差距 | CV／用户约定及证据 |
| --- | --- | --- |
| R09／高 | 用户指定 `bind-address: 192.168.1.2` 并开启 allow-lan，合成后无条件变为 `*`，监听范围扩大；关闭时也被固定为 127.0.0.1。不是只修改默认绑定。 | [Suki overlay](../src/configuration/preferences.rs#L452) 对照 [CV ensure_lan_bind_address](../clash-verge-rev-dev/src-tauri/src/enhance/mod.rs#L531)。CV 仅在 allow-lan 开启且绑定回环时改为 *，明确保留自定义地址，测试在同文件1692行。应保留用户合法绑定范围，影响 C36/B01/B07。 |
| R10／高 | 两个订阅同名，覆写“作用范围”Select 只持有显示文字，保存时按名称 find 第一个。选择第二个同名订阅可能仍保存第一个 ID；与“所有订阅（全局）”同名也可能混淆。 | [Select 数据](../src/profiles/editor.rs#L60)、[保存反查](../src/profiles/editor.rs#L222)。CV 的 profile/chain 关联使用 uid（[PrfOption](../clash-verge-rev-dev/src-tauri/src/config/prfitem.rs#L118)），本项目也要求稳定 ID。需让选项值为身份、名称只展示，影响 C15/B02。 |
| R11／中 | 初次导入时用户明确填写1440分钟，响应 profile-update-interval:6 会改成360分钟，因为代码将值1440当作“未填写”。 | [Suki apply](../src/profiles/remote.rs#L59) 对照 [CV 显式值优先](../clash-verge-rev-dev/src-tauri/src/config/prfitem.rs#L347)。需区分未指定与明确填写；保留用户的0关闭，不补独立自动更新开关，影响 C16/C17。 |
| R12／中 | 远程订阅响应只要是 YAML 对象（如 `{}` 或错误映射）即可通过应用层解析进入正文替换流程；不要求 proxies/proxy-providers，可能被保存成没有节点的成功更新。 | [Suki 远程解析](../src/profiles/remote.rs#L34)、[保存校验](../src/profiles/repository.rs#L57) 对照 [CV 远程订阅结构检查](../clash-verge-rev-dev/src-tauri/src/config/prfitem.rs#L374)。应区分订阅与允许任意合并对象的覆写，不因支持纯 YAML 覆写而放松远程订阅。纯规则本地配置和真实基础运行配置另行保留，不混为一条验证规则，影响 C16/B02。 |
| R13／中 | “使用系统代理”与“使用内核代理”可以同时开启；执行端仍按 self_proxy 优先，界面表示两者启用而系统代理实际上不参与。 | [Suki Switch 回调](../src/profiles/editor.rs#L455) 对照 [CV 两个 useEffect 互斥](../clash-verge-rev-dev/src/components/profile/profile-viewer.tsx#L76)。布局末尾是用户要求，开关不互斥不是用户要求；需对齐选择语义，影响 C16。 |
| R14／高 | “打开文件”交给外部编辑器的是 profiles/id.yaml 或 overrides/id.yaml；但重启／激活读取索引里的 content，不重新读该文件。外部保存后不生效，下一次索引保存还会将文件覆盖回索引正文。 | [文件入口](../src/profiles/mod.rs#L519)、[索引正文](../src/profiles/store.rs#L125)、[Source 读取](../src/configuration/source.rs#L98)、[正文覆盖](../src/profiles/repository.rs#L209)、[覆写读取](../src/profiles/override_repository.rs#L9)。CV [current_mapping](../clash-verge-rev-dev/src-tauri/src/config/profiles.rs#L381) 与 [ChainItem](../clash-verge-rev-dev/src-tauri/src/enhance/chain.rs#L38) 读取真实源文件。需明确唯一正文所有者，沿用校验／热应用边界；不自行加兼容／双读回退，影响 C14/C15/B02。 |
| R15／高 | 凭据编辑提示“留空保留已保存凭据”，但用户只填写新 token、Age 留空时，执行端将整个 `(token, age)` 元组写入钥匙串，清掉原 Age；只换 Age 时同样清掉原 token。下一次加密订阅更新可能失败。 | [编辑提示](../src/profiles/editor.rs#L120)、[收集待写凭据](../src/core/profiles.rs#L122)、[整体写入](../src/core/profiles.rs#L177)、[仅双空才回读](../src/core/profiles.rs#L357)、[覆写同类路径](../src/core/overrides.rs#L174)。这是本项目安全存储与文案契约问题，CV 无同一 token/Age 表单不能冒称逐 API 相同。需分别表达保留／替换／清除意图，不靠空字符串猜测，影响 C39/B02。 |
| R16／中 | 删除订阅／覆写已提交，但随后凭据删除失败时，命令返回“已保存；…失败”；页面将整次操作视为 rejected，跳过最新列表同步，也不执行完成回调，可能仍显示已删除条目且没有本次撤销 toast。 | [已提交后失败](../src/core/profiles.rs#L216)、[覆写](../src/core/overrides.rs#L259)、[页面拒绝分支](../src/profiles/mod.rs#L88)。本项目要求实际状态和已提交事实分开；已有事实型错误文字不足以驱动正确 UI。需结构化区分提交成功与后续清理失败，仍提供撤销，不让旧草稿覆盖实际删除，影响 C15/C38/B02。 |
| R17／中 | 首次导入订阅时 selected 为 None，Save 路径保持 None，停留在基础配置；没有像 CV 那样将第一份订阅设为当前来源。导入成功不等于已启用。 | [Suki 选择计算](../src/core/profiles.rs#L140)、[提交](../src/core/profiles.rs#L571) 对照 [CV append_item](../clash-verge-rev-dev/src-tauri/src/config/profiles.rs#L221)。这是首次使用主流程的行为差距，不是错误处理；应按 CV 的首份来源选择语义评估，不改成每次导入都强制切换，影响 B02。 |
| R18／高 | 启动恢复选择只检查4轮、间隔250ms；某组暂未出现在 API 时还会直接从 pending 移除当作已处理。Provider 较慢才加载出原节点时，本次恢复已结束，没有后续有界恢复任务。 | [Suki restore_proxy_selections](../src/core/proxy_control.rs#L156) 对照 [CV KeepRecords／settle_pending_selections](../clash-verge-rev-dev/src-tauri/src/config/profiles.rs#L840)。CV 将首轮等待与最长30秒的后台收敛分开，防止冷启动长期落在默认节点。需保留代次与用户新选择优先，并有界停止，不变为无限重试，影响 B03/C20/C38。 |
| R19／中 | 订阅地址为 `https://example.com?token=private`（无显式路径斜杠）时，卡片“来源”通过字符串 split 取得 `example.com?token=private`，将查询凭据带入普通卡片展示。 | [Suki 来源](../src/profiles/view.rs#L94)、[渲染](../src/profiles/view.rs#L246) 对照 [CV parseUrl](../clash-verge-rev-dev/src/components/profile/profile-item.tsx#L989)，后者使用 URL.host。需用已验证 URL 的 host／port，不改变用户原 URL；正常卡片不展示 query，影响 B02。 |
| R20／中 | 代理页对无效正则与正则优先级已专门适配，但共用 Search 在日志、连接／规则表中仍将无效正则降为字面量，且同时开启整词时会给正则加边界，行为与 CV 不同。源码注释还误称“与CV字面量回退一致”。 | [Search](../src/search.rs#L12)、[日志使用](../src/logs/model.rs#L82)、[表格使用](../src/traffic/table.rs#L77) 对照 [CV compileStringMatcher](../clash-verge-rev-dev/src/utils/search-matcher.ts#L25)：无效正则无匹配，正则优先于整词。影响 B06/C31/C32；日志／表格相关源码现已全文阅读，界面与平台验收仍另行记录。 |
| R21／中 | 全局 DNS 的 listen 只允许 Rust SocketAddr；CV 正常默认值 `:53` 会在表单和原生 YAML 两处被应用拒绝，无法沿用同一种 Mihomo 地址写法。 | [Suki 验证](../src/configuration/flow.rs#L341)、[CV 默认文件](../clash-verge-rev-dev/src-tauri/src/utils/init.rs#L344)、[CV 表单默认](../clash-verge-rev-dev/src/components/setting/mods/dns-viewer.tsx#L146)。需在 DNS 所属边界按内核支持的监听语义验证；不将外部控制地址限制直接套用于 DNS，不作旧格式迁移，影响 C18/C36。 |
| R22／中 | 通过 clash://install-config?url=…&name=自定义名称 导入时，只提取 url，丢弃协议携带的 name，最终名称改由响应头／URL推断。与用户传入名称的含义不同。 | [Suki parse_uri](../src/desktop/instance.rs#L216)、[导入](../src/profiles/mod.rs#L336) 对照 [CV extract_subscription_info](../clash-verge-rev-dev/src-tauri/src/utils/resolve/scheme.rs#L48)。应保留明确名称、仅未给名称才自动获取；仍使用稳定类型／URL验证，不照搬 CV 的宽松或旧格式解析，影响 C07/B02。 |
| R23／中 | 托盘组图标只按 URL→缓存文件映射是否变化重建，未按组名→图标来源变化失效。组改名，或两个组交换已有图标且缓存集合不变时，可保留旧绑定；异步结果也没有配置身份检查。 | 用户要求托盘显示当前代理组图标；CV 此版本托盘并无完全相同的组图标布局，不将本项冒称 CV 原有能力。 [失效条件与绑定](../src/tray/mod.rs#L167)、[结果发布](../src/tray/mod.rs#L234)。需按当前组来源／配置身份重建并拒绝过期结果，影响 C08。 |
| R24／中 | 未配置自定义图片时，系统代理与 TUN 均使用同一个状态徽标，无法从默认托盘图标区分两种状态。 | CV 默认普通／系统代理／TUN 使用不同图标；不能把“可上传自定义 TUN 图片”等同于默认三态已齐。 [默认选择](../src/tray/platform.rs#L89)、[CV 托盘](../clash-verge-rev-dev/src-tauri/src/core/tray/mod.rs)。需核对默认资产／状态优先级，保留用户选定的原生托盘布局，影响 C08。 |
| R25／中 | 原生菜单构建失败不更新 menu_revision，平台图标写入失败不更新 current；后续运行时刷新会继续尝试并记录相同错误。工具提示每次 sync 都写入，失败也逐次记录，未设置失败暂停或有限次数。 | 用户明确要求类似故障停止或有限重试；CV 的行为也需逐分支核对，不能因其错误宏存在就视作本项目已控制重试。 [菜单／平台错误](../src/tray/mod.rs#L263)、[平台状态提交](../src/tray/platform.rs#L86)、[日志无去重](../src/integration.rs#L34)。需按操作／输入版本记录失败，显式动作或输入变化再试，不关闭整个托盘。 |
| R26／中 | 连接的开始时间只在 cells 保存 HH:mm:ss，numbers 对应列恒为 0；按开始时间列排序退化为字符串比较。跨午夜／多日历史中，旧 23:xx 可能排在新 00:xx 后。 | CV 从 start 解析完整时间戳，time 列以数值比较。格式化显示值不能作为领域排序键。 [记录生成](../src/traffic/connections.rs#L235)、[排序](../src/traffic/model.rs#L75)、[CV 时间排序](../clash-verge-rev-dev/src/components/connection/connection-table.tsx#L214)。需保存完整时间排序键，影响 C31。 |
| R27／中 | 连接关闭或内核停止时仅把 cells／numbers 的速度清零，预生成 details 未更新；详情流量读 cells，但“复制详情”读 details，可能复制关闭前的旧速度。 | 本项目要求同一实际状态的显示与复制一致；作为当前实现一致性问题，不声称 CV 已覆盖完全相同的复制逻辑。 [关闭更新](../src/traffic/connections.rs#L69)、[停止更新](../src/traffic/connections.rs#L143)、[复制来源](../src/traffic/details.rs#L143)。应由结构化记录派生展示／复制，影响 C31。 |
| R28／中 | 出口 IP 的自动刷新只保留 Overview 内存布尔值，重启重新开启；元数据未解析／展示 timezone。即使已有国家／ASN／坐标，仍不能称与 CV 完整对齐。 | CV 的自动刷新开关持久化，并展示时区；隐藏窗口／页面未进入视口时暂停自动查询。Suki 当前监控任务按 300 秒执行，不区分页面／窗口可见性，此项与 C30 绘制／后台策略分开评估，不能擅自暂停实际流量采集。 [页面初值](../src/overview/mod.rs#L70)、[界面与开关](../src/overview/view.rs#L570)、[监控与解析](../src/core/monitor.rs#L270)、[CV IP 卡片](../clash-verge-rev-dev/src/components/home/ip-info-card.tsx#L115)。影响 C28。 |
| R29／高（阻断） | 服务 Session::start 调 client::random_token，生成 24 字节／48 位十六进制 proposed_session_token；同仓服务的 hash_session_token 严格要求 64 位，StartClash 在准备运行配置前直接拒绝。当前真实服务模式启动因此无法走通。 | CV 独立生成 32 字节／64 位服务会话令牌；内核 API secret 与服务会话 token 是不同协议，不能直接复用尺寸不一致的生成器。 [调用](../src/core/privileged.rs#L65)、[24字节](../src/core/client.rs#L477)、[64位校验](../platform-service/src/core/auth.rs#L87)、[拒绝入口](../platform-service/src/core/server.rs#L717)、[CV 32字节](../clash-verge-rev-dev/src-tauri/src/core/service.rs#L102)。这是请求与接收端的确定契约冲突，非平台验收推测；影响 C04/C36/C38。 |
| R30／高 | macOS 系统代理始终走 GUI 的 Native/SCPreferences 写入；服务会话没有接 set_system_proxy，start 请求 macos_proxy 也固定 None。即使已安装提权服务，自动恢复／守卫仍可能在 GUI 权限下遇到 1003，显式操作才尝试 Authorization。 | CV 在 macOS 服务模式通过已认证会话调用 helper 的 set_system_proxy；仅 sidecar 路径本地写入。不能将本地原生字典修复等同于已对齐服务代理所有权。 [Suki 本地执行](../src/system_proxy/native.rs#L67)、[授权写入](../src/system_proxy/macos.rs#L260)、[会话请求](../src/core/privileged.rs#L66)、[CV 分路](../clash-verge-rev-dev/src-tauri/src/core/proxy_control.rs#L474)。进一步读完helper确认：其apply_proxy明确拒绝写入，clear_proxy／clear_proxy_waiting_for_network直接Ok；不只是GUI缺少调用。 [helper执行空缺](../platform-service/src/core/proxy.rs#L106)。需保留串行写／回读／有限失败处理，并把会话归属带到服务调用，不能失败后随意本地接管机器级代理；影响 C02/C04/C38。 |
| R31／中 | 服务模式日志仅连接实时 /logs，未调用 helper 已有 get_clash_logs 或 get_clash_log_snapshot；应用接入前／WebSocket 建立前的服务内核启动日志没有进入程序日志视图，重连也无历史补取。 | CV useLogData 首次连接调用 getClashLogs；后端另有服务日志快照接口。实时订阅接通不等于服务日志读取完整。 [Suki 实时源](../src/core/log_stream.rs#L79)、[现有未调用接口](../platform-service/src/client/mod.rs#L205)、[CV 首次补取](../clash-verge-rev-dev/src/hooks/use-log-data.ts#L128)。需按内核会话区分初始历史／实时去重与脱敏，不能补造断流期间完整性；影响 C37/C38。 |
| R32／中 | Provider 分块同步只在每个返回块之间比较 len/mtime，达到 len 后立即写本地缓存；单块文件可在内核原地写入途中被读到并发布。未比较 Provider updatedAt、等待文件稳定或在最后复核身份。 | CV 明确针对 Mihomo 原地重写缓存，检查稳定时间、前后 updatedAt、最后文件身份，并在生命周期锁下发布仍属当前配置的缓存。不是要求为理论并发加锁，而是参考实现已处理的实际写入方式。 [Suki 分块发布](../src/core/privileged.rs#L134)、[服务端读前元数据](../platform-service/src/core/runtime_generation/readback.rs#L84)、[CV 同步校验](../clash-verge-rev-dev/src-tauri/src/core/service.rs#L1119)。与 R05 的同步触发缺失不同，本项是同步内容一致性；影响 C20/C25/C26。 |
| R33／中 | macOS 无活动网络服务时，Native::read／capture 返回错误；关闭系统代理的保存仍先 Owner::apply→capture，托盘切换也先 observe，因而“关闭”不能完成。离线或切换网络时不能简单等同于启用失败。 | CV 对缺失网络服务区分操作方向：关闭可视为无须写入，开启仍报错；不是吞掉所有权限／配置错误。 [活动服务读取](../src/system_proxy/macos.rs#L146)、[先捕获](../src/system_proxy/mod.rs#L149)、[开关先读取](../src/core/system_proxy.rs#L49)、[CV 方向语义](../clash-verge-rev-dev/src-tauri/src/core/sysopt.rs#L185)。需保留已触碰服务的清理责任，不能因当前无活动服务跳过仍存在的旧服务代理；影响 C02。 |
| R34／高 | 首次正常启动即使服务已就绪，只要没有现存活动会话且未启用 TUN／Linux 特权网络，仍选择 Local。安装／修复服务后 start_prepared 使用旧 Launch 的 force_service，普通子进程可原样重启，未实际交接给服务。 | CV startup_decision 在 ServiceStatus::Ready 时直接选择 Service，TUN 关闭不意味着绕过已安装服务。用户也要求服务相关正常状态／切换对齐，不能仅以已安装／IPC就绪宣称服务模式已运行。 [启动活动会话判定](../src/core/service.rs#L1818)、[进程选择](../src/core/service.rs#L1854)、[管理后恢复](../src/core/service.rs#L1514)、[CV 策略](../clash-verge-rev-dev/src-tauri/src/core/manager/lifecycle.rs#L40)。需将可用服务、实际归属和用户明确选择 Sidecar 分开，交接失败沿用 R02/R03 的补偿边界，影响 C04/C06/C36/C38。 |
| R35／中 | 组作用域读取把 exclude-type 按逗号拆分；CV 按竖线拆分。配置 `exclude-type: ss\|vmess` 在当前解析中成为一个无法匹配协议名的字符串，可把同名成员绑定到本应排除的 Provider 记录，测速也会路由到错误来源。 | CV 明确按 `\|` 解析；include-all Provider 名排序已核实两边一致，不算差距。 [Suki 分隔符](../src/core/client.rs#L161)、[CV 分隔符](../clash-verge-rev-dev/src-tauri/src/cmd/proxy.rs#L96)、[作用域匹配](../src/proxies/runtime.rs#L189)。统一外部协议解析并覆盖同名／过滤后的来源绑定，影响 C20。 |
| R36／中 | 代理链断开提交配置与选择后，DELETE 全部连接，再要求 GET 回读完全为空；正常并发业务可在两请求间新建连接，结果错误地报告“仍有活动连接”。此时链已断开，但 ChainEditor 只在 result.is_ok 时更新 expected，可能仍显示旧链，下一次保存又因预期版本不符被拒绝。 | CV 断开会关闭连接但不要求所有新连接消失；本项目 C24 已按原连接身份确认，链断开也应区分目标连接与后来新连接。已提交事实应进入编辑器，不能让后续清理错误掩盖它。 [提交／空列表回读](../src/core/chain.rs#L130)、[成功才同步编辑器](../src/proxies/chain.rs#L346)、[CV 断开](../clash-verge-rev-dev/src/components/proxy/proxy-chain.tsx#L448)。影响 C25/C26。连接页“关闭全部”也要求新连接不存在，见 [connection_action](../src/core/connections.rs#L90)，同类跟进。 |
| R37／中 | 目标代理组为合法的仅 use Provider／include-all-providers 组、没有 proxies 字段时，链合成要求现存成员列表并报“目标代理组没有成员列表”。候选读取已提供 Provider 节点，但这种常用目标组无法接入 materialized 链出口。 | 属于本项目已接入 Provider 链候选后的执行缺口，不把 CV 旧链实现包装成已有完整 Provider materialization。配置应保留 use 等来源字段，再明确处理额外出口的组成员声明。 [允许选择 select/url-test](../src/proxies/chain.rs#L143)、[强制现存 proxies](../src/proxies/chain_config.rs#L220)、[Provider 候选](../src/proxies/chain_sources.rs#L59)。影响 C20/C25/C26。 |


| R38／高 | 服务模式下单独更新 Geo 数据库，程序替换的是应用内核目录文件，随后直接 client.reload(launch.yaml)，未通过 service_bundle／stage-runtime 把新数据库发布到服务私有运行目录。该路径既绕过资产交付，也重新发送应用侧私有控制配置，不能据 HTTP 成功就宣称服务内核已加载新数据库。 | 同一个运行内核更新应使用其实际文件／控制通道所有者；本项目已有 prepare_reload／reload_prepared 的服务分路，单文件资源更新未复用。CV 的服务运行包同样区分应用源与服务运行资产。 [直接写本地并reload](../src/core/service.rs#L935)、[已有服务暂存分路](../src/core/service.rs#L1614)、[运行包资产](../src/core/runtime_bundle.rs#L193)。需通过同一配置应用／资产发布工作流，失败分别恢复应用文件与服务资产；影响 C36/C38。 |
| R39／高 | 升级新内核失败后恢复旧文件并 start_prepared(previous)，成功分支 publish Ready 后直接返回原错误，没有 sync_system_proxy。此前 stop_owned 已清理代理，因此可出现旧内核已就绪、代理仍关闭。 | 普通 replace_committing 的旧内核恢复分支已重新应用代理，升级分支却没有，属于已确认同类执行路径不一致；CV 同样在就绪后的转换工作流协调代理，不能只看进程恢复。 [升级回退](../src/core/service.rs#L1302)、[普通替换恢复对照](../src/core/service.rs#L1787)、[停止先清代理](../src/core/service.rs#L1991)。与 R02 的服务管理失败分支是不同触发入口；影响 C36/C02/C38。 |

| R40／高 | Windows 系统代理只查询／写入默认 WinINET 连接，未枚举 RAS 连接；使用单独代理配置的拨号／RAS VPN 时，默认连接回读成功不代表该连接已切换，关闭与恢复也未覆盖这些连接。 | CV 当前锁定 sysproxy `89ad876` 的 set/unset/PAC 路径枚举 RasEnumEntriesW 并逐个设置 pszConnection；其 getter 也不代表完整逐连接回读，不能照搬后宣称精确恢复。 [Suki 默认连接查询](../src/system_proxy/windows.rs#L24)、[写入](../src/system_proxy/windows.rs#L152)、[CV锁定提交](https://github.com/clash-verge-rev/sysproxy-rs/blob/89ad876a0942c049ae72e1472f9fc3ed62023c5e/src/windows.rs#L32)。需要明确每连接的原值、归属和确认范围，影响 C02/C38；未做Windows实机操作。 |
| R41／中 | KDE 读取时用 URL 解析器接受现存代理URI，检查点只保存 host/port 等投影；恢复时固定重建 http:// 或 socks://，读取时接受的原scheme、userinfo等内容不能原样恢复。例如 httpsProxy 原值 https://proxy.example:8443 恢复后变成 http://proxy.example:8443。 | 回退比较也只看投影，可能报告恢复成功而原值已改变。属于本项目“恢复操作前系统代理”的契约缺口，不声称 CV 有精确检查点，也不推断每种URI都是KDE正式支持格式。 [读取投影](../src/system_proxy/linux.rs#L188)、[检查点](../src/system_proxy/linux.rs#L332)、[重建原值](../src/system_proxy/linux.rs#L386)。需区分原始恢复数据与展示快照，影响 C02/C38；未操作真实KDE。 |

| R42／中 | 服务状态接口读取 owner 的 desired-state.json 时用 unwrap_or_default 吞掉权限／JSON解析错误，随后成功返回 desired_core_should_be_running=false、generation=0。已有恢复意图因此被投影成“无需运行”，原始文件错误也不再反馈；如内核PID已消失，还可能压过应报告的Fatal／RecoveringCore状态。 | 上游 service-ipc v2.7.6 status.rs 同样存在此逻辑，属于继承的错误处理缺陷，不能以参考CV为由保留。本项目明确要求损坏文件报错、实际／期望／未知区分。 [吞掉读取错误](../platform-service/src/core/status.rs#L11)、[状态派生](../platform-service/src/core/status.rs#L52)、[底层本已区分缺失与损坏](../platform-service/src/core/desired.rs#L206)、[接口本可传播错误](../platform-service/src/core/server.rs#L702)。保持其他状态可读并明确期望状态不可用，不伪造false；影响 C01/C04/C38。仅源码确认，未注入真实服务文件。 |

| R43／中 | 服务启动准备仍扫描 runtime.backup、runtime.generation-*、runtime.staging-*，启动提交后按名称自动递归删除；当前运行目录已固定runtime，这些是上游保留的旧布局清理。 | 与本项目明确“不保留旧格式／布局迁移、不自动删除旧文件”的约定冲突，不是应照搬CV的功能。 [旧命名扫描](../platform-service/src/core/runtime_generation/assets.rs#L69)、[自动清理](../platform-service/src/core/runtime_generation/assets.rs#L45)、[接入启动](../platform-service/src/core/runtime_generation/assets.rs#L251)。移除旧布局识别／清理及相关测试，当前manifest所属文件清理另按真实所有权保留；影响 C04/C38。 |
| R44／高 | 暂存先删除缓存／替换资产，后续复制、源变化检查或PID检查失败时，在写新manifest之前就返回；已变更文件未回滚、旧manifest仍有效。随后恢复旧bundle时，plan_stage只比较源路径／长度／mtime，可能跳过已被替换或删除的目标。旧YAML保留并不保证旧资产可用。 | 具体顺序：旧manifest记X来自A；候选把X改为B，随后Y写入失败；恢复A时仍匹配旧manifest而跳过X，留下B。materialize_plan中途失败也有同类路径。上游service-ipc同样存在，属于需改进的继承缺陷，不声称CV已经有完整资产事务。 [只比源身份](../platform-service/src/core/runtime_generation/staging.rs#L88)、[提前写入及返回](../platform-service/src/core/runtime_generation/staging.rs#L197)、[仅配置提交失败才废弃manifest](../platform-service/src/core/runtime_generation/staging.rs#L250)、[启动物化同类路径](../platform-service/src/core/runtime_generation/assets.rs#L283)。需保留资产恢复材料，或明确作废已不可信的来源证明并确认重新发布；影响 C04/C36/C38。源码确认，未运行故障注入测试。 |
| R45／中 | macOS daemon把run_ipc_supervisor_until_shutdown的任意错误都标记“恢复预算耗尽”并return Ok，包含首次监听失败与停止内核清理失败；launchd模板又只对失败退出重启，因此这些不同错误都被作为成功退出停止。 | 限制自动重试是用户要求，应保留；问题是没有区分预算耗尽、启动失败和清理残留。CV上游直接传播错误且KeepAlive=true，本项目不应退回无限重启，应使用明确终止原因、保留原始失败和清理结果，再决定受限恢复／退出码。 [所有错误返回成功](../platform-service/src/bin/service.rs#L192)、[监听／停止错误来源](../platform-service/src/core/server.rs#L387)、[launchd策略](../platform-service/resources/launchd.plist.tmpl#L30)。影响 C04/C38；未启动真实daemon。 |

| R46／中 | Windows安装核心的allow_core_through_firewall捕获失败后只eprintln并继续；安装器协议就绪即可成功退出，外层installation_result在退出成功时直接Ok，未携带防火墙警告。规则创建失败时GUI仍只收到安装成功，TUN／LAN可能缺少所需规则。 | 上游安装器也采用best-effort，不代表本项目可以丢掉已发生的失败。安装事实应保留，同时返回网络准备的独立警告和修复入口，不能把防火墙失败伪装成服务安装失败，也不能完全消失。 [吞掉防火墙错误](../platform-service/src/maintenance/install.rs)、[仅看退出码](../platform-service/src/management.rs#L125)、[服务管理调用](../src/core/privileged.rs#L205)。影响 C04/C05/C38，未操作真实Windows防火墙。 |
| R47／高 | macOS卸载忽略stop/disable/bootout失败，Linux忽略stop/disable/daemon-reload失败，再删除服务文件；随后用于确认服务已停的repair_active_owner_state失败也仅警告。Unix允许删除仍在执行的程序，故退出0不保证已卸载或停止。应用虽probe状态但未要求卸载后确认为NotInstalled。 | 上游卸载器同样有这些忽略；应区分“本就不存在”和停止／注销失败，先确认所拥有服务停止，再删所属文件，保留部分完成与残留。 [macOS忽略停止](../platform-service/src/maintenance/uninstall.rs)、[Linux忽略停止](../platform-service/src/maintenance/uninstall.rs)、[停止确认失败被吞](../platform-service/src/maintenance/uninstall.rs)、[调用后未确认卸载](../src/core/service.rs#L1490)。Windows现有停止／删除轮询是正确对照，不复制其API到Unix。影响 C04/C38；未运行卸载器。 |
| R48／中 | UWP保存期间只禁用表单，取消按钮仍可关闭弹窗；成功回调不检查编辑器是否仍存活／是否仍是当前弹窗，无条件window.close_dialog。用户取消后打开别的弹窗，旧保存成功可能关闭新弹窗。 | 当前GPUI Kit 0.8.1的close_dialog关闭“最新且未关闭”的弹窗，不绑定原编辑器。应让结果只结束自己拥有的界面；取消是否中止OS请求须明确，已提交事实继续如实反馈。 [始终可取消](../src/settings/uwp.rs#L42)、[成功回调](../src/settings/uwp.rs#L122)。这是本项目异步界面所有权问题，非声称CV有GPUI同名API；影响 C05。保存重复提交已有busy检查，不重复报缺少去重；未做真实窗口复现。 |

| R49／中 | 程序日志只要message包含://，就把整段正文替换成“日志包含地址，正文已省略”；例如下载／Provider失败同时带URL、状态码与恢复失败，全部诊断上下文都会消失，实时程序日志和落盘均受影响。 | 本项目要求脱敏同时保留原因、对象与恢复结果，不能整段文案清洗掩盖问题；CV logging只负责格式与记录，本项目仍应保留自己的凭据脱敏边界，不照搬原文泄漏。 [整段替换](../src/integration/service.rs#L169)、[CV日志记录](../clash-verge-rev-dev/crates/clash-verge-logging/src/lib.rs#L76)。应在拥有URL／凭据的边界结构化脱敏，保留错误阶段和原因；影响 C32/C38。 |
| R50／中 | 程序日志文件创建／保留清理失败使Service::start通过?整体失败；main转成Service::unavailable，存储执行端不存在，此后连打开目录、导出诊断等无关请求也被统一拒绝。日志目录或旧日志权限故障会扩大成整个存储服务不可用。 | 本项目要求隔离受影响能力并保留目录、日志诊断和其他操作；内核输出已有文件不可写时继续内存采集的机制，可作为本项目内的正确边界参照。 [日志初始化传播失败](../src/integration/service.rs#L140)、[无执行端一律拒绝](../src/integration/service.rs#L260)、[main降级](../src/main.rs#L53)、[内核日志独立处理](../src/core/output.rs#L27)。应保留存储执行端与内存错误反馈，恢复文件权限后允许重新建立日志，不自动改用户设置；影响 C38。当前内核有独立执行端，不据此声称内核也必然停用。 |
| R51／中 | Sink::write按剩余字节数切片并分卷，边界可能落在中文／其他UTF-8字符中间，也会把单条记录的时间、级别和正文拆散；单个.log可能不是有效UTF-8，下一卷可能以半个字符和无记录头的内容开始。 | 日志限额不能破坏文本编码与记录结构；CV使用FileLogWriter的记录写入／大小轮转，而非应用层字节截断。 [字节切片分卷](../src/logging.rs#L121)、[完整记录调用](../src/integration/service.rs#L176)、[CV记录写入](../clash-verge-rev-dev/crates/clash-verge-logging/src/lib.rs#L324)。按记录边界轮转并定义超长记录策略，保留有界限制；影响 C32/C37/C38。未运行测试，依据确定的字节切割路径。 |
| R52／中 | 测速计划只剔除reserved中已在其他批次运行的key，没有在本批次内去重。合法组含同一节点两次、或多个成员解析到同一Provider记录时，measurement_key相同，但两项都会提交，发送重复healthcheck并以同一key相互覆盖结果。 | 本项目明确要求同组同节点去重；不同来源的同名节点必须保留，不能仅按显示名删重。 [来源key](../src/proxies/runtime.rs#L248)、[计划逐成员加入](../src/core/delays.rs#L105)、[只对已有预留过滤](../src/core/delays.rs#L313)。例如组[n,n]都解析到同一core记录，本批开始前reserved为空，两项均通过；这是当前来源映射下的确定执行路径，不要求增加全局锁。影响 C20/C23。 |

| R53／中 | 复制环境变量直接从持久化core投影取MixedPort并同时拼HTTP/SOCKS地址；MixedPort=0、HTTP独立端口开启等合法配置仍复制http://host:0和socks5://host:0。没有核对各协议实际可用的监听。 | CV复制入口使用MixedPort::effective查询运行端口；本项目不采用CV自动换端口策略，但复制内容仍须对应真实监听／已配置可用端口，无法生成某协议时应说明。 [复制实现](../src/settings/actions.rs#L121)、[0为合法关闭端口](../src/configuration/preferences.rs#L110)、[CV实际端口](../clash-verge-rev-dev/src-tauri/src/feat/proxy.rs#L107)。影响 C08/C36；不要求未经授权自动启动内核或改端口。 |
| R54／低 | 内核设置搜索“版本”等状态区域文字时，开头已把运行状态／版本／通道卡片加入body，但未设置found=true；其他普通字段无匹配后，函数用“未找到设置项”覆盖整个body，丢掉已经命中的卡片。 | 正常搜索结果应保留各区域命中，不只普通schema字段。 [状态匹配入口](../src/settings/core.rs#L64)、[只在普通区域累计found](../src/settings/core.rs#L163)、[最终覆盖](../src/settings/core.rs#L342)。本项目现有搜索的一致性问题，不照抄CV不同布局；影响 C36。 |
| R55／高（Linux） | 自动路由关闭时，toggle把AutoRedirect草稿同步改false，但只commit_core_field(AutoRoute)；生命周期patch以已提交值合并，AutoRedirect=true仍写回原生配置。可能出现界面显示关闭、实际保存仍开启，或内核拒绝组合；再次开启自动路由还可能恢复界面未表示的重定向。 | CV保存tun时同时提交auto-route与auto-redirect，关闭路由的联动进入同一保存。Suki自动保存也必须把这两个实际变更作为同一补丁，不要求恢复CV的保存按钮或页面重置。 [只改草稿](../src/settings/mod.rs#L548)、[仅提交单字段](../src/settings/mod.rs#L557)、[执行端合并](../src/core/service.rs#L1448)、[CV联动](../clash-verge-rev-dev/src/components/setting/mods/tun-viewer.tsx#L265)、[CV一起保存](../clash-verge-rev-dev/src/components/setting/mods/tun-viewer.tsx#L114)。影响 C03/C36/C38；未修改真实TUN。 |

| R56／中 | 选中分组A过滤后切换到不含A的新配置，`group_filter`仍保留A；新分组全部被过滤，菜单又只列新组，无法取消已消失的A。空态“清除筛选”只清关键词和可用节点开关，仍不清分组集合，界面继续空白。 | 本项目筛选会话应随有效组身份协调，“清除筛选”必须覆盖其声明的筛选条件；不要求新增持久化设置或侧边导航。CV组头状态按当前订阅隔离，可参考其来源边界，但不照搬localStorage结构。 [保留过期集合](../src/proxies/mod.rs#L119)、[过滤入口](../src/proxies/node_list.rs#L55)、[清除遗漏](../src/proxies/node_list.rs#L111)、[菜单仅当前组](../src/proxies/view.rs#L23)、[CV按订阅状态](../clash-verge-rev-dev/src/components/proxy/use-head-state.ts#L60)。影响 C21/C22；纯源码确认，未运行UI。 |

| R57／高 | 增强索引JSON仍有效、但rules/proxies/proxy-groups中的某段YAML无效时，编辑器用`.ok().unwrap_or_else(empty mapping)`吞掉解析错误，原文不进入YAML编辑器。用户修改其他部分并保存时发送三个空／已编辑对象；执行端只校验新候选并用原索引做版本比较，因expected相同可成功覆盖错误原文。 | 本项目明确要求坏文件保留、不可用默认表单覆盖错误配置。CV对应增强编辑器先保留prevData/currData；解析无效时切YAML模式并阻止列表序列化，提供原文修正入口。 [静默空对象](../src/profiles/enhance.rs#L84)、[从草稿提交全部属性](../src/profiles/enhance.rs#L197)、[执行端仅检查当前索引版本](../src/core/profiles.rs#L280)、[CV保留原文](../clash-verge-rev-dev/src/components/profile/proxies-editor-viewer.tsx#L238)、[CV序列化门槛](../clash-verge-rev-dev/src/components/profile/proxies-editor-viewer.tsx#L276)。影响 C15/C38；应在所属读取边界报告并保留原文，不能自动修复或迁移。 |

| R58／高 | 增强YAML中的合法复合规则进入逐条表单编辑时直接按所有逗号拆分，只取type/payload/policy三段。`AND,((DOMAIN,example.com),(NETWORK,TCP)),DIRECT`即使不改字段直接确认，也会重组成`AND,((DOMAIN,example.com)`，丢失其余条件与策略。活动内核可能拒绝，但父草稿已被破坏；停止时通用结构校验不验证规则语义。 | 本项目要求原文与合法未知内容无损保留。CV支持完整AND/OR/NOT条件，以整段payload构造，并未采用前三段反解；不能虚称CV已有同款解析器。 [拆分](../src/profiles/enhance.rs#L784)、[取前三段](../src/profiles/enhance.rs#L807)、[重组](../src/profiles/enhance.rs#L916)、[CV复合条件](../clash-verge-rev-dev/src/components/profile/rules-editor-viewer.tsx#L207)、[CV整段构造](../clash-verge-rev-dev/src/components/profile/rules-editor-viewer.tsx#L560)。影响 C15/C32；应按顶层分隔语义解析，或保留不能无损表达的原文进入YAML修正。 |
| R59／高 | 增强代理表单统一对非type字段trim，含password；读取已有密码`" secret "`后仅修改名称或直接确认，保存也变成`"secret"`。格式校验无法判断远端密码是否正确，可能配置应用成功但认证失败。 | 用户自定义文字／秘密值保留原文，host/port等规范化与秘密字段分别处理；CV节点增强走URI或YAML，不会编辑其他字段时重写既有password。 [统一trim](../src/profiles/enhance.rs#L906)、[所有字段写回](../src/profiles/enhance.rs#L928)、[CV节点输入](../clash-verge-rev-dev/src/components/profile/proxies-editor-viewer.tsx#L181)。影响 C15/C38；这是配置内节点密码完整性，非R15安全存储部分更新问题；未联网验证。 |

| R60／中 | AutoCloseConnection唯一实际读取在旧节点连接清理，只有Select/Unfix调用；切Rule/Global/Direct模式成功不清连接，未兑现设置页“切换节点或代理模式后关闭”的说明。关闭系统代理成功也未接CV相同开关的连接清理。 | CV系统代理hook在关闭成功后且auto_close_connection开启时清连接；续查已闭合CV模式路径：[change_clash_mode](../clash-verge-rev-dev/src-tauri/src/feat/clash.rs#L115)检查auto_close_connection并调用after_change_clash_mode关闭连接；模式缺口也有CV后端证据。应区分模式／代理已提交和后续清理失败，按目标连接ID确认，避免R36并发新连接误报。 [唯一设置消费者](../src/core/connections.rs#L18)、[模式结束](../src/core/proxy_control.rs#L300)、[系统代理结束](../src/core/system_proxy.rs#L140)、[设置承诺](../src/preferences/schema.rs#L1751)、[CV关闭后清理](../clash-verge-rev-dev/src/hooks/use-system-proxy-state.ts#L73)。影响 C02/C24/C36；未运行真实连接操作。 |

| R61／中 | Selector/URLTest/Fallback成员因Provider不可用或缺记录而未解析时，界面显示未知但卡片仍可提交选择。执行端刷新后仅检查all含同名节点，未检查成员已解析且可交互。测速已禁止未知节点，不混报为测速问题；不声称内核一定误选其他来源。 | CV卡片禁用unresolved且不给选择回调；本项目同样应从当前来源快照确定可交互能力，不按名称猜测。 [卡片禁用条件](../src/proxies/node_list.rs#L584)、[命令校验](../src/core/proxy_control.rs#L217)、[CV保护](../clash-verge-rev-dev/src/components/proxy/proxy-item.tsx#L69)。影响 C20；缺Provider记录的正常失败状态即能触发，未实机选择。 |

参考修正：CV 当前 Cargo.lock 的 sysproxy 是 `89ad876a0942c049ae72e1472f9fc3ed62023c5e`，此前文档把本项目原适配来源 `c37a60e` 当成 CV 当前锁定版本，应区分二者。两提交仅 Windows 文件变更，既有 macOS/Linux 对照不因此失效。service-ipc 固定为 v2.7.6 `b2a7427d6681c8d3780ab83a73f4dc21da2f22d6`。

## 本轮核对后保留的行为差异

- CV 脚本异常会返回输入配置并继续；Suki 使本次应用失败并保留最近有效配置，符合用户明确失败契约。QuickJS 真正中断、内存／栈／输出预算为具体改进，不照抄 CV 的仅等待超时。
- CV 配置包含启动时自动寻找下一个混合端口；Suki 保持用户端口并报告冲突，属于已确认 X02，不作为漏实现。
- CV 的订阅表单失败后会尝试另一条下载代理；Suki 固定用户选择，不隐式换传输路径，符合当前约定。CV 的 TLS 平台证书失败后有限回退静态信任根、URL 内 Basic Auth 等行为与 Suki 不同，先记对照事实，不能未经评估归为缺陷或擅自加入。
- CV 会清理代理组中失效的节点／Provider 引用；Suki 保留来源交给校验，不静默删用户内容，按当前“不静默修复配置”的约定保留。DNS 按全局／有序覆写顺序，CV 的按订阅确认／自动禁用也不重新加入。

待进一步证实（不计入 R 编号）：DocumentEditor::revision 未包含 document.enabled，而保存完成使用 revision 与焦点判定是否关闭。已确认比较字段不完整，但尚未验证真实焦点路径能否导致丢弃用户新草稿；不能将猜测写成已复现错误。

修复前服务 fork 有一处明确的历史协议兼容残留：`RuntimeBundle.remote_providers` 标注“clients older than revision 2”并使用 serde(default)，但当前已严格拒绝非 revision 6。见 [structure.rs](../platform-service/src/core/structure.rs#L128)。按当前约定应移除旧客户端兜底并要求当前请求字段完整；不把普通应用设置缺省补全与历史 IPC 兼容混为一谈。此项归于架构清理，不增设产品选项；第一批同步协议revision 7时已删除该缺省兼容，并增加当前契约检查。

## 桌面生命周期：先区分三个对象

1. **应用进程**：主窗口、托盘、应用内存储／调度／监控线程、单实例端点。AutoRun 控制的是用户登录后启动这个程序。
2. **Mihomo 内核**：由应用子进程或提权服务持有，处理代理、TUN、路由。隐藏窗口不应停止它；正常退出应用应停止本应用拥有的内核并清理网络效果。
3. **已安装的提权服务进程**：系统级常驻执行端，正常退出应用后可以继续空闲，服务注册也继续保留。它不等于 Mihomo，更不等于应用已自启动。

**“开机自启关闭，但服务仍随系统启动”在当前实现与 CV 中都可能发生，单凭服务进程存在不能判定开关失效。** Windows 正式构建为 AutoStart（development-channel 为 OnDemand），macOS 为 RunAtLoad，Linux 安装后 enable systemd unit。这些注册不受 AutoRun 控制。对照依据为 CV 所用 service-ipc 的本地 v2.7.6 参考 `/tmp/suki-cv-service-ipc`，以及本项目 [安装器](../platform-service/src/maintenance/install.rs)、[launchd](../platform-service/resources/launchd.plist.tmpl)、[systemd](../platform-service/resources/systemd_service_unit.tmpl)。这是参考实现行为，不应在审计时擅自把应用自启动开关改成服务启停开关。

但必须进一步看 **内核是否启动**：正常退出会写入 stopped 意图并清除 active owner；应用崩溃／强制结束／系统未完成清理时，旧 running 意图可能保留。服务下次启动可依据它恢复内核，即使 AutoRun 关闭、应用未打开。该恢复来自 [desired state](../platform-service/src/core/desired.rs#L151)，CV 的对应服务同样存在，不属于自动启动第二个 GUI。产品清单须如实列出这项语义，不能把“AutoRun=false”描述成“系统不会再启动任何 Suki 相关进程”。

## 正常操作与生命周期场景表

| 用户操作／场景 | 当前 Suki 实际代码行为 | 对照结论／剩余 |
| --- | --- | --- |
| 关闭“开机自启” | 修改应用登录任务／LaunchAgent／desktop 注册，成功后保存；失败恢复注册。不会卸载／停止提权服务。 | 主体符合 CV 对应用自启动的划分；期望值与实际注册漂移见 L01。 |
| 打开“开机自启” | Windows 按当前权限注册登录任务；非 Windows 使用 auto_launch；参数为 --autostart。 | 无额外的开机运行内核开关；管理员／普通注册路径已有，未实机验收。 |
| 用户手动打开应用，AutoRun=false | 正常启动应用和内核；AutoRun 不表示禁止手动启动，不是启动内核的前置条件。 | 符合开关语义；[startup](../src/core/service.rs#L1828) 不受 AutoRun 限制。 |
| 已安装服务但应用未启动 | 服务可独立运行；无待恢复 running 意图时只提供 IPC，不凭服务进程存在认定 TUN 已开启。 | 与 CV 服务划分一致。 |
| 上次正常退出后再次开机 | 服务可启动；正常停止成功留下 stopped 意图，不应因服务启动自动恢复旧内核。应用是否登录启动另看实际 OS 注册。 | [StopClash](../platform-service/src/core/server.rs#L816) 停内核后保存 stopped 并清 active owner。 |
| 上次崩溃／被强杀后再次开机或服务重启 | 若 running 意图仍在，服务可恢复旧内核／TUN；不读取 GUI AutoRun 来阻止恢复。 | CV 服务有对应恢复逻辑；它与“每次打开 GUI 重新合成来源”是两条不同路径，须分开记录。 |
| 静默启动 | 先创建普通主窗口，再 defer 隐藏；仅托盘创建成功才隐藏。手动打开与登录打开都遵守 SilentStart。 | 托盘不可用时保留窗口是合理可访问性策略；初始可见性顺序与 CV 不同，见 L05。 |
| 点窗口关闭，CloseToTray=true、托盘可用 | 拦截关闭，调用 hide，不销毁主窗口；托盘、内核、订阅调度、采集继续。 | 用户要求的“只关界面，后台继续”主路径已接。CV 也拦截并隐藏。 |
| 点窗口关闭，CloseToTray=false | 不只是关窗口，而是调用全程序 quit，等待清理。 | 与 CV 固定隐藏不同；该设置来自 ClashSuki。当前用户举例要求的隐藏行为需在总表明确其条件，不能误写成总是隐藏；本次不自行删设置。 |
| 托盘创建失败后关闭窗口 | close_to_tray 返回 false，执行全程序退出，不会故意留下无法操作的隐藏 GUI。 | 合理的可访问性处理，不能伪称托盘仍可用。 |
| 托盘“显示窗口”／第二实例激活 | 激活原有主窗口，复用原进程，不重建内核。第二实例发送激活后结束。 | 主路径已有；macOS 原生 Reopen 另见 L03。 |
| 在托盘改模式／节点／订阅／系统代理／TUN | 调领域命令，读取实际状态；并非仅改菜单勾选。 | 沿用前表对应 C 项，不因主窗口隐藏而停后台执行。 |
| 托盘“退出”或应用 Quit 快捷键 | 同一个 core::quit：关闭执行端并等待校验／运行内核、代理、监控／日志、存储与实例端点清理，成功后 GPUI quit，托盘随进程释放。 | 退出的是应用与本应用内核，**不卸载、不停止系统级空闲 helper**；CV 正常退出也不是卸载服务。失败后续见 L02。 |
| 应用内“停止内核” | 停止本应用内核并清理系统代理，应用窗口、托盘与已安装 helper 继续。 | 与退出整个应用是不同动作，不能混称“退出服务”。 |
| “停止服务内核” | Action::Stop 映射 Actor::stop，停止内核会话；不停止 OS 服务守护进程。 | 应按真实动作命名；不要把 helper 的空闲进程当成未停止内核。 |
| 卸载服务／转子进程 | 独立显式管理操作；关闭 TUN 的确认／撤销与交接已有。卸载服务才移除对应 OS 注册。 | 不是托盘 Quit 的隐含动作；恢复缺口见 R02–R04。 |
| 重启应用 | 先清理，再停止实例端点并释放锁，启动当前程序，最后退出旧进程。新进程重新读设置／来源。 | 正常路径已有；创建新进程失败后旧应用可用性受 L02 同类问题影响。 |
| 重置程序 | 清理后删除应用配置并退出；不等于卸载服务、删除 OS 登录注册或撤销 UWP／防火墙。 | 若 AutoRun 原来开启，配置删除后默认显示关闭、OS 注册却仍可能存在，属于 L01 的确定触发场景。不可写“所有配置已在系统层重置”。 |
| 注销／关机／系统级终止 | GUI 没有独立的 session-ending 清理工作流，依赖 GPUI on_app_quit 与系统级 helper 自身信号处理。 | 不能以普通托盘 Quit 覆盖；见 L04。强杀不能保证执行任意清理。 |

主要 Suki 证据：[main](../src/main.rs#L94)、[close_to_tray](../src/tray/mod.rs#L101)、[托盘 Quit](../src/tray/mod.rs#L457)、[统一退出](../src/core/bridge.rs#L464)、[停止内核](../src/core/service.rs#L1981)、[应用自启动](../src/desktop/autostart.rs)。CV 证据：[关闭隐藏](../clash-verge-rev-dev/src-tauri/src/lib.rs#L291)、[静默建窗](../clash-verge-rev-dev/src-tauri/src/utils/resolve/mod.rs#L240)、[应用自启动](../clash-verge-rev-dev/src-tauri/src/core/autostart.rs)、[退出清理](../clash-verge-rev-dev/src-tauri/src/feat/window.rs#L133)。

## 续查发现的生命周期差距

| 编号／优先级 | 正常使用触发与当前结果 | 参考与处理边界 |
| --- | --- | --- |
| L01／高 | AutoRun 开关取保存值，OS 注册只有保存值发生变化才写。外部修改注册或“重置程序”删配置后，可能出现开关关闭但登录任务仍在。系统信息虽读实际注册，未统一到设置展示／协调。 | [设置提交](../src/core/service.rs#L1417)、[当前注册读取](../src/desktop/autostart.rs)、[重置](../src/core/bridge.rs#L502)。需分别处理应用登录注册与配置重置，不把独立 helper 自启动误判为同一问题。CV 同样区分保存值和系统注册；这里按本项目期望／实际状态契约列缺口，不声称 CV 每次启动必强制校正。 |
| L02／高 | 退出首先永久关闭存储和内核命令通道并 take worker；清理失败后取消退出，只留下窗口而未恢复执行端。再次退出取不到 worker 可按 Ok 处理，可能跳过上次未完成的服务内核／代理清理。重启创建新进程失败也可能留下已关闭各执行端的旧窗口。 | [finish](../src/core/bridge.rs#L464)、[shutdown](../src/core/service.rs#L223)、[存储 shutdown](../src/integration/service.rs#L281)。需将可取消退出与最终销毁分开，保留失败清理的所有权和结果；不能把第二次没有句柄当成已经清理。CV 明确区分 interactive cleanup 与会话结束清理，本项目应保持同等可恢复语义。 |
| L03／中 | macOS 关闭后应用仍在，代码只注册 on_open_urls 和第二实例激活，未注册官方 on_reopen。原生 Dock／Finder 打开既有进程不能等同于必定启动第二进程，缺少明确恢复主窗口的回调。 | [main](../src/main.rs#L62) 对比 [CV Reopen](../clash-verge-rev-dev/src-tauri/src/lib.rs#L377)。GPUI 提供 Application::on_reopen，应连接已有 show-main 路径；不创建新窗口／新内核。源码缺入口确定，具体 OS 展现尚未实机验证。 |
| L04／高 | 托盘 Quit 等自有命令等待完整清理，但 OS 级终止回调只用 on_app_quit join，受 GPUI SHUTDOWN_TIMEOUT=200ms 限制；没有 CV 那种独立 session-ending 有界清理。较慢 IPC／路由恢复可能超出该预算。 | [回调](../src/core/bridge.rs#L364)、[CV 系统结束](../clash-verge-rev-dev/src-tauri/src/lib.rs#L387)、[CV 有界清理](../clash-verge-rev-dev/src-tauri/src/feat/window.rs#L254)。已核对当前 gpui-fast 0.1.11 的 app.rs；需要区分可拦截退出与不可拦截系统结束，并与 helper 的恢复意图协调，不能承诺强杀必清理。 |
| L05／中 | SilentStart 在普通 open_window 之后才 defer hide；窗口选项没有按静默设置创建为不可见。与 CV 一开始 create_window(false) 不同，有初始窗口短暂出现的路径。 | [创建／隐藏顺序](../src/main.rs#L92)、[窗口选项](../src/app/window.rs#L12)、[CV 初始可见性](../clash-verge-rev-dev/src-tauri/src/utils/resolve/mod.rs#L240)。应在创建阶段处理可见性，同时保留托盘失败时恢复窗口；本轮仅确认顺序差异，未宣称实机观察到闪窗。 |

以上属于产品正常语义与生命周期的复核，不仅是报错重试。当前暂不新增“退出后卸载 helper”“AutoRun 同时控制系统服务”等行为；若照那样实现会改变 CV 和现有服务模型。

## 保留的明确产品差异

- GPUI Kit 官方组件与自动保存、Switch、文本失焦提交；不复制 Tauri/MUI 状态与命令体系。
- 代理组无侧边快速跳转；不透明组头／节点、小幅左右内边距；测速结果立即显示、批次结束排序。
- 单一连接表格；代理链入口／中继／出口、仅拖动排序、右侧类型／删除、不显示延迟／箭头；YAML 与表单切换沿用已有弹窗。
- 手动／PAC 右上角；仅有真实默认值的多行设置可恢复默认；用户订阅／覆写／链内容无恢复默认；删除／清空可撤销。
- 普通错误不自动换端口、改设置或切演示数据；权限错误与重复连接失败暂停，显式操作重试。日常 TUN 通过已安装服务，避免每次提权。
- 不加入已裁剪的字体、侧栏定制、全局快捷键、轻量模式和调色板；C12/C13/C33/C34 暂缓。

## 文档纠正与未完成项

- 两项绕过开关已恢复，旧附录“误删未恢复”过期；TUN 自定义托盘图标已接，不应继续写未接入。
- C17 的 next-update 接口缺失仍可作为原始 API 对照事实，但不再算本轮待开发；备注／计划时间不因审计自行加入。
- C10 已有 packaging 目录，旧表“未见任何打包材料”过期；仍缺完整发布链路。
- 历史 208／214／232 测试记录只代表各阶段快照，不是本次工作区的最新测试结果。
- 尚缺 C09 应用升级、C10 完整交付、C11 完整备份、C19 合成报告、C27 网站测试、C29 内存、C30 布局／绘制策略、C35 本地图标入口。其他代码差距见 R01–R61、生命周期差距见 L01–L05，平台验收见 C40。
- 本轮未重新跑构建、测试或实机 UI／OS 验收，不声称已逐像素审查所有窗口、所有故障组合。源码覆盖复核与平台验收分开记录。

参考指纹：`verge.rs` SHA-256 `acd20e809b2ddd46d637f40b42bc8df004a397b050cc0bcdac477b4871142a5a`；`ipc.rs` SHA-256 `a5b4cb5d36e2cdb13e482caa30bb37dd2a919d5b2bae11a9000eb118c10038e7`，本轮已重新核对。


## 第五批修复状态（2026-11-03）

R06/R07/R08/R52/R20/R56/R26/R27/R28已完成代码修复。测速输入与执行边界一致，原始延迟及明确失败类型贯穿领域、搜索和托盘；同批按来源身份去重。首页组选择与逐组测速URL由生命周期执行端写入统一程序设置的proxies分组，按订阅隔离，手动／自动测速共用。搜索、连接完整时间戳排序和关闭后零速率详情已统一；IP自动刷新偏好持久化并显示时区。原始编号的触发证据保留为修复前记录，最新检查与未验收边界见[修复清单](cv-repair-checklist.md)。剩余10项R、3项L及S01，不包含独立功能差距和待确认假设。

## 第六批修复状态（2026-11-03）

L01/L03/L05与R23/R24/R25/R31/R49/R50/R51/R48/R53/R54完成，至此61项R及5项L均已完成代码修复。历史证据保留；不是当前代码仍有这些缺陷的声明。服务日志采用revision 8会话游标窗口，补历史与增量共一序号所有者；不靠正文比较去重，不沿用旧协议。检查、未验收边界及六批后剩余功能见[修复清单](cv-repair-checklist.md#六批之后的剩余工作)。S01和功能缺项不因缺陷关闭而自动完成。

## S01与补充功能状态（2026-11-03）

S01的8项无消费者入口与依赖已清理，不改变用户旧文件。C15增强表单／来源候选／URI批量导入和C20嵌套组展示补充完成；Gist完整备份另见[实现说明](gist-backup.md)，不冒称CV原有Gist功能。用户新的暂缓／待产品决定／工具集成分类已写入功能总表，历史差距描述以新表为准。
