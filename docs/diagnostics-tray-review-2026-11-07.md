# 诊断配置导出与原生托盘复核（2026-11-07）

## 诊断包

原先仅输出设置检查结果和文件大小，无法判断实际开启的 DNS、TUN、控制端口及程序偏好。现在额外输出 `configuration/` 下的实际文件脱敏副本：

- `settings/settings.json`：完整程序设置分组，包括通用／外观、系统代理、内核、DNS、嗅探及备份偏好。
- `settings/mihomo.yaml`、`dns.yaml`、`sniffer.yaml`：实际持久化的内核原生设置。
- `records/`：订阅／覆写索引、代理选择和订阅增强。当前格式中增强片段为 YAML 字符串，先按其已定义格式处理，再写回脱敏 JSON。
- `profiles/`、`overrides/`：订阅和 YAML 覆写；安全凭据文件与任意 JavaScript 正文不进入包。
- `runtime/`：当前可读取的本地 `core/run-*/candidate.yaml`；另包含存在时的 `runtime-source.yaml`。不越权读取系统服务的受限目录，不把本地候选文件描述为内核 API 的实际回读。

配置脱敏在 ZIP 写入前完成。保留开关、端口、列表顺序和非敏感字段；遮蔽密码、Token、鉴权、自定义请求头、私钥、UUID、代理服务器、订阅 URL 的私有路径／参数，以及应用和用户目录。代理 URI 可能把凭据整体编码在 URL 的 host 部分，因此非公共网络／DNS URL 的地址整体遮蔽。安全凭据库不读取，源文件不改写。

格式损坏的 JSON／YAML 不按其他格式重试：保留行数、空白和结构标点，其余字符掩码，文件名追加 `.redacted.txt`；状态文件记录原错误位置。单文件超过 8 MiB、总读取超过 32 MiB、普通文件读取失败或符号链接均记录原因，其他文件继续导出。设置和候选配置优先于大订阅，避免订阅占满限额后丢失核心设置。日志仍只导出本次运行登记的文件及内存尾部。

## 三端托盘

| 平台 | 当前实际实现 | 主题及溢出区域 |
| --- | --- | --- |
| macOS | `tray-icon`／`muda` 的 AppKit 状态项与 NSMenu | 应用设置 Aqua／Dark Aqua；菜单布局和超长内容由 AppKit 处理。 |
| Windows | `tray-icon 0.26.1` 创建 Win32 状态图标；`muda 0.21.2` 创建 HMENU，库调用 TrackPopupMenu | 原实现只给隐藏的托盘所属窗口设置深浅色。现在也对创建或复用的 `#32768` 原生菜单 HWND 应用 MENU 主题；UI 线程范围的消息 hook 随托盘释放。系统继续负责布局、菜单项、溢出箭头、滚动和键盘。 |
| Linux | 启用 `tray-icon` 的 `ksni` 后端，将 muda 菜单快照通过 StatusNotifier／DBusMenu 导出 | 菜单和溢出滚动区域由桌面宿主绘制并随宿主主题；该协议不接受应用的颜色或滚动条样式。桌面需要提供 StatusNotifier 宿主，不能承诺所有未扩展的桌面环境都显示托盘。 |

源码核对点：`src/tray/platform.rs`、`src/tray/windows.rs`、`src/tray/macos.rs`，以及当前锁定的 `tray-icon` Windows／ksni 后端和 `muda` Windows 菜单实现。未修改依赖或参考仓库。

Windows 使用应用自身 UI 线程的 `WH_CALLWNDPROCRET` 观察创建／显示消息，调用系统 `SetWindowTheme` 更新实际弹出窗口。没有进程外注入、DLL 导入表修改、自绘菜单或自绘滚动条。参考微软 [SetWindowTheme](https://learn.microsoft.com/en-us/windows/win32/api/uxtheme/nf-uxtheme-setwindowtheme) 的窗口级关联及主题刷新契约；HMENU 深浅色策略仍沿用 GPUI Kit 的动态 uxtheme 入口。

## 验证范围

诊断导出 4 项回归通过，覆盖实际设置／订阅／增强／候选文件、嵌套凭据、格式损坏副本、源文件保留、符号链接和部分导出状态，以及本次运行日志范围。完整主项目回归 390 项通过、16 项条件忽略；严格 Clippy 和本次变更文件格式／差异检查通过。

使用现有本地跨平台构建入口，Windows x64、Windows ARM64、Linux x64 的主程序和独立服务 release 编译检查通过。此验证证明平台代码可编译，不代替真实 Windows 长菜单主题切换、滚动区域及 Linux 桌面宿主的视觉验收；本批没有 Windows／Linux 桌面实机验收。
