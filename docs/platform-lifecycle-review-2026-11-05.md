# 关键平台生命周期链路复核（2026-11-05）

本轮范围：Windows、macOS、Linux 的系统代理、TUN、登录自启动、窗口关闭、托盘重开、内核停止和程序退出。以当前本地 CV 为参考；不修改 CV、不安装或卸载真实服务，不切换真实系统代理、TUN 或路由。源码核查不能替代平台集成验收。

## 本轮修复

| 触发场景 | 原问题 | 处理 |
| --- | --- | --- |
| 停止时保存 TUN 开启、Linux TProxy 参数 | 预先检查内核及服务权限，阻止仅保存意图 | 仅运行时检查服务和提权能力；停止时保存，下次启动继续检查实际要求。不会因保存启动内核，实际 TUN 开关仍取回读。 |
| 停止时启用 DNS/嗅探配置 | 缺内核时连保存来源都被拒绝 | 与停止时仅保存契约一致；系统代理启用仍必须有可用内核及端口。 |
| macOS/Linux 本地内核停止 | 直接 SIGKILL，跳过正常退出清理 | 应用与服务复用 owned Child 停止逻辑：先检查退出；Unix SIGTERM，给定宽限后强制结束并确认回收；Windows 继续使用子进程句柄结束，未伪造 SIGTERM 能力。 |
| Windows 隐藏到托盘后退出清理失败 | activate 不保证显示已隐藏窗口 | 复用 show_main，先原生显示再激活，错误 toast 和重试入口可见。 |
| Linux AppImage 自启动/协议注册/重启 | current_exe 指向挂载目录内部，退出卸载挂载后路径不可用 | 共用持久启动目标：APPIMAGE 存在时要求绝对有效文件；普通二进制仍用 current_exe。集成维护的服务程序仍从 current_exe 同目录定位，这是包内资源，不应改为外层 AppImage。 |
| Linux 安装路径含空格、反斜杠或特殊字符 | auto-launch 的 XDG Exec 直接拼接未引用路径；协议入口也缺少完整两层转义 | 应用层生成符合 desktop-entry 的命令参数，供现有 auto-launch 和协议注册复用。含等号的可执行路径按规范明确拒绝。 |

上轮 macOS 授权会话复用仍在当前改动内：代理执行线程持有 AuthorizationRef，写入/清理/回滚共用，后台不自动弹权限窗口；真实弹窗次数待验证。

CV 更新后追加 U05：macOS 自动代理恢复遇到无主网络服务时延后，断网/换网失效旧目标缓存；恢复时确认内核和端口再应用。权限/写入失败继续暂停，换网不解除。两项新增隔离回归通过，当前应用全量 361 通过、16 忽略；此前下文的 359 项是追加前记录。参考固定 SHA、适用性及新增上游风险见 [更新对照](cv-update-comparison-2026-11-05.md)。

## 平台路径与 CV 对照

| 能力 | Windows | macOS | Linux | CV 参考与差异 |
| --- | --- | --- | --- | --- |
| 系统代理 | GUI 账户的 WinINet；默认连接及 RAS 连接；各连接快照、回读、通知和归属清理 | 本地 SystemConfiguration 或鉴权常驻服务；逐服务快照/回读/归属清理；本地交互授权只限用户操作 | GNOME gsettings / KDE 5、6 kioslaverc + DBus 刷新；不支持桌面明确报错 | CV sysopt、proxy_control、sysproxy 原生后端。均须先就绪再启用、关闭守卫后清理；未复制 CV 在部分失败时自动关闭 TUN 的策略。 |
| TUN | 普通用户通过已安装 SCM 服务；已有管理员权限可按现有策略直接运行 | launchd 服务；已有 root 可直接运行 | systemd 服务；已有 root 可直接运行；不增加 setcap 流程 | CV feat/tun、core/manager、服务执行端。Suki 保持已保存意图，服务不足报错，不静默改写配置或起第二个内核。 |
| 配置及服务交接 | 候选校验及归属交接 | 服务就绪/协议/归属确认，失败恢复旧配置及代理 | 同一配置工作流及交接约束 | configure → validate → replace_committing / configure_hot；只有结构化 RestartRequired 才重启。服务安装/卸载的已提交事实不因恢复失败被抹去。 |
| 自启动 | 普通/管理员任务按 SID、权限区分；UTF-16 临时 XML 关闭后交 schtasks 读取；保存失败恢复原任务 XML | 用户 LaunchAgent，和 CV 选择相同机制 | XDG 用户 autostart，AppImage 写持久路径 | CV core/autostart、utils/schtasks、lib::setup_autostart。GUI 自启动和常驻提权服务是两种注册，不混淆。 |
| 关闭与重开 | 关闭到托盘 ShowWindow 隐藏，重开显示并激活 | 关闭到托盘隐藏应用；Dock/reopen/实例激活进入 show_main | 自绘关闭已接统一入口；框架没有 hide，现为原生最小化 | CV CloseRequested 及 feat/window。Linux 最小化仍可能保留任务栏项，不宣称真正隐藏到托盘。 |
| 退出 | Win32 Child/Job Object；系统代理清理成功再停内核 | 停守卫 → 清理代理 → 停内核/TUN → 释放服务会话 | 同上，系统代理留在用户桌面会话执行，不让 root systemd 服务写用户 gsettings | CV 退出也要求确认内核停止。Suki 交互退出失败保留 UI 与执行端；不可取消的系统结束最多等待现有清理线程 3 秒。强杀无法保证用户态清理。 |

