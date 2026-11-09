# macOS 本地检查与跨平台打包

入口为 `packaging/local.py`。支持发布流程现有的 Windows x64、Windows ARM64 和 Linux x64，不触发 GitHub 工作流或发布。

```sh
# 检查全部目标的主程序和服务
python3 packaging/local.py check --target all

# 完整编译及链接，输出可执行文件
python3 packaging/local.py build --target win-x64

# 编译、准备经过校验的离线内核和 Geo 数据，并生成安装包
python3 packaging/local.py package --target all
```

单独构建使用 `--target win-x64`、`--target win-arm64` 或 `--target linux-x64`。默认并发为 2，可用 `--jobs` 调整。`check` 验证编译和类型；需要验证最终链接与安装包时使用 `package`。

## 工具链与环境

macOS 需要 Python 3.11 以上、rustup、Homebrew LLVM、CMake、Ninja、NASM、cargo-xwin、.NET SDK，以及运行中的 OrbStack／Docker。原生依赖可用以下命令准备：

```sh
brew install llvm cmake ninja nasm
~/.cargo/bin/cargo install --locked cargo-xwin
rustup component add llvm-tools --toolchain stable
```

脚本通过 rustup 找到所选 `rustc`，同时设置 Cargo、rustc、rustdoc 和原生 LLVM 的路径；`--toolchain` 默认选择 stable。容器按选中的 Rust 版本创建，当前验证版本为 1.99.1。Windows 标准库由 rustup 安装，Microsoft SDK／CRT 由 cargo-xwin 管理。

本机原先的 `/opt/homebrew/bin/cargo`、`rustc` 为 Homebrew Rust 1.99.0，目标标准库却安装在 rustup Rust 1.99.1 中。`rustup run stable cargo` 本身不能修正 Cargo 子进程的 PATH，因此此前仍报 E0463。本地入口明确选择同一工具链；本次还在 `~/.zprofile` 的 Homebrew 初始化后加入 rustup PATH 优先级，新登录终端已验证使用 `~/.cargo/bin`。已打开的 IDE 若显式指定 Homebrew 工具链，需要在 IDE 中选择 rustup。

## 各平台构建方式

| 目标 | 编译与打包 |
| --- | --- |
| Windows x64 | macOS 上使用 cargo-xwin、MSVC SDK／CRT、Clang 和 LLD，保留发布流程的静态 CRT 配置；Velopack 使用明确的 `[win]` 指令生成安装包和免安装包。 |
| Windows ARM64 | 同上，使用 ARM64 标准库和库文件；ring 的 ARM64 汇编器改用 GNU clang 时，由专用启动脚本将 `/imsvc` 转成 `-isystem`，保留参数及路径边界，编译失败原样传播。 |
| Linux x64 | 在 Ubuntu 22.04 x64 容器内使用 GNU 工具链编译；Apple Silicon 通过 OrbStack 的 x64 容器支持运行。Python 3.12、.NET 8、原生库、linuxdeploy 和 Velopack 均在容器中执行，生成 AppImage。 |

### Windows release 着色器

当前锁定的 `gpui-pre-windows 0.3.11` 只在 Windows **构建主机**上生成 release 的 `shaders_bytes.rs`。直接从 macOS 交叉构建会漏掉该文件；仅安装目标标准库和 Windows SDK 不足以完成 release 构建。

本地工具为此使用单独的 Wine 构建镜像。镜像从 Microsoft 官方 Windows SDK 地址取得 D3DCompiler 47，固定并核验 CAB 的 SHA256。小型 Winelib 编译程序调用 Microsoft `D3DCompileFromFile`，按 GPUI 原有的 `vs_4_1`／`ps_4_1` 和 O3 参数编译当前依赖中的 HLSL，包含标准 include 文件。Rust 编译包装器仅为 `gpui-pre-windows` 的目标库补齐生成文件，再继续原始 rustc 调用。

本次实际编译了全部 18 个着色器并核验 DXBC 文件头。流程没有修改依赖源码、生成空着色器、开启 debug assertions 或把着色器编译挪到用户程序启动时。编译容器的源码只读、网络关闭；Wine 与 Microsoft 编译 DLL 只参与构建，不进入应用安装包。原生 Windows CI 继续使用上游自己的 shader 构建脚本。

连续重建时曾出现 Wine 客户端与服务连接重置；编译批次现在显式保持 wineserver 存活，由临时容器结束其生命周期。修正后连续两轮各 18 个着色器及 Rust 绑定校验通过，随后两种 Windows 架构完成完整构建。任何编译失败都会停止打包，不自动重试或继续使用旧生成文件。

