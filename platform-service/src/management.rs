//! Unprivileged preparation and privileged publication share one installation contract.
#[cfg(any(feature = "client", test))]
use anyhow::bail;
use anyhow::{Context as _, Result};
use sha2::{Digest as _, Sha256};
use std::{
    io::Read as _,
    path::{Path, PathBuf},
    process::{Command, Output},
};

#[cfg(any(feature = "client", test))]
use std::ffi::OsString;

#[derive(Debug, Clone)]
pub struct CoreSource {
    pub name: String,
    pub path: PathBuf,
}

pub const FIREWALL_WARNING_EXIT_CODE: u32 = 2;
const INSTALLATION_SUCCESS_PREFIX: &str = "SUKI_INSTALLATION_RESULT=";

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct InstallationOutcome {
    pub warnings: Vec<InstallationWarning>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum InstallationWarning {
    Firewall,
}
impl std::fmt::Display for InstallationWarning {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Firewall => {
                formatter.write_str("内核已安装，但防火墙规则配置未完成，请重建防火墙规则")
            }
        }
    }
}

const INSTALLATION_FAILURE_PREFIX: &str = "CLASH_VERGE_INSTALLATION_FAILURE_V1=";

/// The installer's final inspection, preserved across the preparation subprocess.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct InstallationVerificationError {
    pub status: crate::InstallationStatus,
}

impl std::fmt::Display for InstallationVerificationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "installation finished without satisfying the requested service and cores"
        )?;
        for core in &self.status.cores {
            match &core.availability {
                crate::CoreAvailability::Ready => {}
                crate::CoreAvailability::Missing => write!(formatter, "; {}: missing", core.name)?,
                crate::CoreAvailability::DigestMismatch => {
                    write!(formatter, "; {}: digest mismatch", core.name)?
                }
                crate::CoreAvailability::Rejected { reason } => {
                    write!(formatter, "; {}: rejected: {reason}", core.name)?;
                }
            }
        }
        Ok(())
    }
}

impl std::error::Error for InstallationVerificationError {}

#[cfg(any(feature = "client", test))]
impl InstallationVerificationError {
    fn write_report(&self, mut output: impl std::io::Write) -> Result<()> {
        output.write_all(INSTALLATION_FAILURE_PREFIX.as_bytes())?;
        serde_json::to_writer(&mut output, self)?;
        output.write_all(b"\n")?;
        Ok(())
    }
}

#[cfg(any(feature = "client", test))]
fn write_installation_result(warning: bool, mut output: impl std::io::Write) -> Result<()> {
    let outcome = InstallationOutcome {
        warnings: if warning {
            vec![InstallationWarning::Firewall]
        } else {
            Vec::new()
        },
    };
    output.write_all(INSTALLATION_SUCCESS_PREFIX.as_bytes())?;
    serde_json::to_writer(&mut output, &outcome)?;
    output.write_all(b"\n")?;
    Ok(())
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let size = file.read(&mut buffer)?;
        if size == 0 {
            break;
        }
        hash.update(&buffer[..size]);
    }
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

pub fn install(
    service: &Path,
    cores: &[CoreSource],
    core_only: bool,
    gid: Option<u32>,
    prompt: &str,
) -> Result<InstallationOutcome> {
    let mut command = Command::new(service);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    }
    command.arg("--prepare-install");
    if core_only {
        command.arg("--core-only");
    }
    if let Some(gid) = gid {
        command.args(["--gid", &gid.to_string()]);
    }
    command.args(["--prompt", prompt]);
    for core in cores {
        command.arg("--core").arg(&core.name).arg(&core.path);
    }
    let output = command
        .output()
        .context("failed to launch service installation")?;
    installation_result(output)
}

#[cfg(feature = "client")]
pub fn uninstall(service: &Path) -> Result<()> {
    let staging = Staging::new()?;
    let service = stage_service(service, &staging)?;
    let arguments = [OsString::from("--uninstall")];
    #[cfg(unix)]
    let warning = elevate(
        &service,
        &arguments,
        unsafe { platform_lib::getegid() },
        "卸载 Suki Clash 后台服务",
    )?;
    #[cfg(windows)]
    let warning = elevate(&service, &arguments, "卸载 Suki Clash 后台服务")?;
    anyhow::ensure!(
        !warning,
        "uninstaller returned an installation warning code"
    );
    Ok(())
}

