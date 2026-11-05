//! The service executable owns installation, removal and daemon startup dispatch.
mod install;
mod shared;
mod uninstall;

use anyhow::{Result, bail, ensure};
use std::ffi::OsString;

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Run,
    PrepareInstall(Vec<OsString>),
    Install(Vec<OsString>),
    Uninstall { debug: bool },
    CleanupOwners,
    Help,
    Version,
}

fn parse(arguments: impl IntoIterator<Item = OsString>) -> Result<Command> {
    let mut arguments = arguments.into_iter();
    let Some(command) = arguments.next() else {
        return Ok(Command::Run);
    };
    let remaining: Vec<_> = arguments.collect();
    match command.to_str() {
        Some("--prepare-install") => Ok(Command::PrepareInstall(remaining)),
        Some("--install") => Ok(Command::Install(remaining)),
        Some("--uninstall") => {
            ensure!(
                remaining.iter().all(|arg| arg == "--debug"),
                "unknown uninstall argument"
            );
            Ok(Command::Uninstall {
                debug: !remaining.is_empty(),
            })
        }
        Some("--cleanup-stale-owners") if remaining.is_empty() => Ok(Command::CleanupOwners),
        Some("--help" | "-h") if remaining.is_empty() => Ok(Command::Help),
        Some("--version" | "-V") if remaining.is_empty() => Ok(Command::Version),
        _ => bail!("unknown service command: {command:?}; use --help"),
    }
}

/// Maintenance must finish before creating a runtime, opening logs or acquiring
/// the daemon owner lock. Installation preparation runs its own IPC runtime.
pub(super) fn run_if_requested() -> Result<bool> {
    match parse(std::env::args_os().skip(1))? {
        Command::Run => return Ok(false),
        Command::PrepareInstall(arguments) => {
            suki_clash_service::management::prepare_install(arguments.into_iter())?
        }
        Command::Install(arguments) => {
            if install::run(arguments.into_iter())? {
                std::process::exit(
                    suki_clash_service::management::FIREWALL_WARNING_EXIT_CODE as i32,
                );
            }
        }
        Command::Uninstall { debug } => uninstall::run(debug)?,
        Command::CleanupOwners => {
            let removed = suki_clash_service::cleanup_stale_owner_state()?;
            println!("Removed {} stale owner state directories", removed.len());
        }
        Command::Help => println!(
            "Suki Clash service\n\nWithout arguments: run the service.\n--install [options]  Install the service and approved cores (administrator).\n--uninstall [--debug]  Stop and remove the service (administrator).\n--prepare-install [options]  Stage approved cores and request installation authorization.\n--cleanup-stale-owners  Remove inactive owner records.\n--version  Show package and protocol versions."
        ),
        Command::Version => println!(
            "Suki Clash service {} (protocol {:?})",
            env!("CARGO_PKG_VERSION"),
            suki_clash_service::ProtocolVersion::current()
        ),
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(arguments: &[&str]) -> Result<Command> {
        parse(arguments.iter().map(OsString::from))
    }

    #[test]
    fn maintenance_commands_never_fall_through_to_daemon_startup() {
        assert_eq!(command(&[]).unwrap(), Command::Run);
        assert_eq!(
            command(&["--install"]).unwrap(),
            Command::Install(Vec::new())
        );
        assert_eq!(
            command(&["--install", "--install-core", "/tmp/core with spaces"]).unwrap(),
            Command::Install(vec![
                "--install-core".into(),
                "/tmp/core with spaces".into()
            ])
        );
        assert_eq!(
            command(&["--uninstall"]).unwrap(),
            Command::Uninstall { debug: false }
        );
        assert_eq!(
            command(&["--uninstall", "--debug"]).unwrap(),
            Command::Uninstall { debug: true }
        );
        assert_eq!(
            command(&["--prepare-install", "--core-only"]).unwrap(),
            Command::PrepareInstall(vec!["--core-only".into()])
        );
        for args in [
            vec!["--uninstal"],
            vec!["--uninstall", "--install"],
            vec!["--help", "--install"],
            vec!["--cleanup-stale-owners", "--install"],
        ] {
            assert!(command(&args).is_err());
        }
    }
}
