//! Assemble independent failures once at their owning workflow boundary.
/// Repeated causes from parallel requests or retries are reported once.
/// Messages remain intact: user text is never split or rewritten.
pub(crate) fn join(errors: impl IntoIterator<Item = impl AsRef<str>>) -> String {
    let mut unique = Vec::<String>::new();
    for error in errors {
        let message = error.as_ref();
        if !message.is_empty() && !unique.iter().any(|existing| existing == message) {
            unique.push(message.to_owned());
        }
    }
    unique.join("；")
}
/// Retain which independent operations failed, with each shared cause only once.
pub(crate) fn scoped<S: AsRef<str>>(errors: impl IntoIterator<Item = (S, String)>) -> String {
    let mut causes = Vec::<(Vec<String>, String)>::new();
    for (scope, message) in errors {
        let scope = scope.as_ref().to_owned();
        if let Some((scopes, _)) = causes.iter_mut().find(|(_, cause)| cause == &message) {
            if !scopes.contains(&scope) {
                scopes.push(scope);
            }
        } else {
            causes.push((vec![scope], message));
        }
    }
    join(
        causes
            .into_iter()
            .map(|(scopes, message)| format!("{}：{message}", scopes.join("、"))),
    )
}
#[cfg(test)]
mod tests {
    #[test]
    fn parallel_failures_share_causes_and_preserve_distinct_recovery_results() {
        assert_eq!(
            super::scoped([
                ("出口请求", "HTTP 连接失败".into()),
                ("出口 IP", "HTTP 连接失败".into())
            ]),
            "出口请求、出口 IP：HTTP 连接失败"
        );
        assert_eq!(
            super::join(["请求失败", "请求失败", "旧配置恢复失败"]),
            "请求失败；旧配置恢复失败"
        );
        assert_eq!(
            super::join(["用户文字：a；b", "用户文字：a；b"]),
            "用户文字：a；b"
        );
    }
    #[test]
    fn settings_recovery_groups_shared_cause_without_hiding_runtime_failure() {
        let reason = std::io::Error::from_raw_os_error(5).to_string();
        assert_eq!(
            super::scoped([
                ("应用设置", reason.clone()),
                ("恢复旧设置", reason.clone()),
                ("恢复旧内核配置", "控制通道已关闭".into()),
                ("恢复来源", reason.clone()),
            ]),
            format!("应用设置、恢复旧设置、恢复来源：{reason}；恢复旧内核配置：控制通道已关闭")
        );
    }
}
