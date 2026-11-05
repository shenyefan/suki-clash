# 服务安装与卸载整合（2026-11-05）

按用户决定，服务程序自身承接安装、修复、内核批准和卸载。正式包只包含两个应用可执行文件：`suki-clash`、`suki-clash-service`（Windows 加 `.exe`）；不再构建或交付独立的 install/uninstall 程序。

参考 `/Users/shenyefan/VSProjects/ClashSuki/ClashSuki.Service/Program.cs`、`PortableServiceInstaller.cs` 和 GUI 的 `PortableServiceInstallerLauncher.cs`：同一个服务程序先分流维护命令，再进入服务宿主。Suki 保持自己的三平台服务注册、归属校验和 IPC，不复制 Windows 专用服务体系。

## 执行链

| 操作 | 当前执行 |
| --- | --- |
| 服务启动 | 无参数运行既有 daemon；Windows 接 SCM，macOS/Linux 保留 launchd/systemd 注册与停止流程 |
| 界面安装／修复 | 应用调用随包服务程序 `--prepare-install`；准备端复制所需内核、计算摘要，再把同一个服务程序复制到临时目录请求提权 |
| 提权安装 | 临时服务程序 `--install`，进入平台安装模块。停止确认、修复互斥、暂存发布、管理员批准内核、就绪回读、结构化失败及防火墙警告契约继续保留 |
| 只更新批准内核 | 同一准备入口带 `--core-only`；提权参数为 `--install --install-core ... --sha256 ...`，不携带 `--install-service`，不重建服务注册 |
| 界面卸载 | 应用把随包服务程序复制到临时目录，提权执行 `--uninstall`；先停止并确认本应用服务／归属，再删除登记和所属文件。临时副本避免 Windows 执行映像锁及 macOS 受保护用户路径问题 |
| 帮助／版本 | `--help`、`--version` 在 daemon 日志、运行时和所有权锁之前返回 |
| 无效命令 | 明确报错；拼错安装／卸载参数不会落入 daemon 启动，也不会先创建系统服务目录 |

维护分流由 `platform-service/src/maintenance/mod.rs` 持有，平台实现分别放在同目录 `install.rs`、`uninstall.rs`，共享维护操作复用 `shared.rs`。`src/bin/service.rs` 是唯一服务入口。安装准备需要的客户端能力由 `standalone` 特性包含；仅依赖客户端的 GUI 不编入 daemon 维护模块。

主程序统一从自身可执行文件同目录取得服务程序；macOS `.app/Contents/MacOS` 和 Linux AppImage 包内资源定位继续使用实际二进制目录。服务目录与应用登录自启动仍是不同所有者，不把安装服务改成打开 GUI 自启动。

本次不改 IPC revision 9、服务名、数据格式或归属，不提供旧工具入口、旧格式兼容或迁移。原独立工具源码转为内部维护模块，未维护第二套执行逻辑；未使用的旧 NSIS 服务安装资源已删除，应用交付继续由 Velopack 负责。

## 构建与打包

- `packaging/build-service.sh OUTPUT_DIRECTORY [TARGET]` 只构建并复制 `suki-clash-service`。
- GitHub release workflow 只构建主程序及该服务程序；Velopack 暂存只选这两个应用可执行文件，其他构建工具不入包。
- Windows 服务图标资源只链接到唯一服务程序；macOS/Linux 的资源、许可证、离线内核和 Geo 清单保持原契约。
- 开发构建命令：`cargo build --locked --manifest-path platform-service/Cargo.toml --features standalone --bin suki-clash-service`。这是构建命令，不安装服务。

## 验证

- 应用：361 项通过、16 条件忽略。
- 服务 `standalone,test`（包含 client）：库 93 项通过、1 条件忽略；统一服务可执行文件 23 项通过，包含原安装／卸载回归与新增命令分流回归。
- 打包：6 项测试通过；真实暂存 Windows、macOS、Linux 文件树，外部图标工具与 linuxdeploy 使用测试替身，确认只需两个应用可执行文件、缺少服务时拒绝打包、其他构建产物不入包。
- 实际 macOS 服务二进制验证帮助、版本、错误安装／卸载／准备参数；未创建 daemon 工作文件或进入维护写入。Cargo metadata 不再声明独立安装／卸载目标。
- 应用和服务构建、严格 Clippy、格式与差异检查通过。

测试使用临时目录和 Memory／模拟内核；本地 socket 需要沙箱外监听权限。没有执行真实服务安装／修复／卸载，没有改变系统代理、TUN、路由、凭据或登录自启动。Windows/Linux 的本次原生构建、真实提权和系统服务操作仍待各平台验收，不能用 macOS 测试或打包暂存代替。