fn installation_result(output: Output) -> Result<InstallationOutcome> {
    if output.status.success() {
        let encoded = output
            .stdout
            .split(|byte| *byte == b'\n')
            .find_map(|line| line.strip_prefix(INSTALLATION_SUCCESS_PREFIX.as_bytes()))
            .context("installer finished without its structured result")?;
        return serde_json::from_slice(encoded).context("installer returned an invalid result");
    }
    for line in output.stdout.split(|byte| *byte == b'\n') {
        if let Some(report) = line.strip_prefix(INSTALLATION_FAILURE_PREFIX.as_bytes())
            && let Ok(failure) = serde_json::from_slice::<InstallationVerificationError>(report)
        {
            return Err(failure.into());
        }
    }
    bail!(
        "service installation failed: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[cfg(any(feature = "client", test))]
struct Staging(PathBuf);
#[cfg(any(feature = "client", test))]
impl Drop for Staging {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[cfg(any(feature = "client", test))]
impl Staging {
    fn new() -> Result<Self> {
        for index in 0..100 {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "suki-clash-install-{}-{nonce}-{index}",
                std::process::id()
            ));
            let builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            let mut builder = builder;
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt as _;
                builder.mode(0o700);
            }
            match builder.create(&root) {
                Ok(()) => return Ok(Self(root)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
        }
        bail!("could not create installation staging directory")
    }
}

#[cfg(feature = "client")]
fn stage_service(service: &Path, staging: &Staging) -> Result<PathBuf> {
    let target = staging
        .0
        .join(service.file_name().context("service has no name")?);
    std::fs::copy(service, &target).context("failed to stage the service executable")?;
    Ok(target)
}

#[cfg(any(feature = "client", test))]
struct PreparedCores {
    staging: Staging,
    requirements: Vec<crate::CoreRequirement>,
    arguments: Vec<OsString>,
}

#[cfg(any(feature = "client", test))]
impl PreparedCores {
    fn new(cores: &[CoreSource], core_only: bool) -> Result<Self> {
        let staging = Staging::new()?;
        let mut requirements = Vec::new();
        let mut elevated: Vec<OsString> = vec!["--install".into()];
        if !core_only {
            elevated.push("--install-service".into());
        }
        for core in cores {
            anyhow::ensure!(
                [
                    format!("mihomo{}", std::env::consts::EXE_SUFFIX),
                    format!("mihomo-alpha{}", std::env::consts::EXE_SUFFIX)
                ]
                .contains(&core.name),
                "unsupported core name {}",
                core.name
            );
            anyhow::ensure!(
                core.path.is_file(),
                "requested core {} is unavailable at {}",
                core.name,
                core.path.display()
            );
            let target = staging.0.join(&core.name);
            anyhow::ensure!(!target.exists(), "duplicate core name {}", core.name);
            std::fs::copy(&core.path, &target)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))?;
            }
            let digest = sha256_file(&target)?;
            elevated.extend([
                "--install-core".into(),
                target.into_os_string(),
                "--sha256".into(),
                digest.clone().into(),
            ]);
            requirements.push(crate::CoreRequirement {
                name: core.name.clone(),
                sha256: Some(digest),
            });
        }
        anyhow::ensure!(
            !requirements.is_empty(),
            "no core executable is available; run prebuild or restore the bundled cores"
        );
        Ok(Self {
            staging,
            requirements,
            arguments: elevated,
        })
    }
}

