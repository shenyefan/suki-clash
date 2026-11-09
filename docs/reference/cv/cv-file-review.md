# CV 与 Suki 逐文件复核台账

2026-11-05 参考目录同步说明：原 CV 文件集已精确匹配上游 eefd025，11-03 同步至 5b7e876，当前已快进至 dev 2.6.0／b9ab3b20。下列 CSV 保持原全文阅读基线，后续 66 文件及本次 128 文件增量不自动计作全文重审；版本与范围见 [同步记录](cv-upstream-sync.md)。

更新：2026-11-02。本台账回应“逐个文件、每段代码核对”的要求，不能用先前能力抽查代替全文阅读。实际差距统一写在 [修复清单](../../repairs/cv-repair-checklist.md)，功能状态同步 [总清单](cv-gap-checklist.md)。

**已登记文本的全文阅读完成，功能修正与平台验收未完成。** 当前清点938个文件：936个文本文件、2个二进制资源。全文已读936个，局部已读0个，待读0个；全文阅读247873行。这里统计的是源码阅读覆盖，不是验证通过数，也不表示这些文件涉及的所有调用链已经闭合。

第一批修复后说明：CSV保留2026-11-02修复前的阅读基线，哈希不再代表修复后所有当前文件；macOS原生适配器与代理事务已提取到platform-service共享模块。修复后的变更和检查以[修复清单](../../repairs/cv-repair-checklist.md)为准，不将旧全文覆盖直接转算为新实现已全部复核。

2026-11-05 服务入口整合：原 `src/bin/install_service.rs`、`uninstall_service.rs` 及共享维护模块迁至 `platform-service/src/maintenance/`，由唯一服务可执行文件分流。CSV 保留原路径与哈希作为历史阅读记录，不冒充新路径已按原哈希全文复审；现状见 [实施与验证](../../features/network/tun-service.md)。

## 文件范围与状态

[CSV 明细](cv-file-review.csv) 每行一个文件，保存侧别、路径、总行数、SHA-256、已读行段、状态、对应实现和发现。不能通过批量搜索、读取行数／哈希、自动脚本或只看函数名把 pending 改成全文已读。

- `read-full`：当前哈希对应文件已经从第一行读到最后一行，包含文件内测试；对照对象可能还待读，另列在 counterpart/findings。
- `read-partial`：只读了 read_ranges 列出的行，余下不能算已审查。区间中断和输出截断不算完整阅读。
- `pending`：已登记、未完成全文阅读；即使前几轮按函数抽查过，本轮也不冒称全文已读。
- `binary-no-code`：二进制没有可逐行阅读的源码；打包引用与实际视觉／平台验收分开记录。

范围含 Suki `src/`、`platform-service/src/`；CV `src/`、`src-tauri/src/`、`crates/`；根配置、打包、脚本、CI、权限与资源模板等代码／配置。额外纳入 CV 锁定的 sysproxy 六个源码文件（补入macOS network模块）与 service-ipc 的49个源码文件及launchd/systemd两份模板，外部路径和固定提交在 CSV 记录。除此之外排除依赖、target/node_modules/dist/build 等产物、IDE元数据、文档／skill自身，以及 assets/icons 纯静态图像目录。图像存在／引用／解码／缓存的代码仍在范围；静态图像不冒充行为源码。不是“两个仓库全部字节均已阅读”的声明。

文件名和架构不同，映射以职责为准：UI入口 → 类型／状态所有者 → 命令 → 来源／持久化 → 执行 → 回读 → 正常返回／失败补偿 → 关闭／重启恢复。没有 Suki 实现的 CV 文件保留待读／未接能力，不编造一比一文件关系。配套服务的上游源码参考与 fork 也不能因来源相同就免审。

哈希改变后，旧阅读记录不再覆盖当前文件，须标为待复核；新增文件须补入台账。CSV 记录当前工作区快照，不拿父仓库提交号冒充 CV 版本。

## 本轮已全文阅读的主要范围

