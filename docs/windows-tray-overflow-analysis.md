# Windows 原生托盘长菜单溢出箭头（2026-11-07）

用户在 0.3.8 的 Windows 截图中确认：HMENU 内容已为深色，但超长子菜单顶部／底部的溢出箭头保留白底。上一轮应用到 `#32768` 窗口的主题 hook 没有解决该机器上的问题，现已删除。应用仍使用原生系统托盘和 Win32 菜单。

## 原生能力核对

- 微软 [MENUINFO](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-menuinfo) 公开最大高度、菜单背景及样式。内容超出最大高度由系统提供滚动；该结构没有溢出箭头颜色或暗色主题字段，设置菜单背景不能据此保证箭头也改变。
- [SetWindowTheme](https://learn.microsoft.com/en-us/windows/win32/api/uxtheme/nf-uxtheme-setwindowtheme) 改变窗口的主题关联，不能把关联修改等同于已控制所有内部绘制路径。上次此方向的实际用户结果为无效。
- [DrawFrameControl](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-drawframecontrol) 区分菜单和滚动箭头绘制；部分类型输出黑白掩码。仅凭截图不能确定用户 Windows 版本内部具体用了哪条绘制路径，不能拿 Wine／ReactOS 的实现当作 Windows 源码证明。
- 微软 [Win32 深浅色说明](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/ui/apply-windows-themes) 没有提供独立的 HMENU 溢出箭头暗色开关。当前项目和 GPUI Kit 的 HMENU 深浅色均使用动态 uxtheme 策略入口，不能保证每种 Windows 版本的所有部分一致。

没有找到经公开接口说明且可在当前 Windows 上验证有效的纯原生主题修复。局部拦截绘制、修改进程导入表或额外覆盖箭头，均属于应用接管绘制，不能叫作单纯开启原生主题；本次不添加这些未经实机验证的实现。

## GPUI 当前依赖核对

当前锁定 GPUI Kit 0.9.0：`gpui-component/src/native_menu/windows.rs` 在 `run_menu` 调用 `TrackPopupMenuEx`，仍走 Win32 HMENU，因此换成 `NativeMenu` 不绕过这一系统限制。

`gpui-component/src/menu/popup_menu.rs` 的 `PopupMenu` 具有原生组件的子菜单、键盘／焦点和 `max_h`／`scrollable` 能力，并使用 GPUI 主题。它是应用窗口内菜单，没有发现可直接替换当前三平台托盘的独立组件。若要用于 Windows 托盘，需要继续用 `tray-icon` 提供系统图标，以专门的 GPUI 弹出窗口承载菜单，并处理屏幕边缘定位、DPI、多屏、点击外部关闭和焦点恢复。macOS／Linux 可继续使用现有原生菜单。

## 当前实现

用户已选择三端原生托盘图标配 GPUI 轻量浮层。策略组无图标，模式图标对应实际模式；不使用标题／搜索／返回面板。使用 GPUI List 的虚拟化、选择与键盘语义和 Base Positioner 的定位、翻转及边界约束，以保留大规模节点列表性能；当前 PopupMenu 的内建滚动内容并未虚拟化。透明宿主承载级联浮层，表面使用应用主题色与透明度，不对整个桌面应用原生模糊。

Windows 不再使用 HMENU，因而不再出现该菜单的白底溢出箭头。Windows／Linux 当前仅验证编译，仍需桌面实测。Wayland 的精确定位依赖 Layer Shell；不支持时报告原因，不自动打开主窗口。Linux 托盘宿主是否调用标准 ContextMenu 也属于桌面集成验收范围。
