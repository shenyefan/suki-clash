# 旗帜渲染检查

2026-11-07：移除全部 MiSans 文件和字体覆盖。

## 方案

全部平台在启动时注册 `assets/fonts/flags.ttf`，主题的 `font_family` 与 `mono_font_family` 均为 Suki Flag Emoji。该字体由 Mozilla 官方 Twemoji COLR v0.7.0 裁剪，只含旗帜相关码位，因此旗帜由它绘制，中英文、数字、符号和其他 Emoji 由平台字体自动回退。应用不识别或拆分旗帜字符，原始名称、复制、搜索与内核请求不改写。

原始字体的彩色基础字形为空，GPUI 以基础字形计算绘制范围，Windows 实测会空白；制作时仅补充由原彩色图层组成的复合轮廓，不更改彩色图层，不修改 GPUI。来源、许可与重建命令见 `assets/fonts/README.md`。

## 实测

Windows 11 Parallels 虚拟机，x64 GPUI 检查程序，裁剪字体设为首选字体：香港、日本、美国、新加坡、德国、英国、台湾、中国旗帜显示彩色图形；中英文、数字、`#*`、火箭、笑脸、`© ™`、输入框和 Select 正常回退。编辑器回退、macOS、Linux 与 Windows ARM64 尚未在正式应用中实机检查。

## UA

默认 UA 为 `clash-verge/v2.6.0 Suki-Clash/<应用版本>`。CV 的 2.6.0 来自本地版本记录 `docs/cv-upstream-sync.md` 和参考项目 `package.json`，不随 Suki 版本变化。单订阅及全局自定义 UA 保留用户设置。
