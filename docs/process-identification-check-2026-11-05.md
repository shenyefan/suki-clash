# 连接进程识别核查（2026-11-05）

## 调用链

CV：`use-connection-data.ts` 从 Mihomo WebSocket 取得 `/connections` 数据，保留 `metadata.process` 和 `metadata.processPath`；`connection-row-view.ts::getConnectionProcess` 优先显示进程名，其次为路径；两者为空时返回空字符串。CV 前端没有自行查进程或从路径之外生成进程名。

Suki：`core/streams.rs` → `core/monitor.rs::decode_connections` → `overview/statistics.rs::Metadata` → `traffic/connections.rs::Tracker` → 连接表格。JSON 字段名为 `process`、`processPath`，解析后原样传给列表及详情；两者均为空时，列表显示“未知进程”。元数据更新不依赖流量计数变化。

## 已验证的原因

Mihomo 的 strict 并不意味着每条连接都识别进程：只有规则需要进程信息时才调用查询；always 在进入路由匹配前尝试查询。off 禁止查询。内核内部连接以及权限、来源不属于本机等情况仍可能没有进程信息，always 也不能保证所有连接都识别。

只读查询当前本机运行的 Suki 私有接口得到 `find-process-mode: strict`，当时没有活动连接，不能据此认定用户所见连接的响应内容。

另使用本机已安装 Mihomo，在临时目录启动两个隔离实例，以同一个本地 HTTP 测试连接及 `MATCH,DIRECT` 规则比较：

| 配置 | metadata.process | metadata.processPath |
| --- | --- | --- |
| strict | 空 | 空 |
| always | Python | 有值 |

实验没有修改用户配置、系统代理或路由，没有访问外网，结束后终止隔离内核并删除临时目录。

## 处理与边界

- 保留既有用户选择及 strict 默认值，不静默改为 always。内核设置中的进程查找模式说明补充 strict/always 的真实区别；需要尽可能显示本机连接进程时可选择“始终（always）”。设置只影响新建立、重新执行路由判断的连接，不能假定已存在连接会补齐元数据。
- 补充原始 API JSON 到列表/详情的回归：Windows 进程名及路径、macOS 仅路径、Linux 进程名；相同连接 ID、上传和下载计数均不变时，元数据仍更新；关闭连接保留最后进程信息。
- 回归可以证明给定 JSON 中的字段不会丢失，不替代用户现场的同内核、同连接比对。用户所见实例的平台及那一条连接的原始字段尚未取得，不能宣称现场问题已消除。

## 来源

- [Mihomo metadata JSON 字段](https://github.com/MetaCubeX/mihomo/blob/Meta/constant/metadata.go)
- [Mihomo 进程查询与路由匹配](https://github.com/MetaCubeX/mihomo/blob/Meta/tunnel/tunnel.go)
- [本地 CV 连接数据](../clash-verge-rev-dev/src/hooks/use-connection-data.ts)
- [本地 CV 进程展示](../clash-verge-rev-dev/src/components/connection/connection-row-view.ts)