/// Called by the service executable before starting its daemon runtime.
#[cfg(feature = "client")]
pub fn prepare_install(mut arguments: impl Iterator<Item = OsString>) -> Result<()> {
    let mut cores = Vec::new();
    let mut ensure = false;
    let mut core_only = false;
    let mut prompt = "Install Suki Clash service and approved cores".to_owned();
    #[cfg(unix)]
    let mut gid = unsafe { platform_lib::getgid() };
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--ensure") => ensure = true,
            Some("--core-only") => core_only = true,
            Some("--core") => {
                let name = arguments
                    .next()
                    .context("--core requires a name and path")?
                    .into_string()
                    .map_err(|_| anyhow::anyhow!("invalid core name"))?;
                let path = arguments.next().context("--core requires a path")?.into();
                cores.push(CoreSource { name, path });
            }
            Some("--prompt") => {
                prompt = arguments
                    .next()
                    .context("--prompt requires text")?
                    .to_string_lossy()
                    .into_owned()
            }
            Some("--gid") => {
                let value = arguments
                    .next()
                    .context("--gid requires an ID")?
                    .to_string_lossy()
                    .parse::<u32>()?;
                #[cfg(unix)]
                {
                    gid = value;
                }
                #[cfg(windows)]
                let _ = value;
            }
            _ => bail!("unknown preparation argument: {argument:?}"),
        }
    }
    let service = std::env::current_exe()?;
    let PreparedCores {
        staging,
        requirements,
        arguments: elevated,
    } = PreparedCores::new(&cores, core_only)?;
    let service_digest = if core_only {
        None
    } else {
        Some(sha256_file(&service)?)
    };
    let runtime = tokio::runtime::Runtime::new()?;
    let verified = |status: &crate::InstallationStatus| {
        status.satisfies(&requirements)
            && service_digest
                .as_ref()
                .is_none_or(|digest| digest == &status.service_sha256)
    };
    if ensure {
        match runtime.block_on(crate::client::inspect_installation_with_digest(
            &requirements,
            !core_only,
        )) {
            Ok(status) if verified(&status) => {
                write_installation_result(false, std::io::stdout().lock())?;
                return Ok(());
            }
            Ok(_) => {}
            Err(error) if crate::client::is_unreachable(&error) => {}
            Err(error) => return Err(error),
        }
    }
    #[cfg(windows)]
    let elevated = {
        let mut arguments = elevated;
        if ensure && !core_only {
            arguments.push("--ensure-service".into());
        }
        arguments
    };
    // One executable performs maintenance and becomes the installed daemon.
    // A temporary copy also avoids macOS TCC paths and Windows image locks.
    let staged_service = stage_service(&service, &staging)?;
    #[cfg(unix)]
    let warning = elevate(&staged_service, &elevated, gid, &prompt)?;
    #[cfg(windows)]
    let warning = elevate(&staged_service, &elevated, &prompt)?;
    // Core-only publication is verified by the privileged installer and must also work offline.
    if core_only {
        write_installation_result(warning, std::io::stdout().lock())?;
        return Ok(());
    }
    let status = runtime
        .block_on(crate::client::inspect_installation_with_digest(
            &requirements,
            true,
        ))
        .context("installation finished but its approved cores could not be verified")?;
    if !verified(&status) {
        let failure = InstallationVerificationError { status };
        failure.write_report(std::io::stdout().lock())?;
        return Err(failure.into());
    }
    write_installation_result(warning, std::io::stdout().lock())?;
    Ok(())
}

#[cfg(any(all(feature = "client", target_os = "macos"), test))]
fn shell_quote(value: &std::ffi::OsStr) -> String {
    format!("'{}'", value.to_string_lossy().replace('\'', "'\\''"))
}

#[cfg(all(unix, feature = "client"))]
fn elevate(installer: &Path, arguments: &[OsString], gid: u32, prompt: &str) -> Result<bool> {
    let status = if unsafe { platform_lib::geteuid() } == 0 {
        Command::new(installer)
            .args(arguments)
            .env("CLASH_VERGE_SERVICE_GID", gid.to_string())
            .status()?
    } else {
        #[cfg(target_os = "macos")]
        {
            let mut shell = format!(
                "cd /; CLASH_VERGE_SERVICE_GID={gid} {}",
                shell_quote(installer.as_os_str())
            );
            for argument in arguments {
                shell.push(' ');
                shell.push_str(&shell_quote(argument));
            }
            let script = "on run argv\ndo shell script (item 1 of argv) with administrator privileges with prompt (item 2 of argv)\nend run";
            Command::new("osascript")
                .args(["-e", script, &shell, prompt])
                .status()?
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = prompt;
            match Command::new("pkexec")
                .arg(installer)
                .args(arguments)
                .status()
            {
                Ok(status) if status.code() == Some(127) => Command::new("sudo")
                    .arg(installer)
                    .args(arguments)
                    .status()?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Command::new("sudo")
                    .arg(installer)
                    .args(arguments)
                    .status()?,
                result => result?,
            }
        }
    };
    anyhow::ensure!(
        status.success() || status.code() == Some(FIREWALL_WARNING_EXIT_CODE as i32),
        "elevated installer failed with {status}"
    );
    Ok(status.code() == Some(FIREWALL_WARNING_EXIT_CODE as i32))
}

