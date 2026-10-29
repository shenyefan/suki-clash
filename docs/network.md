# 网络与库审计

HTTP 使用 reqwest 0.13，WebSocket 使用 tokio-tungstenite 0.30，网卡信息使用 netdev 0.47，ZIP 使用 zip 9，CIDR 使用 ipnet，二维码使用 qrcode 的 SVG 渲染；gzip 沿用 flate2。图标校验使用 image 与 usvg，data URL 使用 base64 和 percent-encoding。构建资源下载使用 Python 标准 urllib。库版本以 Cargo.lock 为准。

直连与指定代理使用独立连接池，明确关闭环境代理；系统代理按任务读取，避免长期缓存系统代理设置。请求限制协议、鉴权、重定向、时间、正文大小，敏感标头不出现在错误与日志。下载先写临时文件、流式限制大小、同步后交由所属业务验证并提交，取消释放请求与临时文件。取消不触发其他下载策略。

保留的进程调用用于真实内核生命周期及平台集成；内核日志沿标准输出/错误输出采集，覆盖校验与启动阶段，不重复订阅同一内核日志。私有 API 沿用 Hyper 的 Unix Socket/Windows Named Pipe；WebSocket 复用同一私有传输，不开放额外 TCP 控制端口。

新默认设置仅允许环回网段访问代理；用户显式设置的空列表或拒绝规则保持原值，无法通过本地代理时提示实际原因。启动不自动扩大允许范围。

自动化覆盖分块大小限制、重定向鉴权隔离、错误脱敏、取消清理、ZIP 路径与重复文件、真实 WS 消息、Ping/Pong、重连与代次变化。CV 的 ws hooks、connections_stream、delay manager 和 icon cache 作为行为核对来源。没有旧格式转换、迁移或 curl 备用实现。

Windows/Linux 原生构建、系统证书和系统代理、PAC、真实窗口视觉与大数据性能验收尚未完成。测试不修改本机系统代理、路由或服务。验证范围见 [CV 差距清单](cv-gap-checklist.md)及[系统代理验证记录](system-proxy.md#验证)。

## Windows 私有通道

命名管道连接遵循 Tokio 的 `ClientOptions::open`：仅对 Win32 `ERROR_PIPE_BUSY`（231）短暂等待重试，受现有请求时限及两秒连接上限约束；其他错误立即返回，并保留系统错误信息和错误码。HTTP 与 WebSocket 共用此入口，不增加备用通信通道。

原日志只输出 `ErrorKind`，无法据此确认原错误一定是 231。增加管道忙恢复路径并保留原始错误，便于区分权限、管道不存在及其他系统原因。