参考：[cargo-xwin](https://github.com/rust-cross/cargo-xwin)、[Velopack 跨平台指令](https://docs.velopack.io/packaging/cross-compiling)、[Winetricks 中 Microsoft 编译 DLL 的官方来源](https://github.com/Winetricks/winetricks/blob/master/src/winetricks)。

## 输出与缓存

- 可执行文件：`target/cross/<目标>/bin/`。
- 安装包：`target/cross/<目标>/packages/<构建时间>/Releases/`；每次使用独立目录。
- Windows SDK／CRT：`target/cross/xwin/`；Windows Rust 编译缓存放在各目标的 `cargo/` 下。
- 离线运行时及下载身份快照：各目标下的 `runtime/` 和 `runtime-lock.json`。缓存提交前完成 SHA256、大小及格式校验，打包时再次校验。
- Linux Cargo 缓存使用 `suki-clash-linux-build`、`suki-clash-linux-registry`、`suki-clash-linux-git` 三个 Docker 卷；源码只读挂载。
- Linux 工具镜像按 Rust 版本复用；Windows shader 工具镜像还绑定构建脚本和编译程序内容摘要。需要更新原生库或重建环境时加 `--rebuild-tools`。

Velopack 固定为 1.2.161。macOS 上的 CLI 进程允许使用现有较新 .NET 运行时，Linux 镜像使用 .NET 8。没有提供签名配置时，生成的包标记为未签名。

## 本次实际发现并修复的问题（2026-11-06）

1. Homebrew 与 rustup 混用，导致已经安装的目标标准库仍不可见；同时补齐缺失的 Rust LLVM 工具组件，验证 rust-objcopy 可正常启动。
2. Windows 应用／服务图标资源使用相对路径，LLVM 资源编译器切换工作目录后找不到 ICO；现通过绝对路径宏传入同一图标。
3. Windows release 跨主机构建漏生成 GPUI 着色器；现通过真实 Microsoft 编译器完成预编译。
4. WinINET 与 KDE 固定字段先转 Vec、再转数组，并试图把 Vec 当作 Display 错误记录，导致两平台编译失败；现直接填充固定数组，删除无意义的数量检查。
5. Windows 自启动分支返回后仍保留公共成功返回，出现不可达代码告警；现各平台分支返回自己的结果。
6. Linux 最终打包缺少 mksquashfs；本地镜像和发布工作流共同补齐 `squashfs-tools`，并补 `desktop-file-utils`。

本地编译、链接、包结构校验与真实桌面验收分别记录。生成安装包不能代替 Windows／Linux 实机的窗口、GPU、托盘、系统代理、TUN、服务和退出恢复验证。

## 验证记录

2026-11-06，使用 0.2.5 源码，在本机完成三组主程序／服务 release 编译及最终打包：

| 目标 | 产物 |
| --- | --- |
| Windows x64 | `SukiClash-win-x64-Setup.exe`、`SukiClash-win-x64-Portable.zip` |
| Windows ARM64 | `SukiClash-win-arm64-Setup.exe`、`SukiClash-win-arm64-Portable.zip` |
| Linux x64 | `SukiClash-linux-x64.AppImage` |

本次提交将应用版本提升至 0.2.6，服务协议版本仍为 2.7.6；上述已验证产物为 0.2.5，版本递增后未重新生成安装包。

两种 Windows Portable ZIP 均核对程序／服务 SHA256 与构建输出一致，包含完整离线运行时；主程序与服务 PE 架构和图标资源已检查，主程序为 GUI 子系统。Linux AppImage 已实际解包，核对其中程序／服务与 linuxdeploy 处理后的 stage 一致，并重新校验全部运行时资产；提取的服务 `--help` 可正常运行，主程序要求的 glibc 最高版本为 2.35。

Wine 6 只用于运行上述 shader 构建工具。直接运行本次 Windows 服务时，该模拟环境缺少 `bcryptprimitives.dll` 和 `api-ms-win-net-isolation-l1-1-0.dll`；不把 Wine 当作 Windows 原生运行验收。Windows 导入表未依赖外部 MSVC C/C++ 运行时 DLL。发布工作流还显式取得 `GPUI_FXC_PATH`，原生构建在 MSVC 初始化阶段确认着色器编译器。

Python 打包回归 12 项通过，包含工具链路径优先级、编译器参数及退出状态、shader 失败阻止后续编译、ARM64 include 参数转换和跨平台 Velopack 指令。格式、差异空白和工作流 YAML 语法检查通过；Windows x64 与 Linux x64 的主程序／独立服务生产目标严格 Clippy 通过，Linux 全测试目标 Clippy 通过。Windows 的完整测试目标另有测试可达性产生的 `dead_code` 告警，严格测试目标 lint 未通过，原始诊断保留在 `target/cross/windows-clippy-all-targets.log`。Windows 原生测试执行仍待完成。
