# 外部控制设置修改后的启动失败排查

## 诊断记录

诊断包 `suki-diagnostics-20261106T083338.782252300` 来自 Windows x86_64、Suki Clash 0.3.4。用户确认首次修改的是内核页的外部控制／端口／密钥；首次操作早于 DEBUG 日志，因此无法从记录还原具体字段及值。

- 16:10 内核就绪，16:13 TUN 正常工作，随后有正常流量。
- 16:31:07 请求 21 保存设置失败：新内核和恢复的旧内核均因 TUN 回读不一致而失败，设置未提交。
- 请求 23、24 是另两次输入校验失败：开启外部控制需要非零端口和访问密钥。它们与请求 21 的运行就绪失败分开处理。
- 请求 28、32、35 再次启动，均在约 300 ms 内因 TUN 回读不一致停止。
- 每两秒的“系统代理已清除”来自非就绪状态下的代理轮询，不代表每两秒重新启动内核。
- 同一次请求错误分别由 `core::service` 和 GPUI 桥接层记录，导致重复错误日志。

## 调用链与参考核对

`src/core/service.rs::configure` 将 `external-controller`、`external-controller-cors`、`secret` 的修改走校验后的受控重启；该路径保留旧 Launch，失败时恢复旧内核。

原 `wait_ready` 在 `/version` 首次成功后，仅即时回读一次监听配置和 TUN。私有 IPC 可用并不表示后续监听及 TUN 已完成初始化；新内核和旧内核恢复共用这条判断，因而可以连续误判。

核对依据：

- 本地 CV `clash-verge-rev-dev/src-tauri/src/core/manager/state.rs`：`poll_sidecar_readiness` 使用有界探测，配置为 30 次、间隔 100 ms、单次超时 400 ms；中间探测失败使用调试日志。
- CV `core/manager/lifecycle.rs`：停止／替换阶段协调代理守卫与清理，内核就绪后才恢复代理，并关联生命周期代次。
- CV `feat/config.rs`、`feat/tun.rs` 与 `core/manager/config.rs`：配置操作由生命周期执行端协调。Suki 保留自身的串行执行与提交点，不引入 Tauri 命令体系，也不采用失败时自动关闭 TUN 的策略。
- 本地 C# 参考 `Services/AppCoordinator.cs`：API 就绪和 `ApplyTunConfigAndVerifyAsync` 的限时状态确认分开处理；`MihomoCoreManager.cs` 负责进程生命周期。
- Mihomo 上游 [`hub/hub.go`](https://github.com/MetaCubeX/mihomo/blob/Meta/hub/hub.go) 的 `ApplyConfig` 先创建控制接口，再调用 executor；[`hub/executor/executor.go`](https://github.com/MetaCubeX/mihomo/blob/Meta/hub/executor/executor.go) 随后更新 listeners 和 TUN。这说明控制接口可能先于实际配置就绪。

日志与源码支持“过早确认实际配置”的缺陷；诊断没有首次失败的期望／实际值，也没有 Windows 驱动实测，不能据此排除持续性的 TUN 或系统故障。

## 修复

- 在私有客户端集中确认监听与 TUN：每 100 ms 回读，最多等待 3 秒；持续不一致或通信失败仍返回最后一次原因。TUN 不一致包含安全的期望／实际布尔值。
- 启动、受控重启、旧内核恢复、设置热加载及订阅／覆写配置应用和恢复使用同一确认逻辑；成功确认后才提交对应配置。不自动修改端口、访问密钥或 TUN 设置。
- 系统代理轮询仅观察实际状态；停止、进程退出和运行失效的转换负责清理。热加载恢复失败等失效转换同样停止代理守卫并清理一次，清理失败与原错误分别保留。
- 命令结果快照明确携带错误是否已记录；执行端记录一次含请求编号的错误，界面继续显示一次错误 toast。自动故障仍由桥接层记录。
- 提交拒绝同样只记录一次。资源页不在构造时发出请求，自动刷新及打开页面刷新均要求内核已就绪，避免执行端不可用时再次弹出启动失败。

## 验证

- `cargo test --locked core:: -- --nocapture`：111 通过，16 项显式要求真实内核或运行包的测试未执行。覆盖延迟就绪、持续不一致的有界失败、恢复、代理清理及命令错误通知归属；本地接口和系统代理适配器均隔离。
- `cargo test --locked app:: -- --nocapture`：8 通过，包含多窗口尺寸下全部导航目的地及侧栏交互，以及执行端不可用时启动错误只提示一次。
- `cargo test --locked settings:: -- --nocapture`：18 通过，包含资源页、设置拒绝后的草稿保留和更新弹窗。
- `cargo clippy --locked --all-targets -- -D warnings`：通过。
- macOS 原生窗口：使用独立临时设置、禁用内核执行端，检查深色／浅色纯色仪表盘。顶部正文与卡片正文左边缘对齐，右侧不叠加额外底面；原卡片与普通字号保留。临时检查进程及源码入口已清理。
- Windows 服务和真实 TUN 驱动未实机验证。
