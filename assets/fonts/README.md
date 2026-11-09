# 旗帜字体

Suki Flag Emoji 由 Twemoji Mozilla v0.7.0 裁剪而成，只保留国家、地区区域指示字符和英格兰、苏格兰、威尔士标签序列，以及对应的组合规则和 COLR/CPAL 彩色图层。全部平台把它设为正文和编辑器的首选字体；字体中没有的字符由平台字体自动回退。仅注册到应用进程，不安装系统字体。

- 来源：https://github.com/mozilla/twemoji-colr/releases/tag/v0.7.0
- 原始文件：https://github.com/mozilla/twemoji-colr/releases/download/v0.7.0/Twemoji.Mozilla.ttf
- 获取日期：2026-11-07。保留码位为 `U+1F1E6..U+1F1FF`、`U+1F3F4`、`U+E0061..U+E007A`、`U+E007F`；为基础字形补充由同一彩色图层组成的复合轮廓，以提供 GPUI/DirectWrite 所需的非空绘制范围。
- 制作工具：Mozilla Foundation；图形设计：Twitter Twemoji。
- 许可：[代码 Apache 2.0 与图形 CC BY 4.0](Twemoji-LICENSE.md)。

重建：`python3 packaging/flag_font.py /path/to/Twemoji.Mozilla.ttf assets/fonts/flags.ttf`，需要 `fonttools==4.68.0`。

2026-11-07 检查：Windows 11 Parallels x64 GPUI 检查程序中，香港、日本、美国、新加坡、德国、英国、台湾、中国旗帜显示彩色图形，中英文、数字、其他 Emoji、Input 和 Select 正常回退。编辑器回退、macOS、Linux 与 Windows ARM64 尚未在正式应用中实机检查。原始名称、复制、搜索及内核请求不改写。
