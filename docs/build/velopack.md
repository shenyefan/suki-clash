# Velopack 交付与更新

状态更新（2026-11-09）：用户确认文档此前列出的实机验收部分均已完成，相关待验收项关闭。下文各次检查中的“未验收／待验收”保留为当时记录；本次未重新运行验收。实现范围、暂缓项及能力限制以[当前功能对照](../reference/cv/cv-gap-checklist.md)为准。

本项目首次接入固定使用 Rust SDK／`vpk` **1.2.161**。Velopack处理应用分发和更新；GPUI Kit仍负责应用界面，Mihomo与独立平台服务保持现有所有权。参考依据为[官方Rust接入](https://docs.velopack.io/getting-started/rust)、[官方打包参数](https://docs.velopack.io/packaging/overview)、[GitHub发布](https://docs.velopack.io/distributing/github-actions)及[1.2.161源码](https://github.com/velopack/velopack/tree/1.2.161)，不使用仅新版本提供的参数。`.config/dotnet-tools.json`使用官方 dotnet local tool清单固定CLI版本，CI执行`dotnet tool restore`后直接调用`dotnet vpk`。

## GitHub自动构建

`.github/workflows/release.yml`提供手动试构建和`v*`标签构建。标签必须严格等于根Cargo.toml版本的`v{version}`。初版只允许稳定版本，拒绝预发布／build metadata，避免beta进入默认stable源。普通分支推送不自动发布。

四个原生runner（`windows-latest`、`macos-latest`、`ubuntu-latest`）分别构建主程序及集成维护的服务程序，使用Cargo.lock；不把整个target目录加入包。矩阵全部成功后才汇总，产物不会由并行job抢写同一Release。手动运行只保留Actions artifacts；标签运行创建／更新**一个草稿Release**，不会自动公开发布，不修改已发布同名版本。用户检查草稿后再发布。普通分支推送不自动发布。

工作流默认使用bash，较长的步骤放在`packaging/`下的脚本中，各脚本只在GitHub runner上运行。只在Ubuntu上运行的步骤用shell；三个平台都要运行的暂存和打包用Python，避免Git Bash改写Windows路径和参数：

| 脚本 | 运行位置 | 职责 |
| --- | --- | --- |
| `runtime.sh` | prepare（ubuntu-latest） | 下载并校验全部平台的内核与Geo，生成各目标的离线资源及manifest |
| `stage.py` | 各平台构建runner | 按平台目录结构暂存程序和离线资源；macOS生成图标并写入版本，Linux运行linuxdeploy |
| `pack.py` | 各平台构建runner | 调用官方`dotnet vpk pack`，写入`build.{channel}.json` |
| `release.sh` | release（ubuntu-latest） | 汇总下载文件与更新清单，生成发布说明并创建／更新草稿Release |

选择MSVC工具链和两个`cargo build`步骤使用PowerShell：前者调用Visual Studio自带的`Launch-VsDevShell.ps1`；后者避免Git Bash的`/usr/bin/link`遮挡MSVC的`link.exe`（pwsh在所有runner上可用）。当前不签名、不公证。

| 平台／架构 | 更新channel | 用户下载文件 |
| --- | --- | --- |
| Windows x64／ARM64 | win-x64／win-arm64 | SukiClash-{channel}.msi、SukiClash-{channel}-Portable.zip |
| macOS Apple Silicon | osx-arm64 | SukiClash-{channel}-Setup.pkg；同时保留Velopack生成的Portable.zip |
| Linux x64 | linux-x64 | SukiClash-{channel}.AppImage |

Release附件为上表的7个安装器／压缩包，加上每个channel的更新清单`releases.{channel}.json`和完整更新包`SukiClash-{version}-{channel}-full.nupkg`（应用内更新的数据源，缺少任一项则汇总失败）。`assets.{channel}.json`、`build.{channel}.json`仅保留在Actions artifacts中14天。禁用delta，不依赖历史版本下载。SDK的PackageId固定`SukiClash`。Linux原生不生成deb／rpm，需要时另接发行版打包工具，不能把AppImage称为deb安装器。各架构文件互不覆盖。

Windows便携版是**免安装程序包**，用户配置仍沿当前平台应用目录保存，不宣称数据与程序同目录。Windows构建采用静态CRT，避免只在安装版补VC运行库而让便携版漏依赖。便携版使用TUN仍遵守现有服务安装／权限要求。

Windows 使用 Velopack 官方 MSI：`pack.py` 传入 `--msi --instLocation PerMachine`，供所有用户使用。当前固定 CLI 的实际生成目录为 `%ProgramFiles%\SukiClash`，没有公司名子目录；已检查 x64／ARM64 MSI 中的 `ALLUSERS=1`、`ApplicationFolderName=SukiClash` 以及交互／静默安装共用的 Program Files 默认目录设置。安装所需权限由 Windows Installer 的标准流程处理，不另加提权包装程序。目录采用官方 PerMachine 默认值，不定制 MSI 属性或安装逻辑。`Moenya` 仅保留为现有发布者／作者元数据。

外观使用 Velopack 官方默认向导和控件，`--icon` 使用现有应用图标；不提供自定义顶部横幅或欢迎页背景，也不修改生成的 MSI。CLI 同时生成的 Setup.exe 仅保留在 Actions artifacts，Release 的 Windows 安装入口只发布 MSI，另保留 Portable.zip 和原有更新源。

用户配置、订阅、覆写、内核、Geo、备份、日志和缓存统一使用各用户的 `%LOCALAPPDATA%\suki-clash`，不读取或迁移其他用户目录；具体结构见[设置存储](../features/settings/settings.md)。这些用户文件不写入 Program Files。

## 服务生命周期接入边界

仅计划在 Windows 接入 Velopack 服务维护 hook；macOS、Linux 继续使用现有应用内维护入口。虚拟网卡页保留安装、更新、修复和卸载动作。当前主程序尚未注册服务生命周期 callback。

依据[官方 App Hooks 文档](https://docs.velopack.io/integrating/hooks)，安装及卸载 fast callback 最长 30 秒，更新前后最长 15 秒，不允许显示 UI，失败不能取消安装、更新或卸载。项目所用 `vpk 1.2.161` 的 Setup.exe 为 `asInvoker`，当前服务维护入口会请求 UAC 并等待结果，不能直接将该入口放入 fast callback。首次启动和更新重启的普通运行 callback 没有 fast callback 的退出期限，但将提权维护放到这些阶段会改变权限请求时机。

PerMachine 只确定应用安装范围，不会自动安装平台服务。官方 MSI 的安装／卸载 hook 使用 `Impersonate="yes"`、`Return="ignore"`，不能把 MSI 获得安装权限视为应用 hook 已获得服务维护权限。当前仍使用应用内服务维护入口，尚未接入 hook，也未增加服务可选安装项或自动服务安装、更新、卸载。应用内 Update.exe 更新不会执行 MSI 的自定义动作，不能把应用包更新完成视为包外系统服务已经更新。macOS、Linux 删除应用也不会调用 Velopack 卸载 hook，服务由用户通过现有入口单独卸载。

## 文件结构与资源

`packaging/stage.py`读取`target/<目标>/release`与`platform-service/target/<目标>/release`中的两个可执行文件：`suki-clash`、`suki-clash-service`（Windows 使用 `.exe`），构建目录内其他工具不会随包。安装／卸载由服务程序自身承接。macOS放入Contents/MacOS，资源在Contents/Resources；其他平台资源相对实际可执行文件为resources/。打包作者／公司为 Moenya，Windows 两个可执行文件的公司与版权属性、macOS 应用版权同步使用 Moenya；`pack.py`直接调用官方`dotnet vpk`。各可执行文件版本来自所属 Cargo 包。

resources 仅保留离线运行资源与必要的平台图标，不再复制 licenses 目录、协议／来源说明或 logo.png。界面及托盘使用编译进主程序的图片；macOS 保留供 Finder／Dock 使用的 suki-clash.icns，Linux 保留 AppDir 根目录下供桌面入口使用的 suki-clash.png，Windows 图标嵌入可执行文件及安装器。仓库中的原始协议文件保留。

Mihomo、四份 GeoData 和 manifest.json 直接放在资源目录中，不再嵌套 runtime/。Windows 安装版与便携版使用程序旁的 resources/，Linux AppImage 内部使用 usr/bin/resources/，macOS 安装版与应用压缩包使用 Contents/Resources/。运行资源没有嵌入主程序。manifest.json 保存目标平台、文件长度和 SHA-256，首次启动按清单校验并将缺失文件复制到用户数据目录 core/；已有内核和数据库不被覆盖。因此随包资源用于离线初始化，用户目录中的副本用于实际运行及独立更新，二者承担不同职责。

所有正式包必须内置 **Mihomo、Country.mmdb、ASN.mmdb、geoip.dat、geosite.dat**。prepare job 在一台 Ubuntu runner 上执行`packaging/runtime.sh`：读取 MetaCubeX/mihomo 与 meta-rules-dat 的 latest 发行元数据，用 GitHub 提供的上游 SHA-256 生成`SHA256SUMS`，以`gh release download`下载四架构内核及四份 Geo（临时失败最多重试三次），再以`sha256sum -c`校验。校验通过后，为每个 Rust 目标解出内核（Windows 归档中唯一的`mihomo*.exe`统一命名为`mihomo.exe`，其他平台为`mihomo`），将 Geo 改为应用内文件名，并生成应用现有格式的manifest。结果按目标分目录作为`runtime` artifact 保留14天，各构建runner直接复制本目标目录，不再访问上游。资源缺失、摘要不符或内核归档条目不唯一均导致失败，不生成缺资源的包。内核采用官方稳定版，Geo采用应用默认来源。

artifact 不保留文件权限；首次启动由现有bootstrap按manifest校验并安装到用户数据目录，同时设置内核执行权限，已有用户内核和数据库不被覆盖。源码开发运行仍允许没有随包资源；正式包只在GitHub runner上构建。

工作流的checkout、.NET和artifact Actions已切换到原生Node.js 24版本；MSVC使用Visual Studio自带Developer PowerShell，移除仍依赖Node.js 20的工具链Action。Linux密钥库oo7禁用默认Tokio后端，显式使用与GPUI门户相同的async-io后端，避免ashpd互斥feature冲突。

GitHub下载代理由共享HTTP规则处理，覆盖内核版本／归档、Geo手动下载、公开订阅／覆写／图标，以及应用更新清单和完整包；携带Authorization的请求不转交第三方代理。内核自身按geox-url发起的后台更新由Mihomo负责，不经过应用HTTP规则。CI从官方GitHub获取资源，不读取用户下载代理设置。

Linux使用成熟linuxdeploy打包动态依赖：`stage.py`下载固定的`1-alpha-20251107-1` x86_64 AppImage，校验SHA-256后解包运行，再由vpk生成AppImage；不自己扫描／复制依赖协议。AppImage的可用性仍受目标glibc、Vulkan／图形驱动、桌面环境、服务授权工具和发行版限制，不能仅靠生成文件宣称所有Linux可运行。

## 草稿状态

当前所有包均未签名、未公证，工作流不读取任何签名secrets。Release说明提示 Windows 可能出现 SmartScreen 提示、macOS 首次打开需在系统设置中允许。

GitHub上传权限只给最终草稿job；不需要在仓库写PAT。更新源仓库通过构建环境`SUKI_UPDATE_REPOSITORY=${{ github.repository }}`写入程序；本地开发没有该值时不得猜GitHub地址或伪造可更新状态。Release描述包含平台／可点击下载的文件名表、必要使用说明，以及GitHub自动生成的版本变更记录，其中“完整变更”链接单独成行；重跑时同步更新草稿说明，并删除草稿中不属于当前下载文件、更新清单和完整包的旧附件。已公开的同名Release仍拒绝修改。

更新清单和完整包随Release公开，应用请求`releases/latest/download/releases.{channel}.json`并按清单下载完整包。v0.2.4与v0.2.6的Release未包含这些文件，对它们检查会报清单获取错误；自动更新从包含清单的版本开始生效，且需要用旧版本安装包升级到该版本后再验证。草稿不会成为普通用户的更新源。

## 当前验证边界

2026-11-13：通过固定 `vpk 1.2.161`，使用隔离占位程序分别生成 Windows x64／ARM64 MSI、Portable.zip、完整更新包和 feed；检查 MSI 安装范围、交互／静默默认目录及目标架构。首次打包检查使用过自定义横幅和背景，此后已移除这两份图片及参数，恢复官方默认外观；移除后检查 Python 语法及差异，未重新生成测试 MSI。未安装或运行这些测试包，未操作真实服务；本次结果不能代替正式程序的安装、卸载、更新及向导实机验收。

本地验证了Python语法、workflow YAML解析、plist与暂存结构；实际安装`vpk 1.2.161`检查CLI参数，并用隔离占位Mach-O二进制生成macOS pkg／Portable.zip／full.nupkg／channel feed，核对PackageId、版本、文件名、摘要和大小。该冒烟验证未运行／安装产物，也不是实际Suki发布验证。

尚需在GitHub执行当前四平台构建，并验收安装／卸载、便携版、AppImage依赖、更新前系统代理与内核清理、服务协议升级后的显式修复及更新失败恢复。本机没有上传Release或操作真实系统服务。


## 应用内执行与开发运行

Velopack启动钩子在单实例和GPUI之前执行，明确关闭启动自动应用待安装包。设置→维护→检查应用更新通过生命周期命令执行：先确认当前安装身份，再由现有reqwest共享客户端取得当前平台／架构的公开stable清单和完整包；检查PackageId、固定文件名、大小与SHA-256，流式下载到临时文件，全部通过才发布。网络取消只取消当前下载，不自动重跑；SDK仅从已验证本地文件源准备包，不另走ureq联网或差分回退。

检查中和无更新使用 toast；发现新版本后展示当前版本 → 目标版本及 GitHub 比较接口返回的全部提交标题，按页读取，不受默认 250 条限制；变更请求复用共享 HTTP 与 GitHub 下载代理。变更读取失败通过 toast 提示并保留更新和完整变更链接。开始下载关闭结果弹窗，进度仅使用持续 toast，完成后自动打开安装重启确认。

下载完成后用户确认安装并重启。先核验本地待安装版本，再通过现有内核／系统代理清理与存储队列屏障；清理失败不调用更新器。成功后调用SDK的等待当前进程退出接口，不使用会直接process::exit的快捷API。更新器启动失败保留主界面和执行端供重试，不把更新提交事实伪装为成功。平台服务仍为独立受限服务，新应用协议不符时使用既有显式修复入口，不能把主程序更新等同服务升级。

非Velopack安装包检查更新时使用 toast 提供 GitHub 最新发布页入口，开发环境默认官方仓库；正式构建按注入仓库提供链接。不弹“请安装正式分发包”错误，不把开发运行假装成自动更新成功。真正的包清单、网络、权限或校验失败仍报告原原因，不自动打开链接掩盖失败。

2026-11-06 更新交互验证：更新相关回归 23 项通过、2 项按资源条件跳过，严格 Clippy、格式和差异检查通过。macOS 隔离窗口核对了 960×640 最小窗口、深浅主题、短／长变更及滚动、Escape／Tab、检查进度 toast 和安装重启确认；未操作真实更新安装、系统代理或服务，Windows／Linux 尚未实机验收。

macOS只有NSBundle具有bundleIdentifier时才执行订阅协议注册，cargo run不进行不适用注册。真实索引旧格式错误会指出具体文件；不会加入自动迁移。2026-11-03经用户单独授权，一次性备份并移除两份索引的3个冗余正文键，独立正文及其他字段一致性已核验；该操作没有放进应用启动代码。

本轮自动检查：根项目346项通过、16项条件忽略；严格Clippy与格式／差异检查通过。包含包身份／路径／大小／摘要、损坏下载不覆盖已暂存文件、开发运行手动更新、安装确认取消、索引错误不改正文等回归。实际GitHub六平台构建、正式签名／公证、真实应用更新及helper升级尚未执行；macOS隔离占位打包成功不等于正式应用安装更新验收通过。

补充实际打包：使用本轮真实debug主程序及三个真实服务工具，已生成未签名／未公证macOS ARM64 pkg、Portable.zip及full.nupkg（/tmp/suki-velopack-real-macos-retry-20261103/Releases）。四程序存在、正式bundle ID、feed大小／SHA-256及signed:false已核验。未运行或安装这些产物；这仍不是release优化构建、签名或端到端更新验收。

2026-11-05修复验证：Linux两架构依赖图均只启用ashpd/async-io；本机Rust全量352项通过、16项忽略，随后新增共享代理设置回归通过，严格Clippy／fmt通过；Python打包回归4项通过。真实下载Mihomo v1.19.33和四份Geo并验证上游摘要，macOS ARM64已暂存四个应用／服务程序和五份完整离线资源，复核包内manifest、大小及摘要；未生成本轮正式安装器。默认get.syf.sh按应用User-Agent请求版本和实际tags API成功，Raw、GeoIP与内核归档完整内容摘要与官方直连一致。Linux交叉编译因本机缺aarch64-linux-gnu-gcc未完成；真实六平台CI、Windows Developer PowerShell和Linux AppImage运行仍待runner验证。

Windows ZIP条目修复：官方v1.19.33的x64／ARM64归档分别包含`mihomo-windows-amd64-compatible.exe`和`mihomo-windows-arm64.exe`。资源准备识别归档中唯一的`mihomo*.exe`并统一输出`mihomo.exe`；不再假设上游条目已经使用应用内文件名。打包回归5项通过；两份官方归档均已核对上游SHA-256，完成解包、五项资源manifest生成和打包复制校验，尚未执行Windows完整安装器构建。
