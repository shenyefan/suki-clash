# 平台生命周期与验收边界

状态更新（2026-11-09）：用户确认文档此前列出的实机验收部分均已完成，相关待验收项关闭。下文各次检查中的“未验收／待验收”保留为当时记录；本次未重新运行验收。实现范围、暂缓项及能力限制以[当前功能对照](../../reference/cv/cv-gap-checklist.md)为准。

整理自 2026-11-05 的平台链路检查。功能状态以对应功能文档和修复清单为准；本页保留跨平台行为与尚未完成的系统验收，不将历史源码检查视为当前实机通过。

## 平台路径

| 能力 | Windows | macOS | Linux | CV 参考与差异 |
| --- | --- | --- | --- | --- |
| 系统代理 | GUI 账户的 WinINet；默认连接及 RAS 连接；各连接快照、回读、通知和归属清理 | 本地 SystemConfiguration 或鉴权常驻服务；逐服务快照/回读/归属清理；本地交互授权只限用户操作 | GNOME gsettings / KDE 5、6 kioslaverc + DBus 刷新；不支持桌面明确报错 | CV sysopt、proxy_control、sysproxy 原生后端。均须先就绪再启用、关闭守卫后清理；未复制 CV 在部分失败时自动关闭 TUN 的策略。 |
| TUN | 普通用户通过已安装 SCM 服务；已有管理员权限可按现有策略直接运行 | launchd 服务；已有 root 可直接运行 | systemd 服务；已有 root 可直接运行；不增加 setcap 流程 | CV feat/tun、core/manager、服务执行端。Suki 保持已保存意图，服务不足报错，不静默改写配置或起第二个内核。 |
| 配置及服务交接 | 候选校验及归属交接 | 服务就绪/协议/归属确认，失败恢复旧配置及代理 | 同一配置工作流及交接约束 | configure → validate → replace_committing / configure_hot；只有结构化 RestartRequired 才重启。服务安装/卸载的已提交事实不因恢复失败被抹去。 |
| 自启动 | 普通/管理员任务按 SID、权限区分；UTF-16 临时 XML 关闭后交 schtasks 读取；保存失败恢复原任务 XML | 用户 LaunchAgent，和 CV 选择相同机制 | XDG 用户 autostart，AppImage 写持久路径 | CV core/autostart、utils/schtasks、lib::setup_autostart。GUI 自启动和常驻提权服务是两种注册，不混淆。 |
| 关闭与重开 | 关闭到托盘 ShowWindow 隐藏，重开显示并激活 | 关闭到托盘隐藏应用；Dock/reopen/实例激活进入 show_main | 自绘关闭已接统一入口；框架没有 hide，现为原生最小化 | CV CloseRequested 及 feat/window。Linux 最小化仍可能保留任务栏项，不宣称真正隐藏到托盘。 |
| 退出 | Win32 Child/Job Object；系统代理清理成功再停内核 | 停守卫 → 清理代理 → 停内核/TUN → 释放服务会话 | 同上，系统代理留在用户桌面会话执行，不让 root systemd 服务写用户 gsettings | CV 退出也要求确认内核停止。Suki 交互退出失败保留 UI 与执行端；不可取消的系统结束最多等待现有清理线程 3 秒。强杀无法保证用户态清理。 |

## 共同执行边界

- 停止内核时保存 TUN、Linux TProxy、DNS 和嗅探来源，不提前要求运行权限或因保存启动内核；启用系统代理仍须确认内核与监听端口。
- Unix 自有子进程先检查退出，再 SIGTERM，宽限后强制结束并确认回收；Windows 使用自有进程句柄。服务与本地进程共享执行排他约束，不按进程名批量结束。
- Linux AppImage 登录启动、协议注册和重启使用有效的绝对 APPIMAGE 路径；包内服务仍从实际可执行文件同目录定位。XDG Exec 按规范转义，含等号的可执行路径明确拒绝。
- 非文本持久控件提交失败回到已提交或实际回读值，文本／多行／YAML 保留草稿。此检查覆盖设置、Gist、覆写、订阅增强、代理链、隧道和 UWP；会话筛选与页面 Tab 不涉及持久化回退。
- Windows 主程序使用 GUI 子系统，启动脚本子进程使用 CREATE_NO_WINDOW。任务计划的 UTF-16 XML 在调用 schtasks 前关闭临时文件句柄。
- Mihomo 日志流不要求 Pong，安静 30 秒时通过私有 /version 检查内核；保留取消、重连和代次隔离。

## 图标与授权

图标源为 assets/logo.png；Windows ICO 包含 16、24、32、48、64、128、256 七档。Windows 主程序／服务／安装包、macOS ICNS／Dock 和 Linux 窗口／桌面项／AppImage 使用同源图标。协议入口及 Linux hicolor 资源均属于桌面集成验收范围。

macOS 本地系统代理 Adapter 在线程内持有 AuthorizationRef，用户交互的写入、清理和恢复复用同一授权会话，销毁时释放；每次操作仍新建 SCPreferences 并回读。后台守卫不弹授权窗口，不持久化密码或授权引用。授权过期仍可能提示，连续开关的实际弹窗次数尚需实机验证；已有服务模式继续使用鉴权服务能力。

## 已由用户确认完成的实机验收（2026-11-09）

- Windows RAS、任务计划实际登录执行、托盘隐藏后清理失败的窗口重开及 SCM。
- macOS 换网／离线登录、授权过期、Dock／Reopen 和系统注销。
- GNOME／KDE 用户会话代理刷新、AppImage 启动及 Wayland／X11 桌面行为。
- 三平台服务安装／修复／卸载、TUN DNS／路由释放、登录启动、关机和托盘不可用。
- 三平台安装器、任务栏／Dock／托盘／快捷方式的真实外观和交互。

历史隔离回归覆盖停止时保存、启动目标与转义、进程 TERM／强杀回收、控件失败回退及日志无 Pong 场景；没有更改真实系统网络。后续跨平台编译范围见 [本地构建](../../build/local-cross-build.md)，编译通过不替代上述验收。
