//! CV's per-user logon tasks; elevated apps use HighestAvailable, ordinary apps LeastPrivilege.
use std::os::windows::process::CommandExt as _;
use std::process::{Command, Output};
const USER: &str = "Suki Clash";
const ADMIN: &str = "Suki Clash (Admin)";
fn run(args: &[&str]) -> Result<Output, String> {
    Command::new("schtasks.exe")
        .args(args)
        .creation_flags(0x08000000)
        .output()
        .map_err(|error| error.to_string())
}
fn decode(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xff, 0xfe]) {
        return String::from_utf16_lossy(
            &bytes[2..]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_le_bytes(*pair))
                .collect::<Vec<_>>(),
        );
    }
    use windows_sys::Win32::Globalization::{GetOEMCP, MultiByteToWideChar};
    let mut wide = vec![0; bytes.len()];
    let size = unsafe {
        MultiByteToWideChar(
            GetOEMCP(),
            0,
            bytes.as_ptr(),
            bytes.len() as i32,
            wide.as_mut_ptr(),
            wide.len() as i32,
        )
    };
    String::from_utf16_lossy(&wide[..size.max(0) as usize])
}
fn tasks() -> Result<String, String> {
    let output = run(&["/Query", "/FO", "CSV", "/NH"])?;
    if !output.status.success() {
        return Err(format!("无法读取自启动任务：{}", decode(&output.stderr)));
    }
    Ok(decode(&output.stdout))
}
fn exists(list: &str, name: &str) -> bool {
    list.lines()
        .any(|line| line.starts_with(&format!("\"\\{name}\",")))
}
pub(super) fn enabled() -> Result<bool, String> {
    let list = tasks()?;
    for name in [USER, ADMIN] {
        if exists(&list, name) {
            let output = run(&["/Query", "/TN", name, "/XML"])?;
            if !output.status.success() {
                return Err(decode(&output.stderr));
            }
            if !decode(&output.stdout).contains("<Enabled>false</Enabled>") {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
fn delete(name: &str) -> Result<(), String> {
    let output = run(&["/Delete", "/TN", name, "/F"])?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!("无法删除自启动任务：{}", decode(&output.stderr)))
    }
}
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
pub(crate) struct Checkpoint(Vec<(String, String)>);
pub(super) fn checkpoint() -> Result<Checkpoint, String> {
    let list = tasks()?;
    let mut saved = Vec::new();
    for name in [USER, ADMIN] {
        if exists(&list, name) {
            let output = run(&["/Query", "/TN", name, "/XML"])?;
            if !output.status.success() {
                return Err(decode(&output.stderr));
            }
            saved.push((name.into(), decode(&output.stdout)));
        }
    }
    Ok(Checkpoint(saved))
}
fn register(name: &str, xml: &str) -> Result<(), String> {
    // Close the writer before Task Scheduler opens the XML in another process.
    let file = tempfile::NamedTempFile::new()
        .map_err(|error| error.to_string())?
        .into_temp_path();
    let bytes = [
        vec![0xff, 0xfe],
        xml.encode_utf16().flat_map(u16::to_le_bytes).collect(),
    ]
    .concat();
    std::fs::write(&file, bytes).map_err(|error| error.to_string())?;
    let output = run(&[
        "/Create",
        "/TN",
        name,
        "/XML",
        file.to_str().ok_or("任务路径无效")?,
        "/F",
    ])?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!("无法写入自启动任务：{}", decode(&output.stderr)))
    }
}
impl Checkpoint {
    pub(crate) fn restore(&self) -> Result<(), String> {
        let list = tasks()?;
        for (name, xml) in &self.0 {
            register(name, xml)?;
        }
        for name in [USER, ADMIN] {
            if exists(&list, name) && !self.0.iter().any(|(saved, _)| saved == name) {
                delete(name)?;
            }
        }
        Ok(())
    }
}
pub(super) fn apply(on: bool) -> Result<(), String> {
    let list = tasks()?;
    let elevated = crate::platform_access::elevated()?;
    if !elevated && exists(&list, ADMIN) {
        return Err("管理员自启动任务已存在，请以管理员身份修改自启动".into());
    }
    if !on {
        for name in [USER, ADMIN] {
            if exists(&list, name) {
                delete(name)?;
            }
        }
        if enabled()? {
            return Err("自启动关闭后回读不一致".into());
        }
        return Ok(());
    }
    let name = if elevated { ADMIN } else { USER };
    let other = if elevated { USER } else { ADMIN };
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let user =
        match crate::core::service_identity_for_autostart().map_err(|error| error.to_string())? {
            suki_service::OwnerIdentity::Windows { sid } => sid,
            _ => return Err("无法读取 Windows 用户身份".into()),
        };
    let level = if elevated {
        "HighestAvailable"
    } else {
        "LeastPrivilege"
    };
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-16"?><Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task"><Triggers><LogonTrigger><Enabled>true</Enabled><UserId>{user}</UserId></LogonTrigger></Triggers><Principals><Principal id="Author"><UserId>{user}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>{level}</RunLevel></Principal></Principals><Settings><MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy><DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries><StopIfGoingOnBatteries>false</StopIfGoingOnBatteries><ExecutionTimeLimit>PT0S</ExecutionTimeLimit><Enabled>true</Enabled></Settings><Actions Context="Author"><Exec><Command>{}</Command><Arguments>--autostart</Arguments></Exec></Actions></Task>"#,
        escape(&exe.to_string_lossy())
    );
    register(name, &xml)?;
    if exists(&list, other) {
        delete(other)?;
    }
    if !enabled()? {
        return Err("自启动任务回读不一致".into());
    }
    Ok(())
}
