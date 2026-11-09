# 本地备份与待确认差异处理

状态更新（2026-11-09）：用户确认文档此前列出的实机验收部分均已完成，相关待验收项关闭。下文各次检查中的“未验收／待验收”保留为当时记录；本次未重新运行验收。实现范围、暂缓项及能力限制以[当前功能对照](../../reference/cv/cv-gap-checklist.md)为准。

日期：2026-11-03。参考CV dev `5b7e876`；本次不修改参考仓库，不操作真实服务、系统DNS或凭据。

## 本地备份

设置 → 维护 → 备份与恢复，提供创建、本地历史和目录入口。历史弹窗使用既有GPUI官方对话框／按钮／虚拟列表及原生文件选择器，支持创建、导入、导出、恢复、删除。重复入口和原先仅备份本页设置的第二套恢复命令／格式已移除；不迁移旧文件。

本地与Gist使用同一当前归档、清单摘要、路径／类型／总量校验和生命周期恢复工作流。归档原始内容总量32MiB，本地编码文件48MiB以内；Gist远程上传仍独立限制8MiB。历史文件放在数据目录 `backups/local/`，独立稳定文件名，不以列表下标作身份。新建本地备份不要求GitHub或凭据；导入Age文件时使用已保存的Age私钥，无密钥明确报错。导入只验证并加入历史，不立即覆盖当前配置。

备份包含订阅／覆写／增强与节点内容，默认本地归档未加密；不包含系统凭据库、OS注册、内核、日志、缓存或SSH暂存私钥。本机同步配置和GUI登录注册保留。恢复复用配置校验／应用／独立文件和凭据补偿，停止时只保存，不启动内核。删除和恢复等待5秒，toast可撤销；恢复先确认。文件导出使用原生保存对话框，不用手写路径输入模拟。

## Provider覆盖与链物化

已确认并修复：原物化只读原节点，未应用Provider override／重命名；HTTP/file备用payload还可能遮住有效缓存。现在按原始名执行有序filter／exclude-filter／exclude-type与原名去重，先应用Provider dialer，再属性override、顺序proxy-name、前后缀。链顺序最后决定dialer。持久来源继续绑定原始名称，最终同名使用独立候选别名；修改prefix可重新绑定，最终名碰撞不会错取原节点。HTTP缓存继续要求URL／内容来源证明。

支持当前布尔／字符串／routing-mark覆盖、普通正则及经检查的数字／命名捕获替换。**高级regexp2语法、override-expr及部分.NET特殊替换尚不支持链物化**，明确拒绝并保留旧链，不以Rust正则近似或静默忽略。正常Provider仍交Mihomo执行。这是已确认的能力限制，不能说全部Provider表达式等价，也不再归为未知语义。

依据：[Mihomo provider parser](https://github.com/MetaCubeX/mihomo/blob/Meta/adapter/provider/parser.go)、[Provider override](https://github.com/MetaCubeX/mihomo/blob/Meta/adapter/provider/override.go)；实现见[chain_sources.rs](../../../src/proxies/chain_sources.rs)。参考主分支为核对时内容，后续同步需再次比对，不能等同于固定版本身份。

## macOS DNS／TUN

CV对fake-ip TUN改写活动网络服务为占位DNS。Suki采用已确认的更合适方案：缺省 `tun.dns-mode` 在macOS fake-ip TUN候选中派生native，不增加设置、不改持久来源；用户显式disabled/native/hijack保留。Mihomo临时注册catch-all resolver，关闭TUN／退出进程时释放，不覆盖用户持久系统DNS，不需要另一套提权DNS脚本、恢复备份或所有者。

已核实Mihomo v1.19.33依赖sing-tun v0.4.28；v1.19.32依赖0.4.27不具备该实现。应用不按版本字符串猜能力，而通过实际私有 `/configs` 回读enable和明确请求的dns-mode。旧核忽略字段／未成功启动时明确失败并沿现有候选补偿；关闭TUN不要求该能力。

证据：[Mihomo v1.19.33 go.mod](https://github.com/MetaCubeX/mihomo/blob/v1.19.33/go.mod)、[sing-tun临时resolver](https://github.com/MetaCubeX/sing-tun/blob/v0.4.28/internal/systemconfiguration/dns.go)、[Darwin配置与关闭](https://github.com/MetaCubeX/sing-tun/blob/v0.4.28/tun_darwin.go)、[内核DNS拦截／blacklist](https://github.com/MetaCubeX/mihomo/blob/v1.19.33/listener/sing_tun/server.go)、[实际configs响应](https://github.com/MetaCubeX/mihomo/blob/v1.19.33/hub/route/configs.go)、[实际TUN配置](https://github.com/MetaCubeX/mihomo/blob/v1.19.33/listener/listener.go)、[dns-mode导出字段](https://github.com/MetaCubeX/mihomo/blob/v1.19.33/listener/config/tun.go)。

## 下载／TLS与回归

当前共享reqwest使用平台证书验证，逐订阅self-proxy／system／direct严格策略与TLS缓存隔离已实现。CV按错误文本重试静态证书根、URL携带凭据等不是本项目待补：保留无自动安全策略回退、无秘密URL、无命令行备用方式。清除了仅含一个策略的无意义循环和永远不会出现的重试文案。

新增专项覆盖：本地完整归档创建／导入／导出／恢复不启动内核、坏文件不改配置、历史删除撤销；Provider属性／重命名／原名绑定／最终名冲突／缓存优先／不支持表达式保留原配置；macOS配置矩阵与真实私有API契约模拟；不安全重定向拒绝且不转发秘密／不重跑，自动测速间隔／代次取消／迟到结果隔离。

真实服务／DNS流量／崩溃恢复、跨平台凭据和本地文件选择器完整工作流仍需实机验收；不以源码或模拟响应宣称实机完成。原生隔离窗口本轮仅确认维护入口，完整弹窗验收未完成。

最终检查：根项目340项通过、16项条件忽略；服务库沿用本轮90项通过／1项条件忽略（其后未修改服务代码）；格式、严格Clippy和差异检查通过。真实平台验收范围不因自动测试通过扩大。
