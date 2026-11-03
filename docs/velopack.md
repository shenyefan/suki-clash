# Velopack 交付与更新

本项目首次接入固定使用 Rust SDK／`vpk` **1.2.161**。Velopack处理应用分发和更新；GPUI Kit仍负责应用界面，Mihomo与独立平台服务保持现有所有权。参考依据为[官方Rust接入](https://docs.velopack.io/getting-started/rust)、[官方打包参数](https://docs.velopack.io/packaging/overview)、[GitHub发布](https://docs.velopack.io/distributing/github-actions)及[1.2.161源码](https://github.com/velopack/velopack/tree/1.2.161)，不使用仅新版本提供的参数。

## GitHub自动构建

`.github/workflows/release.yml`提供手动试构建和`v*`标签构建。标签必须严格等于根Cargo.toml版本的`v{version}`。初版只允许稳定版本，拒绝预发布／build metadata，避免beta进入默认stable源。普通分支推送不自动发布。

六个原生runner分别构建主程序及三服务工具，使用Cargo.lock；不把整个target目录加入包。矩阵全部成功后才汇总，产物不会由并行job抢写同一Release。手动运行只保留Actions artifacts；标签运行创建／更新**一个草稿Release**，不会自动公开发布，不修改已发布同名版本。用户检查草稿和签名状态后再发布。

| 平台／架构 | 更新channel | 用户下载文件 |
| --- | --- | --- |
| Windows x64／ARM64 | win-x64／win-arm64 | SukiClash-{channel}-Setup.exe、SukiClash-{channel}-Portable.zip |
| macOS Intel／Apple Silicon | osx-x64／osx-arm64 | SukiClash-{channel}-Setup.pkg；同时保留Velopack生成的Portable.zip |
| Linux x64／ARM64 | linux-x64／linux-arm64 | SukiClash-{channel}.AppImage |

每个channel还有独立`releases.{channel}.json`、`assets.{channel}.json`和`SukiClash-{version}-{channel}-full.nupkg`；不得只上传安装包而漏更新源。初版禁用delta，不依赖历史版本下载。SDK的PackageId固定`SukiClash`。Linux原生不生成deb／rpm，需要时另接发行版打包工具，不能把AppImage称为deb安装器。各架构文件互不覆盖。

Windows便携版是**免安装程序包**，用户配置仍沿当前平台应用目录保存，不宣称数据与程序同目录。Windows构建采用静态CRT，避免只在安装版补VC运行库而让便携版漏依赖。便携版使用TUN仍遵守现有服务安装／权限要求。

## 文件结构与资源

`packaging/velopack.py`要求传入已构建主程序目录、服务目录、目标及新的工作目录；`--prepare-only`只检查和暂存。四个可执行文件全部必需：suki-clash、suki-clash-service、suki-clash-service-install、suki-clash-service-uninstall；Windows使用.exe。macOS放入Contents/MacOS，资源在Contents/Resources；其他平台资源相对实际可执行文件为resources/。许可证和应用logo随包；macOS保留正式bundle ID、URL协议和版本元数据。

默认是**轻量包，不内置Mihomo和Geo**。缺内核检测与内核页面下载保持现有行为，不在CI临时从latest下载未固定资源。需要离线包时，可在调用脚本时显式传`--runtime-dir`，目录必须包含现有格式manifest和完整五项资源，目标、普通文件、长度及SHA-256都须匹配；没有兼容或回退格式。

Linux使用成熟linuxdeploy打包动态依赖，固定`1-alpha-20251107-1`及各架构SHA-256，再由vpk生成AppImage；不自己扫描／复制依赖协议。AppImage的可用性仍受目标glibc、Vulkan／图形驱动、桌面环境、服务授权工具和发行版限制，不能仅靠生成文件宣称所有Linux可运行。

## 签名与草稿状态

没有签名secrets时，工作流允许生成**未签名草稿候选**，`build.{channel}.json`明确`signed:false`，不会伪装成已签名／已公证发行版；人工发布前须完成正式签名和平台验收。

Windows签名使用：`SUKI_WINDOWS_CERTIFICATE_P12`（base64）、`SUKI_WINDOWS_CERTIFICATE_PASSWORD`。临时导入当前runner证书库，用SHA1身份让vpk签程序及生成文件；签名密码不放CLI参数，任务末尾删除临时材料／导入私钥。

macOS签名需要完整的一组secrets：`SUKI_MACOS_CERTIFICATE_P12`（包含Developer ID Application和Developer ID Installer的base64 p12）、`SUKI_MACOS_CERTIFICATE_PASSWORD`、`SUKI_MACOS_APPLICATION_IDENTITY`、`SUKI_MACOS_INSTALLER_IDENTITY`、`SUKI_APPLE_API_KEY_P8`（原始p8文本）、`SUKI_APPLE_API_KEY_ID`、`SUKI_APPLE_API_ISSUER`。CI创建临时keychain，给vpk指定该keychain和notary profile。使用空的hardened-runtime entitlements，不为GPUI／QuickJS额外开启JIT或关闭库验证。材料仅在runner临时目录，不进入源码或产物。配置不完整直接失败，不退回未签名成功。

GitHub上传权限只给最终草稿job；不需要在仓库写PAT。更新源仓库通过构建环境`SUKI_UPDATE_REPOSITORY=${{ github.repository }}`写入程序；本地开发没有该值时不得猜GitHub地址或伪造可更新状态。更新路径为当前仓库最新公开stable release和各channel完整包，草稿不会成为普通用户的更新源。

## 当前验证边界

本地验证了Python语法、workflow YAML解析、plist与暂存结构；实际安装`vpk 1.2.161`检查CLI参数，并用隔离占位Mach-O二进制生成macOS pkg／Portable.zip／full.nupkg／channel feed，核对PackageId、版本、文件名、摘要和大小。该冒烟验证未运行／安装产物，也不是实际Suki发布验证。

尚需在GitHub执行完整六平台构建，并验收真实签名／公证、安装／卸载、便携版、AppImage依赖、更新前系统代理与内核清理、服务协议升级后的显式修复及更新失败恢复。本机没有上传Release或操作真实系统服务。


## 应用内执行与开发运行

Velopack启动钩子在单实例和GPUI之前执行，明确关闭启动自动应用待安装包。设置→维护→检查应用更新通过生命周期命令执行：先确认当前安装身份，再由现有reqwest共享客户端取得当前平台／架构的公开stable清单和完整包；检查PackageId、固定文件名、大小与SHA-256，流式下载到临时文件，全部通过才发布。网络取消只取消当前下载，不自动重跑；SDK仅从已验证本地文件源准备包，不另走ureq联网或差分回退。

下载完成后用户确认安装并重启。先核验本地待安装版本，再通过现有内核／系统代理清理与存储队列屏障；清理失败不调用更新器。成功后调用SDK的等待当前进程退出接口，不使用会直接process::exit的快捷API。更新器启动失败保留主界面和执行端供重试，不把更新提交事实伪装为成功。平台服务仍为独立受限服务，新应用协议不符时使用既有显式修复入口，不能把主程序更新等同服务升级。

非Velopack安装包点击更新时直接打开GitHub最新发布页，开发环境默认官方仓库；正式构建按注入仓库打开。不弹“请安装正式分发包”错误，不把开发运行假装成自动更新成功。真正的包清单、网络、权限或校验失败仍报告原原因，不自动打开链接掩盖失败。

macOS只有NSBundle具有bundleIdentifier时才执行订阅协议注册，cargo run不进行不适用注册。真实索引旧格式错误会指出具体文件；不会加入自动迁移。2026-11-03经用户单独授权，一次性备份并移除两份索引的3个冗余正文键，独立正文及其他字段一致性已核验；该操作没有放进应用启动代码。

本轮自动检查：根项目346项通过、16项条件忽略；严格Clippy与格式／差异检查通过。包含包身份／路径／大小／摘要、损坏下载不覆盖已暂存文件、开发运行手动更新、安装确认取消、索引错误不改正文等回归。实际GitHub六平台构建、正式签名／公证、真实应用更新及helper升级尚未执行；macOS隔离占位打包成功不等于正式应用安装更新验收通过。

补充实际打包：使用本轮真实debug主程序及三个真实服务工具，已生成未签名／未公证macOS ARM64 pkg、Portable.zip及full.nupkg（/tmp/suki-velopack-real-macos-retry-20261103/Releases）。四程序存在、正式bundle ID、feed大小／SHA-256及signed:false已核验。未运行或安装这些产物；这仍不是release优化构建、签名或端到端更新验收。
