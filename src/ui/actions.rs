//! Shared icons for application actions and menu entries.
use gpui_kit::{SharedString, assets::IconName, component::menu::PopupMenuItem};
pub(crate) fn icon(label: &str) -> IconName {
    if label.contains("删除") || label.contains("移除") {
        IconName::Trash
    } else if label.contains("复制") {
        IconName::Copy
    } else if label.contains("关闭") || label.contains("取消") || label.contains("断开") {
        IconName::X
    } else if label.contains("更新") || label.contains("刷新") {
        IconName::RefreshCw
    } else if label.contains("恢复") || label.contains("重置") || label.contains("撤销") {
        IconName::RotateCcw
    } else if label.contains("添加") || label.contains("新建") {
        IconName::Plus
    } else if label.contains("导入") || label.contains("下载") {
        IconName::Download
    } else if label.contains("编辑") || label.contains("YAML") {
        IconName::Pencil
    } else if label.contains("详情") || label.contains("日志") {
        IconName::Info
    } else if label.contains("打开") || label.contains("目录") {
        IconName::FolderOpen
    } else if label.contains("主页") || label.contains("链接") {
        IconName::Link
    } else if label.contains("正则")
        || label.contains("匹配")
        || label.contains("大小写")
        || label.contains("筛选")
    {
        IconName::SlidersHorizontal
    } else if label.contains("测速") || label.contains("延迟") {
        IconName::Gauge
    } else if label.contains("展开") {
        IconName::ChevronDown
    } else if label.contains("收起") {
        IconName::ChevronUp
    } else if label.contains("时间") {
        IconName::Clock
    } else if label.contains("名称") || label.contains("排序") {
        IconName::ArrowDownUp
    } else if label.contains("类型") {
        IconName::Layers
    } else {
        IconName::List
    }
}
pub(crate) fn menu(label: impl Into<SharedString>) -> PopupMenuItem {
    let label = label.into();
    PopupMenuItem::new(label.clone()).icon(icon(&label))
}