#[cfg(all(windows, feature = "client"))]
fn elevate(installer: &Path, arguments: &[OsString], _prompt: &str) -> Result<bool> {
    use std::ffi::OsStr;
    use std::os::windows::{
        ffi::OsStrExt as _,
        io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle},
    };
    use windows_sys::Win32::{
        System::{
            Com::{COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx},
            Threading::{GetExitCodeProcess, INFINITE, WaitForSingleObject},
        },
        UI::{
            Shell::{
                SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
                ShellExecuteExW,
            },
            WindowsAndMessaging::SW_HIDE,
        },
    };
    let wide = |value: &OsStr| value.encode_wide().chain([0]).collect::<Vec<_>>();
    let command_line = arguments
        .iter()
        .map(|arg| windows_quote(&arg.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(" ");
    let verb = wide(OsStr::new("runas"));
    let file = wide(installer.as_os_str());
    let parameters = wide(OsStr::new(&command_line));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpVerb: verb.as_ptr(),
        lpFile: file.as_ptr(),
        lpParameters: parameters.as_ptr(),
        nShow: SW_HIDE,
        ..Default::default()
    };
    // ShellExecuteExW may delegate the verb to COM shell extensions.
    unsafe {
        CoInitializeEx(
            std::ptr::null(),
            (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
        )
    };
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        return Err(std::io::Error::last_os_error())
            .context("failed to elevate the service installer");
    }
    let process = info.hProcess;
    anyhow::ensure!(
        !process.is_null(),
        "elevation did not start the service installer"
    );
    let process = unsafe { OwnedHandle::from_raw_handle(process) };
    unsafe { WaitForSingleObject(process.as_raw_handle(), INFINITE) };
    let mut code = 0;
    if unsafe { GetExitCodeProcess(process.as_raw_handle(), &mut code) } == 0 {
        return Err(std::io::Error::last_os_error())
            .context("cannot read the elevated installer's exit code");
    }
    anyhow::ensure!(
        code == 0 || code == FIREWALL_WARNING_EXIT_CODE,
        "elevated installer failed with exit code {code}"
    );
    Ok(code == FIREWALL_WARNING_EXIT_CODE)
}

