# 桌面发行与更新

本项目使用自己的 GitHub Release 清单与下载流程，`self_update = 1.3.3` 安装已下载的应用产物。GPUI Kit 文档提供发行方式参考，不提供应用更新 SDK。

参考：[GPUI Kit 打包](https://gpui-kit.com/zh-CN/docs/packaging/)、[GPUI Kit 自动更新](https://gpui-kit.com/zh-CN/docs/auto-update/)、[self_update](https://docs.rs/self_update/1.3.3/self_update/)、[Inno Setup](https://jrsoftware.org/ishelp/)。

## 平台契约

| 平台 | 首次安装 | 应用内更新 |
| --- | --- | --- |
| Windows x64 / ARM64 安装版 | Inno Setup EXE，默认所有用户安装到 Program Files | 下载完整 Setup.exe，请求 UAC，等待原进程退出后覆盖安装，再启动应用 |
| Windows x64 / ARM64 普通版 | Portable.zip | 下载仅包含 suki-clash.exe 的 Update.zip；程序目录中的服务与目标发布不一致时一并下载，由 self_update 替换主程序及所需服务副本后重启 |
| macOS Apple Silicon | DMG，将应用拖到可写的应用目录 | 下载 Update.tar.gz，由 self_update 替换完整 Suki Clash.app 并重启 |
| Linux x64 | AppImage，赋予执行权限后启动 | 下载新版 AppImage，替换 APPIMAGE 指定的外层文件，恢复执行权限后重启 |

Windows 安装程序创建 `inno-install` 标记；普通版不包含标记。应用只用该标记选择 Windows 更新方式。普通版程序目录及 AppImage 所在目录必须可写；macOS 应用目录的父目录必须允许 bundle 替换。从只读 DMG 或 App Translocation 运行时需要先把应用移到正常目录。

开发构建没有 `SUKI_UPDATE_REPOSITORY` 时仅提供手动发布页面，不访问自动更新源。正式构建由 CI 注入 `owner/repo`。

## 自动打包与发布

`.github/workflows/release.yml` 的 prepare/build/release 三阶段保留原有四个目标：win-x64、win-arm64、osx-arm64、linux-x64。标签必须等于根 Cargo.toml 的版本；发布任务只创建或更新草稿，不修改已公开的发布。

1. `packaging/runtime.sh` 下载并验证首次运行所需的 Mihomo 与 Geo 数据。
2. 在原生 runner 构建主程序和带维护入口的独立 service。Windows 使用静态 CRT。
3. Windows 从官方 Release 下载 Inno Setup 7.1.1 x64，验证固定 SHA-256，并以 `/CURRENTUSER /PORTABLE=1` 准备到 runner 临时目录；不依赖 runner 预装编译器。
4. `packaging/stage.py` 组装目录。Linux 使用固定版本和摘要的 linuxdeploy 收集共享库，并通过内置 appimage 输出插件生成 AppImage。
5. `packaging/pack.py` 调用 ISCC、hdiutil 或复制 AppImage，生成首次安装产物、更新产物、独立服务产物及平台清单。
6. `packaging/release.sh` 校验清单引用的产物存在，收集所有平台产物并生成 SHA256SUMS，上传 GitHub Release 草稿。

打包入口：

```text
python packaging/stage.py TARGET VERSION RUNTIME_DIR STAGE_DIR
python packaging/pack.py VERSION TARGET CHANNEL STAGE_DIR OUTPUT_DIR
```

Windows 本地打包需要通过 `ISCC_PATH` 指向 ISCC.exe。安装脚本为 `packaging/windows/setup.iss`，提供安装目录、开始菜单、可选桌面快捷方式、suki-clash 协议注册和卸载入口。更新通过 `/SUKIWAITPID` 等待当前应用结束，应用内先完成内核及系统代理退出准备；安装失败不等于更新成功。安装和覆盖升级在文件落盘后，以安装程序已有的管理员权限执行 `suki-clash-service.exe --install --install-service`，复用维护程序的停止、系统副本替换、注册、启动及就绪检查。卸载在删除应用文件前执行 `--uninstall`。服务安装失败时禁止自动启动应用，完成页显示需修复并返回非零退出码；服务移除失败时中止卸载。维护输出记录到安装／卸载日志；防火墙警告退出码 2 单独提示，不误判为服务安装失败。

## 更新清单与暂存

每个平台发布 `releases.CHANNEL.json`，当前格式为：

```json
{
  "version": "0.7.0",
  "channel": "win-x64",
  "portable": {"file_name": "SukiClash-0.7.0-win-x64-Update.zip", "size": 123, "sha256": "64 hexadecimal characters"},
  "installer": {"file_name": "SukiClash-0.7.0-win-x64-Setup.exe", "size": 456, "sha256": "64 hexadecimal characters"},
  "service_version": "0.1.1",
  "service": {"file_name": "SukiClash-0.7.0-win-x64-Service.exe", "size": 789, "sha256": "64 hexadecimal characters"}
}
```

非 Windows 的 installer 为 null，macOS portable 为 `SukiClash-VERSION-osx-arm64-Update.tar.gz`，Linux portable 为 `SukiClash-VERSION-linux-x64.AppImage`；非 Windows 服务产物无 `.exe` 后缀。

更新入口集中在 `src/updates.rs`。边界校验版本、平台、固定文件名、大小及 SHA-256，不接受任意下载地址。下载 URL 由编译时仓库、发布版本和产物名构造，使用共享 reqwest、系统网络策略及 GitHub 下载代理。下载先写临时文件，完整校验后才提交到 `cache/app-update/`。安装前再次验证暂存包摘要并核对用户确认的版本。

应用下载与服务下载沿用生命周期执行端的进度通知和退出取消。self_update 的自定义传输只读取已验证的本地文件，不建立第二条网络下载路径。变更记录继续分页读取 GitHub compare 接口。

当前产物未签名、未公证。SHA-256 检查用于发现损坏和清单不一致，不构成独立的发布者签名验证。

## 服务更新

系统服务运行的是提权安装到系统目录的副本。主程序目录中的 `suki-clash-service.exe` 只是服务维护来源，覆盖它不会替换正在运行的系统服务。

Windows 普通版检查及应用主体更新时：

1. 检查新版应用的清单，并将程序目录中的服务大小、SHA-256 与**目标应用发布**的服务产物比较；缺失或内容不一致时，将服务加入本次下载计划。服务版本号未变但构建内容改变时同样更新。
2. 用户下载应用更新时，按计划同时下载主程序更新包及独立 Service.exe，进度按两份产物的总大小统计。
3. 两份产物均完成校验后才提交下载完成状态。安装前再次校验所需产物；服务缺失或损坏时不开始替换主程序。
4. 应用更新经过已有退出清理后，替换主程序和程序目录中的服务副本，再重启。服务文件替换失败保留原服务文件，并明确报告主程序已替换的事实。
5. 此流程不调用系统服务的安装、停止或更新命令。实际系统服务的安装、更新、修复和卸载由虚拟网卡页面控制，使用程序目录中的维护程序，不再另行触发服务下载。

Windows 安装版由完整安装程序携带新版维护来源，并在安装和覆盖升级时更新系统服务，卸载时移除系统服务；这项操作由 Inno 执行，主程序不另外发起服务提权。macOS 和 Linux 随完整 app / AppImage 携带维护来源。Windows 普通版、macOS 和 Linux 的应用更新不更新系统目录中的服务副本，需要时仍通过虚拟网卡页面请求权限。

既有维护流程会停止、替换、启动并验证服务；新服务启动失败时并未实现旧系统服务文件的完整自动回滚，不能把应用内核恢复尝试描述成系统服务回滚成功。

首次运行资源只在用户目录缺失对应资源时复制；已有用户资源不随普通版主程序更新被覆盖。资源初始化完毕后可移除普通版旁边的首次运行资源目录。

## CV 参考核对

本地 `clash-verge-rev-dev` 使用 NSIS：`src-tauri/tauri.windows.conf.json` 指定 nsis target、perMachine 及自定义 installer.nsi；发布工作流调用 tauri-action；`scripts/updater.mjs` 收集 setup.exe 与 .sig 写更新清单；Tauri updater 负责安装交接。其安装脚本还负责服务操作。Suki Clash 仅参考执行顺序，采用自己的 Inno Setup、清单和现有受限提权服务边界。

## 验证范围

2026-11-13：Windows 主程序 cargo check、严格 Clippy、格式检查、9 项更新测试及 2 项退出清理测试通过，更新测试包含隔离目录中的实际 self_update ZIP 替换、服务产物判断、服务缺失／损坏时阻止应用更新及程序目录中的服务替换；Python 与发布 shell 语法检查通过。Inno Setup 7.1.1 已对包含服务维护事件的 x64 和 ARM64 配置实际编译通过，输入为隔离打包测试文件。未安装或运行测试安装包，未执行本机真实服务更新。macOS DMG、完整 bundle 更新与 Linux AppImage 打包、替换仍需相应原生平台验证。