| 范围 | 阅读与对照结果 |
| --- | --- |
| Suki bootstrap 两文件 | 完整读取安装逻辑及测试；对照完整 CV utils/init.rs。清单／哈希、临时写入、不覆盖用户文件是明确改进；不复制时间戳覆盖与旧数据迁移。 |
| Suki storage.rs、storage/ 两文件 | 完整读取目录、JSON/YAML分界、读写／备份／恢复与测试；对照 CV dirs/snapshot 等。目录分离按用户约定，重置系统注册问题仍见L01。 |
| Suki configuration/ 全部10文件 | 合成、原始来源、增强、脚本、DNS／嗅探／TUN、资源、隧道、偏好与测试均全文阅读。CV enhance/ 全部9个源码文件也已读完；DNS、clash、port等配置全文对照。R01/R09/R14/R21等有具体证据。 |
| Suki 订阅领域与执行端 | editor/model/store/remote/repository/override_repository/organize/mod/view、凭据三文件及 core/profiles、core/overrides 已全文阅读；CV PrfItem、IProfiles、ProfileViewer全文阅读。enhance/overrides UI也已全文读完；原文保留缺口见R57。 |
| 下载与资源基础 | network、core/download、core/install、core/resources全文阅读；CV NetworkManager全文阅读。内核升级执行端已全文读并发现R39，相关UI与平台安装调用链也已全文阅读，具体补偿缺口见R39/R46/R47，不把底层阅读算为全部交付对齐。 |
| 节点选择与搜索 | core/proxy_control、Search全文阅读；CV IProfiles的恢复工作流与SearchMatcher全文阅读。代理来源 runtime、链配置／编辑器／执行端、日志与 traffic 全部源码已全文读；其他代理页也已全文阅读，不宣称界面实机验收。 |
| 应用入口与桌面 | main.rs及desktop/全部6文件全文阅读；CV autostart/schtasks/server/resolve-scheme全文阅读。应用关闭、原生重开、系统结束仍需沿用L01–L05继续追调用链。 |
| 程序偏好基础 | preferences/app/native/json/choices、权限读取、failure工具全文阅读；schema／kind／初始化已全文读，界面设置消费者也已全文阅读。 |

已确认差距新增 R09–R61：自定义绑定被覆盖、同名作用范围误绑定、明确更新间隔被响应头覆盖、远程订阅结构门槛、下载代理开关互斥、外部文件编辑不进入来源、部分凭据修改清掉另一项、已提交后清理失败的UI、首份订阅默认选择、Provider冷启动选择恢复、来源卡片查询凭据、共用正则、DNS监听地址，以及协议导入明确名称丢失。

本次续读新增：Suki/CV 原生托盘、连接表格与详情、日志视图／实时监督、仪表盘与IP信息、服务会话／令牌／身份、Provider运行包与同步、链式代理和来源解析。确定新增 R23–R39，包含服务会话48/64位协议冲突（R29）、macOS helper代理写入缺口（R30）、正常服务优先级（R34）、exclude-type分隔符（R35）。服务和运行包代码来源相同也逐行核对，不因“参考CV”免审。

本轮继续全文核对 Windows／Linux 代理和服务安装、执行锁、看门狗、启动残留／状态持久化。新增 R40（RAS连接覆盖）与 R41（KDE回退信息丢失）；固定依赖参考更正为 sysproxy `89ad876`，服务参考为 v2.7.6 `b2a7427`。看门狗3次重启与进程身份核验保留，不因和CV数值不同就认定缺陷。

2026-11-01续查：补齐 CV manager/lifecycle 全文及服务 server 全文，继续读完服务状态、代理执行、日志、归属锁、维护、安装检查、Unix权限和目录等文件；上游对应范围按CSV逐文件登记。新增 R42：服务状态读取坏文件时静默默认；此逻辑来自上游，本项目仍须按明确报错契约改进。R30补充helper内部拒绝写入／空清理的证据，不重复编号。

本轮运行包续查：两边 runtime_generation/assets、staging、mod全文读完，并补上游readback；两边daemon与维护共用入口、launchd/systemd模板全文读完。新增R43旧目录自动清理、R44部分资产写入后旧manifest继续被信任、R45 macOS把所有监督错误视为预算耗尽并成功退出。Windows SCM最终None动作、Linux启动频率限制属于已有有界恢复改进，不要求改回CV无限重启。

安装／卸载续查：两边安装器、卸载器、SELinux标记全文读完；Suki Windows网络／UWP弹窗、应用壳三文件、存储桥接及编辑器／动作公共文件全文读完。另纳入ClashSuki的UWP和防火墙两个只读源码快照。新增R46防火墙安装错误丢失、R47 Unix卸载未确认停止、R48 UWP迟到成功关闭其他弹窗。

日志／测量续查：存储执行端、诊断导出、程序日志／Feed、内核生命周期容器、网卡枚举、独立测速任务全文读完；对照CV logger包装、logging crate和delay服务全文。新增R49整段日志过滤、R50日志创建失败封闭存储执行端、R51字节分卷切断UTF-8记录、R52同批测速未去重同一来源身份。诊断包当前主要检查设置，不代表已覆盖订阅／覆写正文与服务故障材料；与C38后续补全一起记录。