## 完整性与仍待验收项

- 自动代理处理失败会暂停，显式操作后恢复；不把轮询当无限提权重试。目标变更仍需回读，不把持久化成功当 OS 已生效。
- 服务与本地子进程共享执行排他约束。服务请求携带账户/代次/token，错误会话不得停止别人的内核；丢应答后先回读，不直接启动替代实例。
- Windows 任务的 exists/Enabled 回读仍不足以证明实际登录执行成功；macOS/Linux auto-launch 的状态查询主要检查注册文件存在，不能证明系统允许后台启动或启动目标已执行。完整登录/重启场景仍需实机验证。非 Windows 的注册回滚仍按启用状态重建，不是任意外部修改文件的逐字节恢复。
- 真实 Windows RAS、macOS 换网/授权过期、GNOME/KDE 会话及代理刷新、三平台服务安装修复、TUN DNS/路由释放、登录启动、关机/注销、托盘不可用等均未操作实机。
- 上述限制不通过权限放宽、兼容读取、迁移旧文件或自动改端口掩盖。

## 验证

- 新增隔离回归：停止时保存 TUN/DNS/嗅探而系统代理仍要求就绪；AppImage 与普通二进制启动目标；桌面 Exec 特殊字符；子进程 TERM 清理、忽略 TERM 后强杀与回收、重复停止。
- 服务测试必须启用 `standalone,client,test`，其中 test 将服务状态及 IPC 切到临时目录。初次缺少该特性的运行有权限失败，使用正确组合重跑通过；不据初次失败修改系统服务目录权限。
- macOS 应用完整回归及主程序/服务 Clippy、编译、格式检查以本轮工具输出为准。
- Linux 交叉检查遇到 E0463（缺目标标准库）；Windows 同样没有可用交叉标准库。本轮没有三平台交叉编译通过的结论。

## 来源入口

- Suki：[系统代理生命周期](../src/core/system_proxy.rs)、[代理事务](../platform-service/src/proxy_owner.rs)、[配置及内核生命周期](../src/core/service.rs)、[服务归属](../src/core/privileged.rs)、[统一退出](../src/core/bridge.rs)、[桌面](../src/desktop/mod.rs)、[持久启动目标](../src/desktop/launch.rs)。
- CV：[系统代理](../clash-verge-rev-dev/src-tauri/src/core/sysopt.rs)、[平台代理分流](../clash-verge-rev-dev/src-tauri/src/core/proxy_control.rs)、[TUN](../clash-verge-rev-dev/src-tauri/src/feat/tun.rs)、[内核停止](../clash-verge-rev-dev/src-tauri/src/core/manager/state.rs)、[退出](../clash-verge-rev-dev/src-tauri/src/feat/window.rs)、[自启动](../clash-verge-rev-dev/src-tauri/src/core/autostart.rs)。
- [AppImage APPIMAGE/APPDIR 规范](https://docs.appimage.org/packaging-guide/environment-variables.html)、[Desktop Entry Exec 转义](https://specifications.freedesktop.org/desktop-entry/latest/exec-variables.html)。

最终结果：应用 359 通过、16 忽略；服务使用 standalone,client,test 的库测试 92 通过、1 忽略，安装工具 21 通过、卸载工具 1 通过。主程序构建、应用和服务的严格 Clippy、双 crate 格式及差异检查通过。未运行真实三平台窗口/系统网络操作。
