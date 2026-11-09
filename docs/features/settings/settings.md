# 设置存储

设置位于本机配置目录（不漫游）的 `suki-clash/settings/`；各类内容使用各平台的标准位置，不读取旧位置。

| 内容 | Windows | macOS | Linux |
| --- | --- | --- | --- |
| 设置 | `%LOCALAPPDATA%\suki-clash` | `~/Library/Application Support/suki-clash` | `~/.config/suki-clash` |
| 数据（订阅、覆写、内核、备份） | `%LOCALAPPDATA%\suki-clash` | `~/Library/Application Support/suki-clash` | `~/.local/share/suki-clash` |
| 日志 | `%LOCALAPPDATA%\suki-clash\logs` | `~/Library/Logs/suki-clash` | `~/.local/state/suki-clash/logs` |
| 缓存 | `%LOCALAPPDATA%\suki-clash\cache` | `~/Library/Caches/suki-clash` | `~/.cache/suki-clash` |
| 实例锁 | 同缓存 | 同缓存 | `$XDG_RUNTIME_DIR/suki-clash`，缺失时同缓存 |

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
    "core_release_index": "latest",
    "web_ui": {"panels": []}
  },
  "dns": {"dns_override_enabled": false},
  "sniffer": {"sniffer_override_enabled": false},
  "sysproxy": {},
  "tun": {},
  "profiles": {"selected_profile_id": null, "items": []},
  "proxy_selections": [],
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

订阅元数据和节点选择在程序设置中，覆写及增强索引在数据目录 `indexes/`，正文在 `profiles/`、`overrides/`。当前订阅 ID 在程序设置的 `profiles.selected_profile_id` 字段，启动时重新读取订阅并合成，不保存或依赖 `applied.json`。

## 更新与失败

启动前初始化先完成，运行中的程序设置统一由生命周期执行端串行提交，不再维护额外的路径锁表。每次更新读取最新文档，仅替换所属分组，再原子写入；不同模块更新不覆盖其他分组。配置和订阅事务恢复只恢复自己拥有的分组，不能用旧的整份程序设置覆盖其他模块的新值。页面备份将原生配置对象和程序开关分别记录；恢复只返回待提交值，不直接覆盖文件。

设置页自动保存，无保存按钮。开关和枚举立即提交，文本与多行编辑失焦提交；切换表单/YAML 模式也提交当前修改。运行配置先合成、校验、热加载和确认，再提交；停止时只保存。内核拒绝时保留界面草稿，重新打开读取已提交值。

配置损坏时保留原文件和完整界面，通过错误 toast 和程序日志报告。语法错误含文件位置及行列，不输出正文。合法 JSON 中某一分组类型错误不阻止其他分组的操作；整份 JSON 语法错误时拒绝改写该文件，日志、导航及独立用户数据仍可用。修正文件后重试，不要求重启应用。

只实现当前文件和结构，不读取、迁移、移动或删除其他格式文件。

## 凭据

内核控制凭据保存在受限权限的原生内核设置文件。订阅和覆写的 Authorization、Age 私钥使用各自的安全存储命名空间，不进入程序设置或索引。GitHub/WebDAV 等未接入的凭据存储保持明确拒绝，不明文保存。

DNS、嗅探与完整文档编辑的热应用和布局规则见 [flow.md](../configuration/flow.md)。

## 用户状态与缓存

订阅元数据和当前选择统一保存在 `settings/settings.json`：

```json
{
  "profiles": {
    "selected_profile_id": 1794184021508,
    "items": [
      {
        "id": 1794184021508,
        "name": "Meow",
        "url": "",
        "enabled": true,
        "update_interval_minutes": 1440,
        "request_timeout_seconds": 30,
        "user_agent": "",
        "with_proxy": false,
        "self_proxy": false,
        "accept_invalid_certs": false,
        "updated_at": null,
        "home": "",
        "usage": null
      }
    ]
  },
  "proxy_selections": [
    {"profile_id": 1794184021508, "groups": {"AI": "Land"}}
  ]
}
```

`items` 数组顺序就是订阅顺序。代理选择的 `profile_id: null` 表示未选订阅的基础配置。流量历史唯一保存到缓存目录 `traffic.json`，不进入配置备份。覆写索引 `override-index.json` 和订阅增强 `profile-enhance-<id>.json` 使用数据目录 `indexes/`。系统代理恢复记录使用运行目录。不再创建或读取 `records/`，不兼容、迁移或自动删除旧文件。

## 重置程序

设置 → 维护提供唯一全局重置入口，确认后先停止内核／系统代理并等待存储线程结束。随后删除 `settings/`、数据目录中的 `indexes/`、`profiles/`、`overrides/` 及配置目录中的 `runtime-source.yaml`，成功后退出。下次启动重新生成默认设置。内核、资源数据库、日志、备份与统计历史不属于配置删除范围。部分删除失败会明确报告重置未完成，不执行自动迁移或备份。仅具有程序默认值的设置多行文本可恢复默认；订阅、覆写、增强和自定义代理链属于用户内容，不提供恢复默认。其他页面不再提供重置入口。

删除与恢复默认提供可自动关闭的撤销通知。重置程序确认后先等待 5 秒，点击撤销会取消执行；等待结束后才停止后台任务并删除配置。Fake-IP 清理采用相同的执行前撤销窗口。
