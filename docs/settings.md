# 设置存储

设置位于平台应用目录的 `settings/`。macOS 默认位置为 `~/Library/Application Support/suki-clash/settings/`。

| 文件 | 内容 |
| --- | --- |
| `settings.json` | 全部程序设置，按能力分组 |
| `mihomo.yaml` | Mihomo 原生参数及 GeoData 地址 |
| `dns.yaml` | 原生 DNS 与 Hosts 配置 |
| `sniffer.yaml` | 原生 sniffer 配置 |

非内核字段全部保存到 `settings.json`，不写入 YAML。DNS/嗅探的全局开关属于程序偏好；`dns.enable`、`sniffer.enable` 属于内核服务开关。两者独立，关闭全局配置时继承订阅，不改写已保存的内核服务开关。

程序设置示例（其余默认字段省略）：

```json
{
  "schema_version": 1,
  "general": {
    "theme_index": "dark",
    "notice_position": "top_center",
    "app_log_level": "info"
  },
  "core": {
    "start_core": true,
    "core_release_index": "latest",
    "web_ui": {"panels": []}
  },
  "dns": {"dns_override_enabled": false},
  "sniffer": {"sniffer_override_enabled": false},
  "sysproxy": {},
  "tun": {},
  "runtime": {"selected_profile_id": null},
  "proxy_chain": {
    "enabled": false,
    "group": "",
    "nodes": [],
    "custom": {"proxies": []}
  }
}
```

对象、数组、布尔和数字使用原生类型。固定枚举保存英文标识，用户自定义文字保持原文。默认值集中在 `src/preferences/`；缺失文件生成默认值，缺失字段使用当前默认值。

原生配置示例：

```yaml
dns:
  enable: true
  nameserver: [https://dns.alidns.com/dns-query, https://doh.pub/dns-query]
  fake-ip-filter: ['*.lan', localhost.ptlogin2.qq.com]
hosts: {}
```

表单和 YAML 编辑器共用原生文档；修改已知字段保留其他字段。编辑器不包含程序总开关或其他应用标记；代码编辑不会改变程序总开关。DNS 和嗅探的总开关位于标题旁，只显示开关，保留无障碍名称。总开关关闭时 YAML 编辑器禁用，内容保留；开启后恢复编辑。

订阅、覆写及增强索引在数据目录 `records/`，正文在 `profiles/`、`overrides/`；节点选择记录属于用户数据。当前订阅 ID 在程序设置的 `runtime` 分组，启动时重新读取订阅并合成，不保存或依赖 `applied.json`。

## 更新与失败

同一程序设置文件使用单一写入锁。每次更新读取最新文档，仅替换所属分组，再原子写入；不同模块更新不覆盖其他分组。配置和订阅事务恢复只恢复自己拥有的分组，不能用旧的整份程序设置覆盖其他模块的新值。页面备份将原生配置对象和程序开关分别记录；恢复只返回待提交值，不直接覆盖文件。

设置页自动保存，无保存按钮。开关和枚举立即提交，文本与多行编辑失焦提交；切换表单/YAML 模式也提交当前修改。运行配置先合成、校验、热加载和确认，再提交；停止时只保存。内核拒绝时保留界面草稿，重新打开读取已提交值。

配置损坏时保留原文件和完整界面，通过错误 toast 和程序日志报告。语法错误含文件位置及行列，不输出正文。合法 JSON 中某一分组类型错误不阻止其他分组的操作；整份 JSON 语法错误时拒绝改写该文件，日志、导航及独立用户数据仍可用。修正文件后重试，不要求重启应用。

只实现当前文件和结构，不读取、迁移、移动或删除其他格式文件。

## 凭据

内核控制凭据保存在受限权限的原生内核设置文件。订阅和覆写的 Authorization、Age 私钥使用各自的安全存储命名空间，不进入程序设置或索引。GitHub/WebDAV 等未接入的凭据存储保持明确拒绝，不明文保存。

DNS、嗅探与完整文档编辑的热应用和布局规则见 [flow.md](flow.md)。