设置页续查：settings除core_tools/view外其余消费者均已全文阅读；发行包解析全文读完。CV TUN/WebUI弹窗、feat/proxy全文对照，保留用户指定的自动保存、无页面重置／独立DNS模式等产品差异。新增R53复制终端代理的关闭端口、R54内核状态搜索结果被覆盖、R55自动路由与重定向只改草稿而未共同提交。

公共界面／平台身份续查：settings/core_tools与view已读完，settings目录全部消费者完成全文阅读；ui公共布局、modal、theme、notifications、download、motion、virtual_rows全文读完，包含内联测试但未运行。对照CV隧道弹窗及服务Windows ACL、管道身份、通道标识全文；旧权限迁移／目录重建按明确约定不接入，不误报为功能缺失。读取服务构建、macOS应用／helper元数据、NSIS模板和三个测试fixture入口。NSIS模板未找到构建消费者，其卸载脚本缺服务调用先记为闲置打包材料问题，不当作当前安装入口已触发的故障。本批未新增确定R编号。

代理组界面续查：mod/model/view/node_list四文件全文读完，包含内联测试但未运行；CV filter-sort、delay分类与按订阅组头状态全文对照。确认不透明组头／节点、横向内容内边距与无侧边跳转符合明确产品选择。新增R56旧组筛选残留且清除遗漏；R07补充超时错误显示为失败、通过错误文案推断状态问题（未测图标符合当前DESIGN），不拆成重复编号。

参考弹窗续查：CV ProfileItem（997行）和DNSViewer（1101行）均补齐全文。订阅按已有C17确认范围核对，不因参考含下次更新展示就擅自扩大需求；CV DNS的按订阅作用域／页面重置／读取失败默认／先保存再校验不符合本项目明确规则，保留为差异与改进，不列缺陷。

增强／覆写续查：Suki enhance（1212行）与overrides（480行）全文读完，CV ProxiesEditorViewer（500行）全文对照。新增R57错误增强原文被空草稿替代。CV多条节点URI导入与按前置／追加批量解析在Suki增强表单尚无对应入口，记入C15功能差距；不复制参考的明文URI日志。

服务可信路径续查：两边trusted_core_location全文对照，上游auth/desired/paths/process/client及reconcile/runtime/state/lib/execution_windows全文读完；Suki服务Cargo和测试凭据fixture全文读完但未执行。Windows ACL、服务注册归属、Unix内核路径规则主体一致，品牌／通道名差异不列缺陷；上游旧状态备份／隔离／默认继续不复制进当前格式契约。CV基础空态／错误边界／加载／页面／控件包装和返回顶部／更新入口也已全文阅读；CV整页错误替代不作为Suki必须实现的行为。用户现已授权子代理批量审阅，后续覆盖在父代理复核报告与hash后合并。

子代理首批合并：CV base/layout剩余16文件与profile编辑器14文件完成全文读取，父代理复核hash、对应证据后登记；sysproxy macOS/guard与此前遗漏的network模块也已全文读完。补齐两边service集成driver/fixture及上游旧清理模块阅读，未执行任何测试／清理。新增R58复合规则表单截断、R59密码trim；C15补记组字段／来源候选覆盖、中文下拉标签，C38补记完整诊断复制入口。

子代理第二批合并：CV hooks22文件及交付54文件全文阅读；父代理复核系统代理／模式清连接消费者后新增R60。交付补入旧台账漏收的Windows installer.nsi与get_latest_tauri_commit.bash，sysproxy/network模块也已纳入，当前总数按实际清点修正。应用发布／签名／公证／升级仍沿用C09/C10缺项，不新增重复故障编号；CV按进程名批量结束、TCP reset、旧文件迁移等不会当作本项目必需实现。Suki系统代理隔离测试全文已读但未运行。

代理组件子批次：CV代理组件／来源类型17文件4035行全文读取并合并，父代理复核未解析成员UI和执行端校验，新增R61。嵌套组当前节点及协议能力标记展示尚未完整覆盖，记为C20/C22显示差距；链布局、无侧边导航与会话状态均按用户决定保留。

设置／交付续批：CV设置消费者23文件4353行、发布／开发workflow及脚本23文件3975行全文审阅并合并，父代理补读cmd/app、network、system、uwp、validate、listener、verge、service入口。无新增重复R。CV系统代理先关弹窗后异步保存、controller先写开关后校验、运行配置读错替注释均不套用；stable发布draft门槛与autobuild先公开后构建已区分记录，不能统称所有渠道都完成产物后发布。

