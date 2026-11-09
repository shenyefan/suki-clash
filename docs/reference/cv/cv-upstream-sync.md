# CV 版本与上游同步记录

核对日期：2026-11-05（北京时间）。参考目录已从 `5b7e876a` 快进到 `b9ab3b20`，开发版本声明为 2.6.0。官方稳定版为 [v2.5.10](https://github.com/clash-verge-rev/clash-verge-rev/releases/tag/v2.5.10)，并非 2.6.0。

## 当前同步

- 当前 dev：`b9ab3b2015fec1acaeddd5a59ae913ecc54fc184`，提交时间 2026-11-05 06:56:57 +08。
- 稳定版发布页对应：`c83f7ad529c7c9cb3d2d54a0955ef8c1b22a411b`，v2.5.10。
- 服务依赖仍为 v2.7.7／`3ed29700b093190b5e7addcaa892e77f172af544`；Suki 自有协议仍为 revision 9。
- 本次 27 提交、128 文件（125 修改、3 删除），固定范围和复查命令见下文“增量结论与复查范围”。U05 macOS 离线/换网代理恢复已独立适配；U06 GLOBAL 初次默认出口记录为策略差异，未自动采用。
- 参考工作树更新前后干净，只执行快进；本次没有替换目录或修改参考实现。旧 SHA 及摘要保留在 [JSON 历史](cv-reference-version.json)，新目录摘要使用明确记录的 Git 跟踪路径/模式/文件内容算法，历史摘要保留原算法。

以下保留 2026-11-03 的历史记录，不代表当前目录或最新稳定版。

## 2026-11-03 历史版本基线

| 对象 | 版本与身份 | 说明 |
| --- | --- | --- |
| 同步前本地目录 | 2.5.9，`eefd025806f255b85e7bc28158c30fdad477935d` | 全部747个文件路径与 Git blob 完全匹配；commit 时间2026-10-26 10:04:55 +08 |
| 当时同步后本地 `clash-verge-rev-dev/` | 2.5.10，`5b7e876ab0332620e100bf826fc84f20f1658acd` | 独立 Git 仓库，dev 跟踪 origin/dev，工作树干净 |
| 当时官方稳定版 | v2.5.9，`160bb4fe8eaef2bc626163d89622229e5d7c5b59` | 2026-11-02 18:09:17 +08 发布 |
| 当时核对的 dev | 2.5.10，`5b7e876ab0332620e100bf826fc84f20f1658acd` | commit 时间 2026-11-03 09:34:16 +08；当时开发版 |
| dev 的服务依赖 | v2.7.7，`3ed29700b093190b5e7addcaa892e77f172af544` | 同步前参考依赖为 v2.7.6，当前已同步为 v2.7.7；Suki 自有协议修订不能直接用该版本号替换 |

来源：[稳定版发布记录](https://github.com/clash-verge-rev/clash-verge-rev/releases/tag/v2.5.9)、[dev 固定提交的版本文件](https://github.com/clash-verge-rev/clash-verge-rev/blob/5b7e876ab0332620e100bf826fc84f20f1658acd/package.json)。机器可读基线、目录摘要、关键文件摘要和历史见 [cv-reference-version.json](cv-reference-version.json)。

同步前目录已通过完整文件集合与 Git blob 比对，精确定位为 `eefd025806f255b85e7bc28158c30fdad477935d`，不再是未知快照。与当前 dev 相差66个文件（新增4、修改62、删除0），Git统计1838行新增、737行删除。逐文件清单使用下文 Git 命令复查。

旧目录已完整压缩备份至 `clash-verge-rev-dev/.git/suki-reference-before-sync-eefd025-20261103.tar.gz`，并逐文件核验备份内容。原目录副本另保留在 `/tmp/suki-cv-before-sync-eefd025-20261103`；持久备份位于当前参考仓库的 `.git` 内，不依赖临时目录。参考目录继续由主项目 `.gitignore` 排除，未成为主项目子模块或主项目提交内容。

此前逐文件审计以旧提交为历史基线，不表示这66个更新文件已重新全文审计。后续可在参考仓库用 `git status`、`git log` 和 `git diff eefd025..HEAD` 查看精确状态；同步前先检查工作树，更新只允许快进，不能覆盖本地改动。

## 最值得补充的更新

同步后按实际39个提交进一步复核，逐项适用性见下文；服务职责见 [TUN 与服务](../../features/network/tun-service.md)。本轮U01–U03已完成代码补齐；U04保持语义参考项，不等同于已确认缺陷。实际检查与平台边界见下文。

| 编号／优先级 | 上游改动与触发条件 | Suki 当前核对结果 | 建议 |
| --- | --- | --- | --- |
| U01／高 | 服务模式 SSH 节点使用文件形式的 `private-key`；内核工作目录变化导致文件不可达 | 已补齐：[runtime_bundle.rs](../../../src/core/runtime_bundle.rs) 收集普通节点及Provider payload的文件私钥，复用同文件别名；内联密钥不变，越界／缺失明确失败 | 在现有资源打包边界登记受限本地文件并重写服务候选路径；保留内联私钥，禁止任意路径读取和泄露密钥。需要连同服务权限、归档秘密策略核对 |
| U02／高 | 服务内核自行更新 Geo 后，应用目录仍是旧文件；下一次本地校验读取旧分类 | 已补齐：[privileged.rs](../../../src/core/privileged.rs) 使用鉴权Geo回读，[readback.rs](../../../platform-service/src/core/runtime_generation/readback.rs) 使用固定白名单；[service.rs](../../../src/core/service.rs) 在校验前和Geo更新成功后同步 | 扩展明确的 Geo 白名单读回与同步，在后续校验／重新打包前协调最新资产。不能把服务较旧副本覆盖用户刚下载的新数据。此前 R38/R44 修复的是应用主动更新／暂存补偿，不覆盖此反向更新路径 |
| U03／中 | 全局模式显示 `GLOBAL` 及普通组，使用户能继续调整嵌套出口 | 已补齐：[model.rs](../../../src/proxies/model.rs) 全局模式显示GLOBAL与普通组，切换时GLOBAL优先，保留普通组排序／筛选／展开身份 | 按新上游展示普通组，同时保留当前不透明组头、内容边距和无侧边快速跳转的产品决定 |
| U04／跟踪 | 配置写锁由立即拒绝改为等待；DNS 覆写决定在运行配置替换后集中同步 | Suki 已有串行生命周期执行端；全局 DNS 的持久化与优先级是用户确认的独立契约 | 借鉴提交时机与锁顺序，不引入 CV 按订阅来源变化自动关闭 DNS 的策略；不能仅看到上游改动便判断本项目同样有故障 |

补丁依据：[SSH 私钥暂存](https://github.com/clash-verge-rev/clash-verge-rev/commit/a038e01cd5e885c9dceec38d40431f120b81da01)、[服务 Geo 同步与 v2.7.7](https://github.com/clash-verge-rev/clash-verge-rev/commit/726b2b362fe4e10fc4ff3df546bdc9a5f2ade284)、[全局模式组展示](https://github.com/clash-verge-rev/clash-verge-rev/commit/3027e2b87f7083ecf50bfb527f0d78c59b33a6bf)、[配置写锁等待](https://github.com/clash-verge-rev/clash-verge-rev/commit/0cae907de659b0856364e172fe7d2bf4ab9cb09e)、[最新 DNS／资源同步整理](https://github.com/clash-verge-rev/clash-verge-rev/commit/5b7e876ab0332620e100bf826fc84f20f1658acd)。U01–U03 是新上游对照项，现已完成Suki适配；不重新使用旧R编号。实际实现与验证边界见下文。

## 稳定版其他更新的适用性

- Mihomo 升至 **v1.19.33**：Suki 稳定内核下载走官方 latest 元信息，并非固定在旧版本；本次没有升级本机内核，也没有核对用户已安装的版本。
- IP 自动刷新、Windows `*.local` 默认绕过、测速结果及时刷新：Suki 已有对应实现，作为回归核对项；不代表经过三平台实机验收。
- 空规则列表项编辑修复：可用于增强输入边界回归。仍遵守本项目坏数据明确报错、保留草稿的规则，不照搬静默忽略。
- CV 删除托盘测速入口已包含在旧快照；Suki当前托盘也无测速命令，只展示共享测量结果，不重复登记为待移除入口。
- macOS 关闭窗口时恢复隐藏 Dock：Suki 已有隐藏应用与统一唤起路径，需原生窗口验收，不直接复制 Tauri 窗口实现。
- 自动备份重启、WebDAV 清空：对应功能当前暂缓；不扩大本轮范围。
- Muse AI／Cloudflare Risky：列入用户决定操作的未来仪表盘解锁能力，不自动接入。
- TUN 独立 DNS 模式、链式代理额外网速确认：不自动恢复用户已移除的选项或增加确认流程。
- Big Sur 的网页图表、自定义 CSS 编辑器字体：针对 WebView／React，不能直接用于 GPUI。
- Linux 安装包多余文件、Windows 安装／卸载先关闭实例、发布脚本与依赖安全更新：留给后续成熟交付工具集成；当前 Rust `zip` 已是 9.1.3，不重复登记依赖升级任务。

## 后续同步规则

每次核对或更新参考目录时，同时维护本文件与 JSON 基线：记录核对时间、稳定 tag／不可变 SHA、dev 版本／SHA、本地目录摘要、关键依赖版本；逐条登记采用／已有／不适用／待评估及原因。替换目录前先保留旧基线，替换后重新生成摘要；旧审计台账不得直接当作新快照的阅读证明。只知道版本号、不知道提交时明确标记未知，不借用父仓库 SHA。

本次记录是手动核对结果，没有开启自动联网监控或自动更新。

## 增量结论与复查范围

2026-11-03 的 eefd025..5b7e876 共 39 提交、66 个变更文件；2026-11-05 的 5b7e876..b9ab3b20 共 27 提交、128 个变更文件。两次均按 diff 与关键调用链核对，没有重做全仓全文审计。日期化的逐次报告及 CSV 不再单独维护；固定 SHA、摘要和历史保留在 [机器基线](cv-reference-version.json)，文件清单可在本地参考仓库重现：

```powershell
git -C clash-verge-rev-dev diff --raw eefd025806f255b85e7bc28158c30fdad477935d 5b7e876ab0332620e100bf826fc84f20f1658acd
git -C clash-verge-rev-dev diff --raw 5b7e876ab0332620e100bf826fc84f20f1658acd b9ab3b2015fec1acaeddd5a59ae913ecc54fc184
```

- U01–U03 已适配 SSH 文件私钥资产、Geo 白名单回读和 GLOBAL 加普通组展示；普通节点与 Provider payload 共用资产边界，秘密文件不加入备份白名单。Geo 分块及 EOF 检查身份，本地较新文件不被覆盖，部分同步与 API 已提交事实分别报告。
- U04 为配置提交时机参考，未证实为 Suki 缺陷；不引入按订阅自动关闭全局 DNS 的策略。
- U05 已修复 macOS 离线／换网自动代理恢复：无主网络先等待，换网使目标缓存与旧守卫失效，恢复前确认内核和端口；真实权限／写入失败仍暂停，不被换网解除。
- U06 为尚未采用的默认出口策略：没有保存选择时沿用内核选择，不自动把 DIRECT／REJECT 换成首个组或节点。Suki 保留 GLOBAL 与全部可见普通组，不随后台刷新滚动视口。
- 上游收缩 GLOBAL 列表可能隐藏多层嵌套组；备份解压移到订阅写锁之前存在共同写入阶段失去互斥的风险；默认出口启发式也不能保证一定代理。这些是固定 diff 的源码判断，没有运行 CV 实机复现，Suki 不照搬。
- React／WebView、Tauri、Monaco 和前端构建依赖的整理不直接移植；备份恢复与订阅修改仍由自身串行执行端负责。当前产品范围以功能差距总清单为准。

历史隔离回归覆盖资产路径、Geo 新旧与传输变化、来源白名单、全局嵌套组、macOS 离线恢复及真实写入失败暂停。实际服务安装／升级、SSH 文件节点、服务内核 Geo 更新和 macOS 换网／授权，以及 Windows／Linux 桌面权限联动仍需实机验收。