#[cfg(any(all(windows, feature = "client"), test))]
fn windows_quote(value: &str) -> String {
    let mut result = String::from("\"");
    let mut slashes = 0;
    for c in value.chars() {
        if c == '\\' {
            slashes += 1;
            continue;
        }
        result.extend(std::iter::repeat_n(
            '\\',
            if c == '"' { slashes * 2 + 1 } else { slashes },
        ));
        slashes = 0;
        result.push(c);
    }
    result.extend(std::iter::repeat_n('\\', slashes * 2));
    result.push('"');
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installation_warning_is_preserved_without_turning_success_into_failure() -> Result<()> {
        #[cfg(unix)]
        use std::os::unix::process::ExitStatusExt as _;
        #[cfg(windows)]
        use std::os::windows::process::ExitStatusExt as _;
        let mut stdout = b"installer diagnostic\n".to_vec();
        write_installation_result(true, &mut stdout)?;
        let result = installation_result(Output {
            status: std::process::ExitStatus::from_raw(0),
            stdout,
            stderr: Vec::new(),
        })?;
        assert!(matches!(
            result.warnings.as_slice(),
            [InstallationWarning::Firewall]
        ));
        assert!(
            installation_result(Output {
                status: std::process::ExitStatus::from_raw(0),
                stdout: Vec::new(),
                stderr: Vec::new()
            })
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn installation_refusal_survives_subprocess_output_and_error_context() -> Result<()> {
        #[cfg(unix)]
        use std::os::unix::process::ExitStatusExt as _;
        #[cfg(windows)]
        use std::os::windows::process::ExitStatusExt as _;

        let reason = "path C:\\用户\\core is writable by Everyone\npermissions rejected";
        let failure = InstallationVerificationError {
            status: crate::InstallationStatus {
                service_sha256: "service digest".into(),
                protocol: crate::ProtocolInfo::current(),
                cores: vec![crate::CoreInspection {
                    name: "mihomo.exe".into(),
                    availability: crate::CoreAvailability::Rejected {
                        reason: reason.into(),
                    },
                }],
                core_busy: true,
            },
        };
        let mut stdout = b"installer diagnostics\n".to_vec();
        write_installation_result(false, &mut stdout)?;
        failure.write_report(&mut stdout)?;
        assert!(
            installation_result(Output {
                status: std::process::ExitStatus::from_raw(0),
                stdout: stdout.clone(),
                stderr: Vec::new(),
            })
            .is_ok()
        );
        let result = installation_result(Output {
            status: std::process::ExitStatus::from_raw(1),
            stdout,
            stderr:
                b"Error: installation finished without satisfying the requested service and cores"
                    .to_vec(),
        })
        .context("install service failed");
        let error = result.expect_err("a rejected core must not be reported as installed");
        let report = error
            .downcast_ref::<InstallationVerificationError>()
            .expect("installation must preserve its structured inspection");
        assert_eq!(report.status.cores, failure.status.cores);
        assert_eq!(report.status.service_sha256, failure.status.service_sha256);
        assert!(report.status.core_busy);
        assert!(format!("{error:#}").contains(reason));
        Ok(())
    }

    #[test]
    fn unstructured_installer_failures_keep_their_diagnostics() {
        #[cfg(unix)]
        use std::os::unix::process::ExitStatusExt as _;
        #[cfg(windows)]
        use std::os::windows::process::ExitStatusExt as _;

        for stdout in [
            "installer output without a structured result".to_owned(),
            format!("{INSTALLATION_FAILURE_PREFIX}invalid json"),
        ] {
            let error = installation_result(Output {
                status: std::process::ExitStatus::from_raw(1),
                stdout: stdout.as_bytes().to_vec(),
                stderr: b"elevation cancelled".to_vec(),
            })
            .expect_err("failed installers must remain errors");
            assert!(
                error
                    .downcast_ref::<InstallationVerificationError>()
                    .is_none()
            );
            assert!(error.to_string().contains(&stdout));
            assert!(error.to_string().contains("elevation cancelled"));
        }
    }

    #[test]
    fn preparation_attests_staged_bytes_before_elevation() -> Result<()> {
        let source = Staging::new()?;
        let name = format!("mihomo{}", std::env::consts::EXE_SUFFIX);
        let path = source.0.join("core with spaces and 'quotes'");
        std::fs::write(&path, b"abc")?;
        let plan = PreparedCores::new(
            &[CoreSource {
                name: name.clone(),
                path: path.clone(),
            }],
            false,
        )?;
        std::fs::write(&path, b"changed source")?;
        assert_eq!(plan.requirements.len(), 1);
        assert_eq!(
            plan.requirements[0].sha256.as_deref(),
            Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        );
        assert_eq!(std::fs::read(plan.staging.0.join(&name))?, b"abc");
        assert_eq!(plan.arguments[0], "--install");
        assert_eq!(plan.arguments[1], "--install-service");
        assert_eq!(plan.arguments[2], "--install-core");
        assert_eq!(plan.arguments[4], "--sha256");
        assert_eq!(plan.arguments.len(), 6);
        let core_only = PreparedCores::new(
            &[CoreSource {
                name: name.clone(),
                path: path.clone(),
            }],
            true,
        )?;
        assert_eq!(core_only.arguments[0], "--install");
        assert_eq!(core_only.arguments[1], "--install-core");
        assert!(
            !core_only
                .arguments
                .iter()
                .any(|arg| arg == "--install-service")
        );
        let staged = plan.staging.0.clone();
        drop(plan);
        assert!(!staged.exists());
        assert!(PreparedCores::new(&[], false).is_err());
        assert!(
            PreparedCores::new(
                &[CoreSource {
                    name,
                    path: source.0.join("missing")
                }],
                false
            )
            .is_err()
        );
        Ok(())
    }

    #[cfg(feature = "client")]
    #[test]
    fn maintenance_stages_one_service_copy_independent_of_the_source() -> Result<()> {
        let source = Staging::new()?;
        let executable = source.0.join(format!(
            "suki-clash-service{}",
            std::env::consts::EXE_SUFFIX
        ));
        std::fs::write(&executable, b"service and maintenance")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755))?;
        }
        let staged = Staging::new()?;
        let target = stage_service(&executable, &staged)?;
        std::fs::remove_file(&executable)?;
        assert_eq!(std::fs::read(&target)?, b"service and maintenance");
        assert_eq!(std::fs::read_dir(&staged.0)?.count(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_ne!(std::fs::metadata(&target)?.permissions().mode() & 0o111, 0);
        }
        let directory = staged.0.clone();
        drop(staged);
        assert!(!directory.exists());
        Ok(())
    }

    #[test]
    fn elevation_quotes_shell_and_windows_arguments() {
        assert_eq!(
            shell_quote(std::ffi::OsStr::new("a'b $HOME")),
            "'a'\\''b $HOME'"
        );
        assert_eq!(windows_quote(r#"C:\core's path\"#), r#""C:\core's path\\""#);
        assert_eq!(windows_quote(r#"a"b"#), r#""a\"b""#);
    }
}
