//! Bounded JavaScript evaluation. No filesystem, networking, process or GPUI APIs.
use rquickjs::{Context, Function, Runtime};
use serde::{Deserialize, Serialize};
use serde_yaml_ng::Mapping;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};
const MAX_LOGS: usize = 256;
const MAX_LOG_BYTES: usize = 64 * 1024;
const MEMORY_LIMIT: usize = 64 * 1024 * 1024;
const TIME_LIMIT: Duration = Duration::from_secs(5);
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Report {
    pub(crate) id: u64,
    pub(crate) at: String,
    pub(crate) error: Option<String>,
    pub(crate) logs: Vec<(String, String)>,
}
pub(crate) fn validate(source: &str) -> Result<(), String> {
    if source.len() as u64 > super::INPUT_LIMIT {
        return Err("脚本超过大小上限".into());
    }
    let runtime = Runtime::new()
        .map_err(|error| crate::logging::diagnostic_error("无法创建脚本运行时", error))?;
    runtime.set_memory_limit(MEMORY_LIMIT);
    runtime.set_max_stack_size(512 * 1024);
    let deadline = Instant::now() + TIME_LIMIT;
    runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)));
    let context = Context::full(&runtime)
        .map_err(|error| crate::logging::diagnostic_error("无法创建脚本上下文", error))?;
    context.with(|ctx| {
        ctx.eval::<Function, _>(format!("(function(config,name,__write) {{\n{source}\n}})"))
            .map(|_| ())
            .map_err(|error| crate::logging::rejected("JavaScript 语法无效".into(), error))
    })
}
#[cfg(test)]
pub(crate) fn run(
    id: u64,
    source: &str,
    config: &Mapping,
    name: &str,
) -> (Result<Mapping, String>, Report) {
    run_with_budget(id, source, config, name, TIME_LIMIT, None)
}
pub(crate) fn run_cancellable(
    id: u64,
    source: &str,
    config: &Mapping,
    name: &str,
    cancellation: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
) -> (Result<Mapping, String>, Report) {
    run_with_budget(id, source, config, name, TIME_LIMIT, cancellation)
}
fn run_with_budget(
    id: u64,
    source: &str,
    config: &Mapping,
    name: &str,
    budget: Duration,
    cancellation: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
) -> (Result<Mapping, String>, Report) {
    let logs = Rc::new(RefCell::new(Vec::<(String, String)>::new()));
    let overflow = Rc::new(Cell::new(false));
    let deadline = Instant::now() + budget;
    let mut result = (|| -> Result<Mapping, String> {
        if source.len() as u64 > super::INPUT_LIMIT {
            return Err("脚本超过大小上限".into());
        }
        let input = serde_json::to_vec(config)
            .map_err(|error| crate::logging::rejected("脚本输入不能转换为 JSON", error))?;
        if input.len() as u64 > super::INPUT_LIMIT {
            return Err("脚本输入超过大小上限".into());
        }
        let runtime = Runtime::new()
            .map_err(|error| crate::logging::diagnostic_error("无法创建脚本运行时", error))?;
        runtime.set_memory_limit(MEMORY_LIMIT);
        runtime.set_max_stack_size(512 * 1024);
        let cancelled = cancellation.clone();
        runtime.set_interrupt_handler(Some(Box::new(move || {
            Instant::now() >= deadline
                || cancelled
                    .as_ref()
                    .is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Relaxed))
        })));
        let context = Context::full(&runtime)
            .map_err(|error| crate::logging::diagnostic_error("无法创建脚本上下文", error))?;
        context.with(|ctx| -> Result<Mapping,String> {
            let output_logs=logs.clone(); let exceeded=overflow.clone(); let size=Cell::new(0usize);
            let logger=Function::new(ctx.clone(),move |level:String,text:String| {
                if output_logs.borrow().len()>=MAX_LOGS || size.get().saturating_add(text.len())>MAX_LOG_BYTES { exceeded.set(true); return; }
                size.set(size.get()+text.len()); output_logs.borrow_mut().push((level,text));
            }).map_err(|error| crate::logging::diagnostic_error("无法初始化脚本日志", error))?;
            // Input/name are arguments, never interpolated into executable text.
            let code=format!(r#"(function(config,name,__write) {{
                const console=Object.freeze(Object.fromEntries(['log','info','warn','error','debug','trace','table'].map(level=>[level,(...args)=>__write(level,args.map(value=>typeof value==='string'?value:JSON.stringify(value)).join(' '))])));
                {source}
                const result=main(config,name);
                if (!result || typeof result!=='object' || Array.isArray(result) || typeof result.then==='function') throw new Error('invalid output');
                return JSON.stringify(result);
            }})"#);
            let function:Function=ctx.eval(code).map_err(|error| crate::logging::rejected("JavaScript 语法无效", error))?;
            let input=ctx.json_parse(input).map_err(|error| crate::logging::rejected("脚本输入无效", error))?;
            let output:String=function.call((input,name,logger)).map_err(|error| crate::logging::rejected("JavaScript 执行异常或超过资源预算", error))?;
            if overflow.get() { return Err("脚本日志超过上限".into()); }
            if output.len() as u64 > super::INPUT_LIMIT { return Err("脚本结果超过大小上限".into()); }
            let value:serde_json::Value=serde_json::from_str(&output).map_err(|error| crate::logging::diagnostic_error("脚本结果不是有效 JSON", error))?;
            if !value.is_object() { return Err("脚本必须返回配置对象".into()); }
            serde_json::from_value(value).map_err(|error| crate::logging::rejected("脚本结果不能转换为配置".into(), error))
        })
    })();
    if cancellation
        .as_ref()
        .is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Relaxed))
    {
        result = Err("脚本执行已取消".into());
    }
    if Instant::now() >= deadline {
        result = Err("JavaScript 执行超时".into());
    }
    if overflow.get() {
        result = Err("脚本日志超过上限".into());
    }
    let mut entries = logs.borrow().clone();
    // Console content must not disclose config credentials, even on failure.
    let mut secrets = Vec::new();
    collect_secrets(&serde_yaml_ng::Value::Mapping(config.clone()), &mut secrets);
    if let Ok(output) = &result {
        collect_secrets(&serde_yaml_ng::Value::Mapping(output.clone()), &mut secrets);
    }
    for secret in secrets.clone() {
        if let Ok(encoded) = serde_json::to_string(&secret)
            && encoded.len() > 2
        {
            secrets.push(encoded[1..encoded.len() - 1].to_owned());
        }
    }
    secrets.sort_by_key(|secret| std::cmp::Reverse(secret.len()));
    for (_, line) in &mut entries {
        for secret in &secrets {
            *line = line.replace(secret, "<redacted>");
        }
        *line = crate::logging::redact(line);
    }
    let report = Report {
        id,
        at: chrono::Utc::now().to_rfc3339(),
        error: result.as_ref().err().cloned(),
        logs: entries,
    };
    (result, report)
}
fn collect_secrets(value: &serde_yaml_ng::Value, secrets: &mut Vec<String>) {
    match value {
        serde_yaml_ng::Value::Mapping(map) => {
            for (key, value) in map {
                if key.as_str().is_some_and(|key| {
                    [
                        "password",
                        "secret",
                        "token",
                        "uuid",
                        "private-key",
                        "authentication",
                    ]
                    .contains(&key)
                }) {
                    match value {
                        serde_yaml_ng::Value::String(text) if !text.is_empty() => {
                            secrets.push(text.clone());
                        }
                        serde_yaml_ng::Value::Sequence(values) => {
                            for value in values {
                                if let Some(text) = value.as_str() {
                                    secrets.push(text.into());
                                    if key.as_str() == Some("authentication")
                                        && let Some((_, password)) = text.split_once(':')
                                        && !password.is_empty()
                                    {
                                        secrets.push(password.into());
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                collect_secrets(value, secrets);
            }
        }
        serde_yaml_ng::Value::Sequence(values) => {
            for value in values {
                collect_secrets(value, secrets);
            }
        }
        _ => {}
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scripts_mutate_real_objects_accept_names_and_redact_logs() {
        let config = serde_yaml_ng::from_str("secret: credential\npassword: 'with\"quote'\nauthentication: ['user:auth-pass']\nrules: []").unwrap();
        let (result, report) = run(
            7,
            "function main(c,n){console.info(c.secret);console.info(c);console.info(c.authentication[0].split(':')[1]); c.name=n; c.rules.push('MATCH,DIRECT');return c;}",
            &config,
            "Quote'\\Name",
        );
        assert_eq!(result.unwrap()["name"].as_str(), Some("Quote'\\Name"));
        assert_eq!(report.logs[0].1, "<redacted>");
        assert!(!report.logs[1].1.contains("with"));
        assert_eq!(report.logs[2].1, "<redacted>");
    }
    #[test]
    fn actual_interrupt_stops_infinite_loops_even_when_exception_is_caught() {
        let start = Instant::now();
        let (result, _) = run_with_budget(
            1,
            "function main(c){for(;;){try {while(true){}}catch(e){}}}",
            &Mapping::new(),
            "",
            Duration::from_millis(30),
            None,
        );
        assert!(result.is_err());
        assert!(start.elapsed() < Duration::from_secs(2));
    }
    #[test]
    fn invalid_outputs_exceptions_and_log_overflow_fail_instead_of_returning_input() {
        for script in [
            "function main(){return null}",
            "function main(){return []}",
            "function main(){throw new Error('sensitive')}",
            "async function main(c){return c}",
            "function main(c){for(let i=0;i<300;i++)console.log('x');return c;}",
            "function main(c){c.self=c;return c;}",
            "function main(c){c.buffer=new ArrayBuffer(128*1024*1024);return c;}",
        ] {
            let (result, report) = run(1, script, &Mapping::new(), "");
            assert!(result.is_err(), "{script}");
            assert!(!report.error.unwrap().contains("sensitive"));
        }
        assert!(validate("function main(").is_err());
        let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let (result, _) = run_cancellable(
            1,
            "function main(c){while(true){}}",
            &Mapping::new(),
            "",
            Some(cancelled),
        );
        assert_eq!(result.unwrap_err(), "脚本执行已取消");
    }
}