服务桥接／命令续批：CV services/providers20文件1846行全文读取；父代理补齐clash/save_profile/backup/webdav/lightweight/media_unlock命令入口。未新增独立R；YAML schema校验／联想、PAC类型提示为当前原生编辑器能力差异，记C19/C36，不要求引入Monaco。CV请求完成后缓存刷新失败混同提交失败、test_delay错误返回10000成功值、解锁检测默认忽略证书等不作为需要复制的逻辑。

设置剩余7文件已补齐：本批分工共30文件6945行全文阅读，包含layout/update与五个backup界面。无新增确定R，C09/C11缺项及C12/C13暂缓保持；不恢复用户移除的侧栏／hover／字体／热键／轻量模式。备份历史迟到响应、更新以release文案作为禁止条件、清图标无撤销等参考自身问题均不照搬。

内核测试源码2562行全文补齐：读取生命周期／候选恢复、订阅／覆写、服务隔离、链式、资源、DNS、实时流与系统代理断言，未执行。不能把已有路径断言当作R02/R39系统代理补偿或R36并发新连接回读已经覆盖。Suki src/与服务src/全部登记源码现已全文读完；两份Suki锁文件仍待读，CV业务／资源／翻译／crate及工具仍继续。

并行续批已复核合并：后端15文件1841行、页面组件32文件4936行、utils/services/store48文件4510行全文阅读，另补signal/limiter/i18n/sysinfo九个crate文件。无独立新增R，R60闭合CV模式清连接，R05补规则Provider更新后同步；C15 URI导入、C27/C29/C30仍按已有差距。CV getAutoProxy读错返回false、非法mode自动写rule、Provider全更吞错后总成功等不复制；框架polyfill与语言fallback不新增到Suki当前契约。

11月2日续批：合并上轮后端5文件1173行与bindings1046行完成记录，全文读取draft源码／测试／bench、sysinfo剩余实现、logging manifest及media-unlock全部15文件。C33仍暂缓；读取检测实现不等于外部服务当前可用或已经验证。参考部分网络失败／缺区域推断No/Yes不作为当前项目已确认状态策略，不执行外部请求。

11月2日并行首批：core/config六文件2817行、utils/feat二十五文件3467行、长页面四文件3727行已全文合并；feat/window仅1–175局部阅读。R04补连续3次失联与私有端点确认，R08补首页组选择／测速URL，C30补focus绘制证据，未新增独立R。macOS public DNS placeholder步骤仅记待闭合，不因缺脚本就断言Suki TUN失效。

11月2日后续合并：core-next三文件、utils-next四文件（补齐window全文）、global类型878行及父代理十六个开发环境／perf文件全文完成。无独立新R；CV退出停止失败分支虽发取消通知却缺return继续退出，作为参考自身缺口记录，不能照搬。性能对比包含PID/启动身份、可见状态／输入量、独立样本及不可比较判定；仅阅读，不运行或宣称当前性能验收。

2026-11-02 续批：合并 CV core/config 第三、四批11文件5602行，生成翻译类型及 zh/en/zhtw/de 42文件8982行、锁文件阶段结果；父代理补完性能运行器及报告测试1014行、后端14种语言资源1288行。未新增独立R。参考校验eval超时不终止执行、热加载任意错误转重启、快捷键忽略执行错误后通知等均不照搬；多语言仍按C34暂缓，翻译键不构成新增设置授权。大锁与其余语言、脚本、静态资源仍待读，精确局部范围见CSV。

续批合并：翻译维护脚本、已登记SVG/SCSS、西班牙语资源及Suki主锁已全文读完；CV Cargo锁仅1–2700行，保持局部状态。无新增独立R。静态资源与锁定依赖阅读不等于视觉或供应链验证；参考翻译清理脚本未执行。

## 全文阅读完成与剩余核对

2026-11-02：补完CV两份大锁及所有已登记语言资源。936个文本文件均已实际阅读全文，CSV无pending或partial；2个二进制资源保留binary-no-code。此处的完成仅限上述登记范围的文本阅读，不涵盖排除的第三方依赖源码、所有仓库字节、运行验证或所有故障组合。

- 后续工作以R01–R61、L01–L05、S01及功能总清单为依据；本轮没有修复这些缺陷。
- 仍需处理报告中的待确认语义，例如Provider override物化与macOS系统DNS链路；未确认项不混入已确认缺陷数量。
- 真实平台服务／系统代理／TUN／权限／退出恢复、界面视觉与性能验收另列；源码阅读和锁文件哈希不能替代这些证据。

目前只修改审计文档与台账，未修改产品源码，未运行测试或执行真实网络／服务／凭据操作。文档链接、CSV哈希／行段及差异检查单独核查；不能据此宣称全部行为正确或跨平台验收完成。
