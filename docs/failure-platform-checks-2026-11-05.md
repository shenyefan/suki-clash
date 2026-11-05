# 失败回退、桌面生命周期与图标检查（2026-11-05）

本次按用户决定：非文本控件提交失败恢复已提交或最近回读值，只有文本输入保留失败草稿。旧文档中“保留整份失败草稿”的约定由本次决定替代。

| 范围 | 实现及检查 |
| --- | --- |
| 全部设置页 | `SettingsPage` 公共提交入口同时处理同步拒绝、原生配置解析错误和异步失败；Toggle、Select、分段选择回退，文本/多行/YAML 保留。DNS/嗅探仅还原失败的非文本字段，保留未知 YAML 内容。主题、背景和通知配置随已提交值恢复。 |
| 系统状态 | 系统代理、TUN 保持既有实际回读；开机自启开关直接显示独立系统回读，不用失败草稿代替实际注册。 |
| 其他持久化控件 | Gist 加密、覆写启用与排序、订阅信息开关/范围、增强条目类型/来源/开关、代理链启用/组选择、隧道协议/代理以及 UWP 选择处理失败回退。文本草稿留在原输入实体中。 |
| 会话控件 | 搜索、日志等级、连接筛选、页面 Tab 等没有持久化提交，不执行保存失败回退。 |
| 关闭 | 原生窗口关闭与 Linux 自绘标题栏关闭共用 `desktop::close_main`；退出继续等待已有内核与网络清理。Windows 使用原生 ShowWindow 隐藏/重新显示，macOS 隐藏应用。Linux 当前 GPUI 没有隐藏 API，X11/Wayland 使用原生最小化，限制已写入 DESIGN.md。 |
| 命令行窗口 | Windows 主程序声明 GUI 子系统；启动脚本子进程设置 CREATE_NO_WINDOW；内核和任务计划程序已有无窗口标志。 |
| Windows 自启 | Task Scheduler 读取 XML 前关闭临时文件句柄；UTF-16、按权限注册、回读及失败恢复路径保留。未在 Windows 运行 schtasks 实测。 |
| 日志流 | Mihomo `/logs` 只写 WebSocket 帧，不读取客户端 Ping。停止要求 Pong；安静 30 秒时通过私有 `/version` 检查内核，保留连接关闭、失败重连、取消和代次切换。测试模拟只写无 Pong 的服务器，30 秒后接收真实测试消息。 |

## 图标接入

统一来源为 `assets/logo.png`；`assets/logo.ico` 包含 16、24、32、48、64、128、256 七档 PNG 帧。

- Windows：主程序资源 ID 1 为 GPUI 窗口/任务栏图标；服务、服务安装及卸载工具也嵌入同源图标；Velopack 安装程序使用同一个 ICO，应用快捷方式从主程序取得图标；协议注册补 DefaultIcon。
- macOS：打包仍生成完整 ICNS 并写入 CFBundleIconFile；运行时设置 Dock 图标；原生状态栏沿用同源图片及既有状态徽标。
- Linux：窗口 icon、app_id、桌面文件 Icon/StartupWMClass、AppImage 图标、协议桌面项及 hicolor 图标接入；托盘沿用同源图片。

## 验证及限制

- macOS 主程序默认完整测试：355 通过、16 忽略。另针对最终 UWP 回退运行相关回归。
- 设置回归覆盖同步拒绝后的开关/选择回退、文本保留、外部提交同步、DNS/嗅探 YAML 和系统代理分段控件；标题栏原生按钮已有鼠标及键盘回归。
- 日志流 5 项回归通过，使用隔离临时 Unix Socket；不修改真实网络和内核。
- 打包 Python 回归 5 项通过；ICO 七档目录、PNG 格式及各帧尺寸检查通过。
- 主程序格式、编译、Clippy 检查；服务使用发布所需 standalone,client 特性编译。服务默认无特性构建存在原有 serde_json 可选依赖错误，不属于本次图标逻辑。
- Windows 交叉检查因当前工具链缺失目标标准库而失败，不能视作通过；没有运行真实 Windows 任务计划程序或 Windows/Linux 桌面。
- 当前未运行新版本真实窗口，也未生成并安装三平台发布包；Dock、托盘、任务栏、安装器、快捷方式和 Linux 桌面集成的实机外观/交互仍待验证。自动 UI 回归及图标结构检查不能替代这些验收。

## macOS 系统代理授权会话与维护页签

通用设置最后一个页签由“同步与维护”改为“维护”，分组及 Gist 功能不变。

本地代理执行端原先在每次 `write_native` 调用中创建 `AuthorizationRef`，写完随局部变量释放；启用、关闭、失败恢复分别重新创建会话，无法持续使用该授权实例的凭据缓存。现将授权引用移到 macOS Adapter，首次需要写入时创建，由同一个串行执行线程在后续开关、清理和恢复中复用，执行端销毁时释放；系统设置仍按每次操作新建 SCPreferences 并回读。代理后端在各自工作线程内部创建和使用，因此移除 Backend 不必要的 Send 限制，让授权引用保持线程内所有权，无需为原生句柄增加 unsafe Send。

只有用户请求的可交互操作允许请求权限。后台守卫及服务维持原有非交互语义；不保存密码或授权引用到磁盘，不改 macOS 的授权策略。授权过期或系统策略要求重新验证时仍可能提示，不能保证跨启动或永久免授权。

参考本地 CV 的 `core/proxy_control.rs::apply`：macOS 按运行模式分流，本地模式经 Sysopt/sysproxy，服务模式经 `set_system_proxy_by_service`。其 sysproxy 原生路径使用普通 SCPreferences，不自行创建交互授权窗口，权限不足返回 RequiresAdminPrivileges；常驻提权服务可在日常开关时执行写入。因此不能将 Suki 每次创建交互授权会话的现象解释为 CV 或 macOS 必然要求每次确认。

参考 [Apple 授权缓存说明](https://developer.apple.com/library/archive/documentation/Security/Conceptual/authorization_concepts/02authconcepts/authconcepts.html)：系统同时管理全局缓存和每个授权实例的缓存，是否提示取决于权限规则和有效凭据。

验证使用原生代理字典/清理回归及模拟执行端事务测试，不切换真实系统代理，不触发实际授权对话框。连续开关后的弹窗次数尚需用户在真实窗口验证；代码中复用授权引用不等于已经实测免提示。
