# CV 更新差异复核（2026-11-05）

## 固定范围

参考目录在工作树干净的前提下执行 `git pull --ff-only origin dev`，从 `5b7e876ab0332620e100bf826fc84f20f1658acd` 快进到 `b9ab3b2015fec1acaeddd5a59ae913ecc54fc184`。共 27 个提交、128 个文件（125 修改、3 删除），新增 2456 行、删除 3379 行；版本声明从 2.5.10 变为 **dev 2.6.0**。参考工作树更新后仍干净，不修改其实现。

官方 [latest 发布页](https://github.com/clash-verge-rev/clash-verge-rev/releases/tag/v2.5.10) 此次核对为 **v2.5.10／c83f7ad529c7c9cb3d2d54a0955ef8c1b22a411b**；以下多数修复属于其后的开发版，不能称为稳定版已经交付。服务依赖仍为 v2.7.7／`3ed29700b093190b5e7addcaa892e77f172af544`，不替换 Suki 自有服务和 revision 9 协议。

完整路径及前后 blob 见 [128 文件清单](cv-reference-sync-2026-11-05.csv)，版本、目录摘要和历史见 [机器基线](cv-reference-version.json)。本次按提交 diff、关键调用链和 Suki 对应所有者复核；不是重新全文审计参考仓库全部文件，也不扩大旧逐文件审计的覆盖声明。

## 修复适用性

| 上游变化与证据 | Suki 核对结果 | 本轮处理 |
| --- | --- | --- |
| [f076d4cd](https://github.com/clash-verge-rev/clash-verge-rev/commit/f076d4cd5de03eaf0c9d4fca72dce2843ca5760f)：macOS 离线登录延后代理恢复，缺网络不耗尽守卫失败次数；换网时同步代理/DNS | **发现同类缺陷，已修复 U05。** 原 `sync_system_proxy` 将无活动网络的应用失败也视为永久暂停；目标缓存仅比较端口及设置，不含网络服务身份，守卫关闭时换网不会重新应用 | 读取 SystemConfiguration 主网络服务 ID。自动恢复在无服务时显示待确认并等待；断网或服务变化使旧目标/守卫缓存失效。恢复时重新确认 API 和混合端口，再非交互应用并回读。真实权限/写入失败仍暂停，换网不能解除暂停。显式操作仍返回真实错误 |
| 同一提交：按服务 ID 保存原 DNS、托管标记、多个占位 DNS、恢复脚本及旧备份迁移 | Suki 已使用 Mihomo 原生临时 resolver，没有同一套持久 DNS 占位/备份脚本 | 保持 `tun.dns-mode: native` 的既有策略和显式原生选择；不引入 DNS 迁移、持久占位或按错误自动改设置。真实换网后的 resolver 行为仍待实机验收 |
| [8039feb2](https://github.com/clash-verge-rev/clash-verge-rev/commit/8039feb270664e7b97b1f4fe28aae452d1cbd21b)：备份恢复统一配置写锁，模式保存加锁，ZIP 解压放后台 | Suki 的配置/模式/备份/自动订阅更新由同一个生命周期 Actor 串行执行，备份先暂存校验再提交，有独立恢复材料；在专用工作线程执行，不在 GPUI 回调中解压 | 没有 CV 多全局草稿/写锁的同一路径；不增加第二套全局锁。检查 `core/service.rs`、`local_backup.rs`、`gist.rs`、`backup/mod.rs` |
| [2b7b7cc0](https://github.com/clash-verge-rev/clash-verge-rev/commit/2b7b7cc0e78acdbb6e79e3d4442ee5266459f5e3)：安装前 latest 报价变化时刷新对话框，不把旧报价当普通安装错误；签名验证交给插件 | Suki 使用 Velopack，保留检查得到的完整资产；下载使用固定版本 URL/文件名/摘要，安装前再次核对暂存版本。新 latest 不会使已展示版本被偷偷替换 | 无同一缺陷。旧资产被上游撤回时仍明确报错并允许重新检查，不在后台改装另一版本；不复制 Tauri updater 状态 |
| [f65a7f13](https://github.com/clash-verge-rev/clash-verge-rev/commit/f65a7f13489a19e18667c013d4b8578549dab5c1)：服务操作后同时刷新托盘菜单及图标 | Suki `tray::sync` 对核心状态更新同时调用 `platform.update(system, tun, ...)`，图标不依赖菜单 revision 是否变化 | 已有对应状态链；真实托盘显示仍需三平台验收 |
| [1105eede](https://github.com/clash-verge-rev/clash-verge-rev/commit/1105eedeb69d42cb847983801251384595adab26)：DNS 表单/YAML 不再经重复 effect 相互覆盖，系统代理提交减少重复刷新 | Suki 由保留的表单草稿和统一配置工作流持有状态，没有 React effect/SWR 的回写链；失败时文本保留、枚举/开关回到已提交或回读值 | 未发现同一框架问题；不引入第二份 DNS 持久状态或保存按钮 |
| [84bbe790](https://github.com/clash-verge-rev/clash-verge-rev/commit/84bbe790e8fff315d73bdbe6c73f784f66db071e)：无 GLOBAL 保存记录时，将内核默认 DIRECT/REJECT 改为第一个组/节点 | **新增策略差异 U06，未采用。** Suki 恢复明确保存的选择，无记录时沿用内核选择，首次进入 global 仍可能为 DIRECT | 这会影响默认出站，不是框架修补。记录为待确定的产品策略；本轮不自动替用户选节点，也不把“可能默认直连”写成已修复 |
| [943e18e8](https://github.com/clash-verge-rev/clash-verge-rev/commit/943e18e8cafb121b6d7375d01237ee6c4379c896)：global 页仅显示 GLOBAL 及其直接选中的组，并在用户选择后定位该组 | Suki U03 保持 GLOBAL 置顶及全部可见普通组，支持多层嵌套调整；已有覆盖 `GLOBAL → outer → inner → leaf` 的模型回归 | 保留已采用的交互，不自动缩减列表或在后台刷新时滚动用户视口；参见下方上游新增风险 |
| 后端整理：共享 bypass 构造、Windows 复用已有提权、退出/重启清理复用、端口读取合并、增强处理函数合并 | Suki 本地/服务代理共用同一 `Target` 构造；服务管理已区分当前权限；退出/重启共用生命周期清理；端口在输入边界校验 | 不移植 CV 命令/全局状态。上游端口数值 `as u16` 和 0 自动替默认是既存政策，本项目继续拒绝无效值/保留明确关闭值 |
| 前端整理：WebSocket 共享订阅、traffic Worker、派生选择、测速状态、链 YAML 解析和窗口监听 | Suki 原生后台监测有代次/配置 epoch、取消、有限缓冲及断流状态；图表按用户决定仅保留最近 30 秒；链编辑使用 YAML 边界校验 | React/WebView 整理没有直接移植点。没有发现这批整理对应的新增 Suki 缺陷；不以此代替真实 UI 验收 |
| IP 检测将 AbortController 放入每次重试；TLS 客户端构造复用 | Suki 每个出口检测请求使用独立超时，失败换源继续走同一代理，已有隔离回归。共享 HTTP 客户端按传输/证书政策区分 | 无已中止信号复用的问题，不引入 CLI 下载路径或降低 TLS 验证 |
| 依赖、发布、CI：Vite 安全更新、Tauri/Specta、ZIP 9.3.0、pnpm action、翻译类型检查与构建缓存 | Suki 不使用 Vite/React/Tauri/Specta/pnpm，交付采用 Velopack；当前根锁文件 ZIP 为 9.3.0，服务独立锁文件无 ZIP 依赖 | 根锁文件更新在本轮前已存在，不归为本次修复。上游没有在此 diff 给出 ZIP 安全缺陷依据，不仅凭版本号联动改服务依赖或覆盖正在进行的打包修改 |

## 新上游代码中的风险

以下是固定 diff 的源码判断，没有运行 CV 桌面或修改它的代码，不宣称已经实机复现。

1. **多层全局组在代理页变得不可见。** `src/components/proxy/use-render-list.ts` 现在只返回 `[global, current.group]`。当 `GLOBAL → outer → inner → leaf` 时，`inner` 不在该列表；旧版包含全部普通组。Suki 已有的嵌套组回归证明自己的列表保留 `inner`。CV 其他入口可能仍能调整它，不能扩大为整个 CV 无法编辑。
2. **备份解压期间失去原有订阅文件写锁。** `8039feb2` 提取出的 `feat/backup.rs::restore_backup_archive` 先 `spawn_blocking(zip.extract(home_dir))`，完成后才取 `PROFILE_WRITE_LOCK`；旧代码在解压前持有该锁。备份恢复与订阅编辑/更新重叠时，共同写入用户目录的阶段没有该锁保护。Suki 没有采用这一结构，恢复和订阅修改保持串行；并不因名为 lock safety 就认定全部改动更安全。
3. **GLOBAL 自动默认值不等于一定代理。** 新 `global_exit_target` 的备用分支只排除 DIRECT/REJECT；没有可见组时，其他内置策略或隐藏组仍可能成为候选。它也用“没有持久化记录”判断默认选择，未记忆的显式选择与内核默认值无法完全区分。Suki 本轮不引入该启发式。

## 实现边界与验证

U05 修改仅涉及 macOS 的只读主网络服务查询、系统代理缓存及自动恢复调度；普通进程和鉴权服务模式共用该调度，没有改 IPC 格式。Windows/Linux 沿用各自后端，没有套用 macOS 网络服务判断。本次没有新增旧格式读取、迁移、自动换端口、权限放宽或命令行备用路径。

新增隔离回归覆盖：离线启动/连续轮询不触发写入失败暂停；网络回来重新校验内核，不能把代理指向未确认监听端口；断网/切换服务使缓存与旧守卫失效；真实写入失败的暂停不被换网解除；缓存失效后原生所有者仍可清理其写入。测试用 Memory 后端和可控网络探针，不写真实代理。

- 应用全量测试：**361 通过、16 条件忽略**；服务 `standalone,client,test`：库 **92 通过、1 条件忽略**，安装器 **21 通过**，卸载器 **1 通过**。
- 应用/服务严格 Clippy、应用构建、两工程格式与差异检查通过。测试 socket 需要沙箱外本地监听；普通沙箱首次运行的两个监听权限失败未计为通过。
- 没有操作真实系统代理、DNS、TUN、服务安装、自启或登录/关机；没有运行 CV 或重新验收原生窗口。真实 macOS 换网/离线登录/授权过期，以及 Windows/Linux 桌面链路仍需实机验收。当前工具链的跨目标编译限制见 [平台复核](platform-lifecycle-review-2026-11-05.md)。

## 提交索引

完整顺序可用 `git -C clash-verge-rev-dev log --reverse --oneline 5b7e876a..b9ab3b20` 重现：

```text
55bb74c3 changelog
c83f7ad5 Release v2.5.10
6aeae2f9 build caches / release team
d7964e6d Dev v2.6.0
9a904af3 backend cleanup
8039feb2 backend lock safety
40fd1d50 frontend cleanup
2b7b7cc0 changed updater offers
891757ed Vite security update
1105eede redundant DNS / proxy updates
5d41f99e Specta grouping
01fbc571 Specta formatting
8be05c17 Specta PR scheduling
943e18e8 GLOBAL list
9fb46607 Cargo dependencies
e4d82a1b Tauri dependencies
84bbe790 unpicked GLOBAL default
14abc4c4 React effects
f65a7f13 tray service refresh
97dc1622 updater GitHub endpoint order
f076d4cd macOS DNS / proxy network changes
27da92b5 npm dependencies
39ba1e64 unused dependencies
dae542f7 i18n type drift CI
a15ff6b4 frontend CI paths
a45c6813 pnpm action
b9ab3b20 ZIP 9.3.0
```
