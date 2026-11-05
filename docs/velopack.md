# Velopack 交付与更新

本项目首次接入固定使用 Rust SDK／`vpk` **1.2.161**。Velopack处理应用分发和更新；GPUI Kit仍负责应用界面，Mihomo与独立平台服务保持现有所有权。参考依据为[官方Rust接入](https://docs.velopack.io/getting-started/rust)、[官方打包参数](https://docs.velopack.io/packaging/overview)、[GitHub发布](https://docs.velopack.io/distributing/github-actions)及[1.2.161源码](https://github.com/velopack/velopack/tree/1.2.161)，不使用仅新版本提供的参数。

## GitHub自动构建

`.github/workflows/release.yml`提供手动试构建和`v*`标签构建。标签必须严格等于根Cargo.toml版本的`v{version}`。初版只允许稳定版本，拒绝预发布／build metadata，避免beta进入默认stable源。普通分支推送不自动发布。

四个原生runner分别构建主程序及集成维护的服务程序，使用Cargo.lock；不把整个target目录加入包。矩阵全部成功后才汇总，产物不会由并行job抢写同一Release。手动运行只保留Actions artifacts；标签运行创建／更新**一个草稿Release**，不会自动公开发布，不修改已发布同名版本。用户检查草稿和签名状态后再发布。

| 平台／架构 | 更新channel | 用户下载文件 |
| --- | --- | --- |
| Windows x64／ARM64 | win-x64／win-arm64 | SukiClash-{channel}-Setup.exe、SukiClash-{channel}-Portable.zip |
| macOS Apple Silicon | osx-arm64 | SukiClash-{channel}-Setup.pkg；同时保留Velopack生成的Portable.zip |
| Linux x64 | linux-x64 | SukiClash-{channel}.AppImage |

Release附件严格限定为上表的7个安装器／压缩包，不上传JSON、nupkg或其他内部文件。每个channel的`releases.{channel}.json`、`assets.{channel}.json`、`build.{channel}.json`和完整更新包仅保留在Actions artifacts中14天。禁用delta，不依赖历史版本下载。SDK的PackageId固定`SukiClash`。Linux原生不生成deb／rpm，需要时另接发行版打包工具，不能把AppImage称为deb安装器。各架构文件互不覆盖。

Windows便携版是**免安装程序包**，用户配置仍沿当前平台应用目录保存，不宣称数据与程序同目录。Windows构建采用静态CRT，避免只在安装版补VC运行库而让便携版漏依赖。便携版使用TUN仍遵守现有服务安装／权限要求。

## 文件结构与资源

`packaging/velopack.py`要求传入已构建主程序目录、服务目录、目标及新的工作目录；`--prepare-only`只检查和暂存。仅两个可执行文件必需：`suki-clash`、`suki-clash-service`；Windows 使用 `.exe`。安装／卸载由服务程序自身承接，构建目录内其他工具不会随包。macOS放入Contents/MacOS，资源在Contents/Resources；其他平台资源相对实际可执行文件为resources/。许可证和应用logo随包；macOS保留正式bundle ID、URL协议和版本元数据。

所有正式包必须内置 **Mihomo、Country.mmdb、ASN.mmdb、geoip.dat、geosite.dat**，`--runtime-dir`为必填。CI先由`packaging/runtime.py --resolve runtime-lock.json`读取官方发行元数据，记录当轮四架构内核及四份Geo的URL、长度和上游SHA-256，作为共享artifact。每个runner使用同一份快照下载，校验后解压内核、生成应用现有格式的manifest，再交给打包脚本复核。资源缺失、损坏或上游latest文件在构建期间发生变化均导致失败，不生成缺资源的包。内核采用官方稳定版，Geo采用应用默认来源；快照随Actions artifacts保留14天，重现当轮资源时使用该快照，不重新解析latest。

本地准备：`python packaging/runtime.py --resolve /tmp/runtime-lock.json`，然后执行`python packaging/runtime.py --lock /tmp/runtime-lock.json --target aarch64-apple-darwin --output /tmp/suki-runtime`；向打包脚本传入`--runtime-dir /tmp/suki-runtime`。首次启动复用现有bootstrap校验和安装，已有用户内核和数据库不被覆盖。源码开发运行仍允许没有随包资源。

工作流的checkout、Python、.NET和artifact Actions已切换到原生Node.js 24版本；MSVC使用Visual Studio自带Developer PowerShell，移除仍依赖Node.js 20的工具链Action。Linux密钥库oo7禁用默认Tokio后端，显式使用与GPUI门户相同的async-io后端，避免ashpd互斥feature冲突。

GitHub下载代理由共享HTTP规则处理，覆盖内核版本／归档、Geo手动下载、公开订阅／覆写／图标，以及应用更新清单和完整包；携带Authorization的请求不转交第三方代理。内核自身按geox-url发起的后台更新由Mihomo负责，不经过应用HTTP规则。CI从官方GitHub获取资源，不读取用户下载代理设置。

Linux使用成熟linuxdeploy打包动态依赖，固定`1-alpha-20251107-1`及各架构SHA-256，再由vpk生成AppImage；不自己扫描／复制依赖协议。AppImage的可用性仍受目标glibc、Vulkan／图形驱动、桌面环境、服务授权工具和发行版限制，不能仅靠生成文件宣称所有Linux可运行。

## 签名与草稿状态

没有签名secrets时，工作流允许生成**未签名草稿候选**，Actions artifact中的`build.{channel}.json`明确`signed:false`，Release描述按实际构建记录展示签名／公证状态，不会伪装成已签名／已公证发行版；人工发布前须完成正式签名和平台验收。

Windows签名使用：`SUKI_WINDOWS_CERTIFICATE_P12`（base64）、`SUKI_WINDOWS_CERTIFICATE_PASSWORD`。临时导入当前runner证书库，用SHA1身份让vpk签程序及生成文件；签名密码不放CLI参数，任务末尾删除临时材料／导入私钥。

macOS签名需要完整的一组secrets：`SUKI_MACOS_CERTIFICATE_P12`（包含Developer ID Application和Developer ID Installer的base64 p12）、`SUKI_MACOS_CERTIFICATE_PASSWORD`、`SUKI_MACOS_APPLICATION_IDENTITY`、`SUKI_MACOS_INSTALLER_IDENTITY`、`SUKI_APPLE_API_KEY_P8`（原始p8文本）、`SUKI_APPLE_API_KEY_ID`、`SUKI_APPLE_API_ISSUER`。CI创建临时keychain，给vpk指定该keychain和notary profile。使用空的hardened-runtime entitlements，不为GPUI／QuickJS额外开启JIT或关闭库验证。材料仅在runner临时目录，不进入源码或产物。配置不完整直接失败，不退回未签名成功。

GitHub上传权限只给最终草稿job；不需要在仓库写PAT。更新源仓库通过构建环境`SUKI_UPDATE_REPOSITORY=${{ github.repository }}`写入程序；本地开发没有该值时不得猜GitHub地址或伪造可更新状态。Release描述包含平台／文件／签名状态表、必要使用说明，以及GitHub自动生成的版本变更记录；重跑时同步更新草稿说明，并删除草稿中不属于当前7个下载文件的旧附件。已公开的同名Release仍拒绝修改。

当前分发策略不公开更新清单和nupkg，因此新Release仅支持手动下载安装／解压升级。现有已安装版自动更新代码仍请求公开stable Release的channel JSON；对这些新Release检查会报清单获取错误，不能宣称支持自动更新。后续若恢复自动更新，需要单独确定更新源方案。草稿不会成为普通用户的更新源。

## 当前验证边界

本地验证了Python语法、workflow YAML解析、plist与暂存结构；实际安装`vpk 1.2.161`检查CLI参数，并用隔离占位Mach-O二进制生成macOS pkg／Portable.zip／full.nupkg／channel feed，核对PackageId、版本、文件名、摘要和大小。该冒烟验证未运行／安装产物，也不是实际Suki发布验证。

尚需在GitHub执行当前四平台构建，并验收真实签名／公证、安装／卸载、便携版、AppImage依赖、更新前系统代理与内核清理、服务协议升级后的显式修复及更新失败恢复。本机没有上传Release或操作真实系统服务。


## 应用内执行与开发运行

Velopack启动钩子在单实例和GPUI之前执行，明确关闭启动自动应用待安装包。设置→维护→检查应用更新通过生命周期命令执行：先确认当前安装身份，再由现有reqwest共享客户端取得当前平台／架构的公开stable清单和完整包；检查PackageId、固定文件名、大小与SHA-256，流式下载到临时文件，全部通过才发布。网络取消只取消当前下载，不自动重跑；SDK仅从已验证本地文件源准备包，不另走ureq联网或差分回退。

下载完成后用户确认安装并重启。先核验本地待安装版本，再通过现有内核／系统代理清理与存储队列屏障；清理失败不调用更新器。成功后调用SDK的等待当前进程退出接口，不使用会直接process::exit的快捷API。更新器启动失败保留主界面和执行端供重试，不把更新提交事实伪装为成功。平台服务仍为独立受限服务，新应用协议不符时使用既有显式修复入口，不能把主程序更新等同服务升级。

非Velopack安装包点击更新时直接打开GitHub最新发布页，开发环境默认官方仓库；正式构建按注入仓库打开。不弹“请安装正式分发包”错误，不把开发运行假装成自动更新成功。真正的包清单、网络、权限或校验失败仍报告原原因，不自动打开链接掩盖失败。

macOS只有NSBundle具有bundleIdentifier时才执行订阅协议注册，cargo run不进行不适用注册。真实索引旧格式错误会指出具体文件；不会加入自动迁移。2026-11-03经用户单独授权，一次性备份并移除两份索引的3个冗余正文键，独立正文及其他字段一致性已核验；该操作没有放进应用启动代码。

本轮自动检查：根项目346项通过、16项条件忽略；严格Clippy与格式／差异检查通过。包含包身份／路径／大小／摘要、损坏下载不覆盖已暂存文件、开发运行手动更新、安装确认取消、索引错误不改正文等回归。实际GitHub六平台构建、正式签名／公证、真实应用更新及helper升级尚未执行；macOS隔离占位打包成功不等于正式应用安装更新验收通过。

补充实际打包：使用本轮真实debug主程序及三个真实服务工具，已生成未签名／未公证macOS ARM64 pkg、Portable.zip及full.nupkg（/tmp/suki-velopack-real-macos-retry-20261103/Releases）。四程序存在、正式bundle ID、feed大小／SHA-256及signed:false已核验。未运行或安装这些产物；这仍不是release优化构建、签名或端到端更新验收。

2026-11-05修复验证：Linux两架构依赖图均只启用ashpd/async-io；本机Rust全量352项通过、16项忽略，随后新增共享代理设置回归通过，严格Clippy／fmt通过；Python打包回归4项通过。真实下载Mihomo v1.19.33和四份Geo并验证上游摘要，macOS ARM64已暂存四个应用／服务程序和五份完整离线资源，复核包内manifest、大小及摘要；未生成本轮正式安装器。默认get.syf.sh按应用User-Agent请求版本和实际tags API成功，Raw、GeoIP与内核归档完整内容摘要与官方直连一致。Linux交叉编译因本机缺aarch64-linux-gnu-gcc未完成；真实六平台CI、Windows Developer PowerShell和Linux AppImage运行仍待runner验证。

Windows ZIP条目修复：官方v1.19.33的x64／ARM64归档分别包含`mihomo-windows-amd64-compatible.exe`和`mihomo-windows-arm64.exe`。资源准备识别归档中唯一的`mihomo*.exe`并统一输出`mihomo.exe`；不再假设上游条目已经使用应用内文件名。打包回归5项通过；两份官方归档均已核对上游SHA-256，完成解包、五项资源manifest生成和打包复制校验，尚未执行Windows完整安装器构建。
