# MiSans 渲染检查

日期：2026-11-07。全局界面及代码编辑区采用小米官网原始 MiSans TTF，内嵌 Regular、Medium、Semibold、Bold 四个字重；字体来源与许可见 `assets/fonts/README.md`。

## 已验证

- macOS CoreText 成功注册四个字重；中英文和数字实际使用 MiSans，旗帜与其他 Emoji 实际使用 Apple Color Emoji。
- 使用生产应用页面打开隔离的 GPUI 预览窗口，不启动内核，不接入用户设置与系统代理。
- 浅色主题下检查仪表盘、代理组工具栏与搜索输入，以及订阅新建 YAML 对话框。输入 `🇭🇰 香港 ABC 123 🚀` 后，文字、旗帜及 Emoji 均可见；未见明显文字裁切或错位。
- YAML 编辑器使用 MiSans；语法高亮、行号、输入与取消操作正常。预览草稿未保存。
- 环境诊断只输出 application、system、environment_variables，移除导出说明及 diagnostics、redaction 字段；实际导出异常进入日志。
- 构建、Clippy、诊断测试 5 项、主题测试 2 项通过。

## 未验证

Windows 与 Linux 实机、深色主题及最小窗口下的本轮视觉表现尚未验证。MiSans 不提供本次测试中的旗帜与 Emoji 字形，不能将 macOS 的显示结果视为 Windows 旗帜问题已解决。MiSans 为比例字体，代码编辑区不再使用等宽字体。
