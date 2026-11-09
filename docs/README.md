# 文档索引

产品视觉与交互规则见根目录 [DESIGN.md](../DESIGN.md)，架构、状态与执行边界见 [AGENTS.md](../AGENTS.md)。历史检查中的日期、测试数量和平台范围只代表当时验证，不代表当前工作区已经重新验收。

## 目录用途

| 目录 | 内容 |
| --- | --- |
| `features/` | 按功能维护的配置、执行契约与使用说明 |
| `build/` | 本地构建、打包和应用更新 |
| `repairs/` | 修复清单、实施进度与持续维护的故障处理契约 |
| `reviews/` | 组件、平台和日志审核及未验收项 |
| `reference/cv/` | CV 功能差距、参考版本及源码阅读基线 |

文档使用稳定主题名；阶段性修复结论并入对应文档，日期写在正文，不再按日期创建独立报告。

## 功能与维护

| 范围 | 文档 |
| --- | --- |
| 设置 | [设置存储](features/settings/settings.md)、[设置来源核查](features/settings/settings-origin-audit.md)、[开关执行接入](features/settings/settings-switch-wiring.md) |
| 订阅与配置 | [订阅](features/profiles/profiles.md)、[规则／覆写／DNS／嗅探](features/configuration/flow.md) |
| 运行数据 | [仪表盘与代理组、连接](features/runtime/runtime.md)、[统计与小时账本](features/runtime/dashboard-data.md) |
| 网络与平台 | [网络与私有通道](features/network/network.md)、[系统代理](features/network/system-proxy.md)、[TUN 与服务维护](features/network/tun-service.md) |
| 日志与诊断 | [日志](features/logging/logging.md)、[诊断导出与托盘](features/logging/diagnostics-tray.md) |
| 备份 | [Gist 备份](features/backup/gist-backup.md)、[本地备份及 Provider／DNS 边界](features/backup/local-backup-and-parity.md) |
| 构建与交付 | [本地跨平台构建](build/local-cross-build.md)、[Velopack 打包与更新](build/velopack.md) |

## 修复与检查

- [修复清单](repairs/cv-repair-checklist.md)：R／L／S 编号、修复条件及剩余工作。
- [实施记录](repairs/cv-implementation-progress.md)：分批接入与历史验证范围。
- [内核就绪与服务错误处理](repairs/runtime-readiness.md)：启动、热更新和恢复的统一等待及错误分类。
- [组件使用审核](reviews/ui/component-audit.md)：公共组件、页面、弹窗、按钮与菜单的检查及验收边界。
- [平台生命周期](reviews/platform/lifecycle.md)：跨平台启动、关闭、退出、授权、失败回退和待验收项。
- [日志审核](reviews/logging/logging-audit.md)：日志级别、文案、归属与已知问题。

## 参考对照

- [功能差距总清单](reference/cv/cv-gap-checklist.md)：功能状态、明确暂缓项及平台验收。
- [逐文件阅读台账](reference/cv/cv-file-review.md)及[CSV 明细](reference/cv/cv-file-review.csv)：历史阅读范围与哈希，不作为当前代码已复验的证明。
- [版本与同步记录](reference/cv/cv-upstream-sync.md)及[机器基线](reference/cv/cv-reference-version.json)：固定提交、摘要、增量适用性和复查命令。

字体来源、重建方式与渲染检查见 [字体说明](../assets/fonts/README.md)。
