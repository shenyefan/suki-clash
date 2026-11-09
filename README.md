> **自用 Vibe Coding 项目，请勿传播**

<p align="center">
  <img src="assets/logo.png" width="112" alt="Suki Clash 图标" />
</p>

<h1 align="center">Suki Clash</h1>

<p align="center">基于 Rust、GPUI Kit 和 Mihomo 的桌面代理客户端</p>

<p align="center">
  <a href="../../releases">下载</a> ·
  <a href="docs/README.md">项目文档</a> ·
  <a href="#开源致谢">开源致谢</a>
</p>

## 简介

Suki Clash 使用 GPUI Kit 构建原生桌面界面，以 Mihomo 为代理内核，提供订阅管理、代理切换、连接查看、流量统计和配置管理等功能。

项目面向 Windows、macOS 和 Linux 开发。不同平台的系统代理、TUN、服务和权限能力存在差异，实际验证范围见[平台生命周期记录](docs/reviews/platform/lifecycle.md)。

## 主要功能

- **订阅管理**：远程与本地订阅、更新、编辑、排序、激活，以及节点 URI 导入。
- **代理管理**：代理组与节点切换、延迟测试、Provider 管理和链式代理。
- **配置管理**：YAML 与 JavaScript 覆写、订阅增强、全局 DNS、嗅探和内核设置。
- **运行信息**：实时流量、流量历史、连接详情、规则查看，以及内核和程序日志。
- **网络集成**：系统代理、代理守卫、TUN 和平台服务管理。
- **桌面功能**：深浅主题、托盘、开机自启、内核与资源数据库更新。
- **备份与维护**：本地备份、GitHub Gist 备份、可选 Age 加密、诊断导出和应用更新。

具体执行边界、已知限制和待验收项见[功能对照与差距清单](docs/reference/cv/cv-gap-checklist.md)。

## 下载与使用

在 [GitHub Releases](../../releases) 中选择对应平台和架构的文件。仓库中的发布工作流配置了以下产物，是否已有可用版本以 Releases 页面为准：

| 平台 | 架构 | 文件形式 |
| --- | --- | --- |
| Windows | x64、ARM64 | 安装程序、免安装 ZIP |
| macOS | Apple Silicon（ARM64） | PKG、应用压缩包 |
| Linux | x64 | AppImage |

正式包包含 Mihomo 和初始化所需的 Geo 数据。启动后导入订阅或本地配置，选择订阅并启动内核，再按需启用系统代理或 TUN。

Windows 免安装版的用户数据仍保存在平台应用目录。TUN 与服务操作需要相应的平台权限；安装包的签名和公证状态以对应发布说明为准。

## 从源码构建

准备支持 Rust 2024 Edition 的 Rust 工具链，以及当前平台的原生构建依赖。Windows 使用 MSVC 工具链；Linux 所需开发库可参考[发布工作流](.github/workflows/release.yml)。

在仓库根目录执行：

```sh
# 构建主程序
cargo build --locked --release --bin suki-clash

# 构建平台服务
cargo build --locked --release \
  --manifest-path platform-service/Cargo.toml \
  --features standalone,client \
  --bin suki-clash-service
```

Windows PowerShell 可将第二条命令写成一行。主程序和平台服务须配套构建与分发。

开发运行可使用 `cargo run --locked --bin suki-clash`。直接从源码运行不会自动获得正式包附带的离线内核与 Geo 资源，资源准备、目录布局和打包步骤见下方文档。

## 文档

| 内容 | 入口 |
| --- | --- |
| 全部文档 | [文档索引](docs/README.md) |
| 设置与数据存储 | [设置存储](docs/features/settings/settings.md) |
| 订阅管理 | [订阅](docs/features/profiles/profiles.md) |
| 配置合成与覆写 | [规则、覆写、DNS 与嗅探](docs/features/configuration/flow.md) |
| 本地构建 | [本地跨平台构建](docs/build/local-cross-build.md) |
| 分发与更新 | [Velopack 打包与更新](docs/build/velopack.md) |
| 开发约定 | [AGENTS.md](AGENTS.md)、[DESIGN.md](DESIGN.md) |

## 开源致谢

感谢以下开源项目及其贡献者。Suki Clash 使用了这些项目的框架、内核、组件、资源或实现参考：

| 项目 | 用途 |
| --- | --- |
| [Rust](https://github.com/rust-lang/rust) | 主程序与平台服务的开发语言和工具链 |
| [GPUI Kit](https://github.com/longbridge/gpui-kit) | 桌面应用框架与组件整合 |
| [GPUI / Zed](https://github.com/zed-industries/zed) | 原生界面与渲染基础 |
| [GPUI Component](https://github.com/longbridge/gpui-component) | 界面组件 |
| [Mihomo](https://github.com/MetaCubeX/mihomo) | 代理内核 |
| [Clash Verge Rev](https://github.com/clash-verge-rev/clash-verge-rev) | 功能与行为参考，部分运行资源处理实现来源 |
| [clash-verge-service-ipc](https://github.com/clash-verge-rev/clash-verge-service-ipc) | 平台服务的上游实现 |
| [sysproxy](https://github.com/zzzgydi/sysproxy-rs) | 系统代理相关实现来源，保留 [MIT 许可](licenses/sysproxy-MIT.txt) |
| [meta-rules-dat](https://github.com/MetaCubeX/meta-rules-dat) | Geo 数据资源 |
| [Twemoji Mozilla](https://github.com/mozilla/twemoji-colr)、[Twemoji](https://github.com/twitter/twemoji) | 旗帜字体与图形，见[字体说明及署名](assets/fonts/README.md) |
| [Lucide](https://github.com/lucide-icons/lucide) | GPUI Kit 组件使用的图标资源 |
| [QuickJS](https://github.com/bellard/quickjs)、[rquickjs](https://github.com/DelSkayn/rquickjs) | JavaScript 覆写执行 |
| [Tree-sitter](https://github.com/tree-sitter/tree-sitter) | 编辑器语法解析与高亮 |
| [Velopack](https://github.com/velopack/velopack) | 应用打包、分发与更新 |
| [linuxdeploy](https://github.com/linuxdeploy/linuxdeploy)、[AppImage](https://github.com/AppImage/AppImageKit) | Linux 应用打包 |
| [cargo-xwin](https://github.com/rust-cross/cargo-xwin)、[LLVM](https://github.com/llvm/llvm-project)、[Wine](https://gitlab.winehq.org/wine/wine) | 本地跨平台构建 |
| [FontTools](https://github.com/fonttools/fonttools) | 旗帜字体裁剪与处理 |

主程序及平台服务使用的 Rust 库，包括直接依赖、间接依赖、构建依赖、测试依赖和平台依赖，完整列于[第三方依赖清单](THIRD_PARTY.md)。版本与来源分别以根目录 [Cargo.lock](Cargo.lock) 和服务的 [Cargo.lock](platform-service/Cargo.lock) 为准。

随包 Geo 数据还包含 MaxMind 提供的 GeoLite2 数据，相关来源见[运行资源说明](packaging/runtime-sources.txt)。服务与相关改编代码的 GPL-3.0 许可见 [platform-service/LICENSE](platform-service/LICENSE)，字体的 Apache-2.0 与 CC BY 4.0 声明见 [Twemoji-LICENSE.md](assets/fonts/Twemoji-LICENSE.md)。各第三方项目的代码、图形和数据遵循其各自许可。
