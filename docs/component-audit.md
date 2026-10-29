# 组件使用审核

日期：2026-10-29。依据：GPUI Kit 0.8.1 源码、项目 Coding Guides、Design Guides 与 DESIGN.md。

| 范围 | 核对与处理 |
| --- | --- |
| 公共弹窗字段 | `ui::field_label` 改用 Form / Field；标签作用域隔离原生字段身份。`ui::form` 保留为字段、横排和辅助内容的布局栈 |
| 全部公共空状态 | 改用 Empty / EmptyHeader / EmptyMedia / EmptyTitle / EmptyDescription；页面与内嵌区域仍各自分配高度 |
| 设置数值 | 全部数值字段直接使用原生 Input，不提供步进按钮；沿用 InputState、失焦提交和原提交工作流 |
| 订阅编辑、增强、隧道 | 更新间隔、超时和端口使用原生 Input，不保留数字输入包装或步进逻辑 |
| 覆写列表 | 原生 ListItem 负责行交互，按官方 List 示例组合拖放事件；使用上下插入线与 drop_target 背景，原生行设置圆角、滚动视口保留内距，避免悬停背景覆盖外层圆角；虚拟化视口保留 |
| 订阅与代理组 | 可点击卡片使用 Button，选中、禁用与键盘沿用原生行为；不因存在 List 而改换卡片结构 |
| 日志、连接、规则 | 原生 DataTable；选中恢复仍保留视口，当前 TableState 没有不滚动的选择设置接口 |
| 外部资源、增强列表及其他集合 | 原生虚拟列表与覆盖式 Scrollbar；保留单一滚动所有者 |
| 设置开关、选择、页面切换 | Switch、Checkbox、Select、TabBar 已使用原生组件 |
| 按钮、菜单、弹窗 | Button、DropdownButton、PopupMenu、Dialog、AlertDialog 与原生内容/底栏槽位；关闭按钮使用原生 Button 调用关闭动作，无需重复包装 |
| 下载、通知 | Progress、NotificationList；通知的应用层 Root 插件仅提供用户要求的覆盖层级 |
| 动效 | Presence、MotionReveal、Collapsible 和原生 transition；不替换为应用层计时器 |
| 侧栏、标题栏、仪表盘与详情 | 导航使用原生 Button，状态和图表沿用现有原生控件；布局保留。Windows 标题栏命中区域保留平台原生命中测试要求 |

保留 `div` 的场景为结构布局、文字、领域内容、身份作用域和平台窗口命中区域。原生 SidebarMenuItem 存在，但当前侧栏已用原生 Button 承担导航行为，无需为替换名称重写既有折叠动效。

本记录为组件选型与调用方审核，不等于全平台视觉验收。回归检查覆盖表单对齐、数值输入与禁用、端口框内按钮、拖放插入线、虚拟化及既有交互；原生窗口的主题、尺寸和平台表现仍需实机验收。

验证：184 项测试通过，16 项忽略；Clippy（全部目标、警告视为错误）通过。拖拽测试检查插入线在目标行上边/下边的位置及松开后移除；端口测试检查普通输入框类型和随机按钮的框内边界。数字输入测试检查文本编辑、变更通知及保存中禁用，未单独完成数值控件失焦保存的原生窗口验收。桌面读取工具超时，圆角视觉效果及跨平台表现尚未实机确认。

悬停问题复核：GPUI Style::overflow_mask 返回矩形 ContentMask，不读取 corner_radii。单独设置外层 rounded + overflow_hidden 不能裁剪子项圆角；覆写列表改为原生行自身圆角和视口内距，悬停与滚动背景均避开外层圆角。回归检查首行悬停前后几何及视口内距，并保留插入线与落点测试。

拖动预览主题复核：订阅与覆写共用 DocumentDrag，显式使用 popover、popover_foreground、border，避免独立拖动层继承默认黑字；公共 Dialog / AlertDialog 同样显式设置配对文字色。全 src 色值检查未发现主题模块外的界面 RGB/HSLA 常量；二维码的黑白编码色保留。深浅背景对比度回归、Clippy 通过；预览实际显示仍需原生窗口验收。

按用户最新决定，移除 NumberInput 及 Input 加自定义步进按钮的组合，直接使用 GPUI Kit Input；保留现有数值校验、自动保存和随机端口动作。
