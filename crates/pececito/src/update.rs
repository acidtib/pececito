//! Checks GitHub for a newer release of Pececito and installs it.

use std::env;
use std::ffi::OsStr;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

const RELEASES: &str = "https://github.com/acidtib/pececito/releases";
const INSTALL_SCRIPT: &str = include_str!("../../../script/install.sh");

pub enum Update {
    Unchecked,
    Checking(Receiver<Result<String, String>>),
    UpToDate,
    /// A newer release, by version.
    Available(String),
    Installing(String, Receiver<Result<(), String>>),
    /// The browser is opening the release's page, for a copy that can't replace itself.
    Opening(String, Receiver<Result<(), String>>),
    /// The new release is on disk; this process is still the old one.
    Installed(String),
    Failed(String),
}

impl Update {
    /// Does what the Updates button offers: checks, or gets the release that a check found.
    /// `wake` runs on a worker thread once the work is done.
    pub fn advance(&mut self, wake: impl FnOnce() + Send + 'static) {
        match self {
            Update::Unchecked | Update::UpToDate | Update::Failed(_) => {
                *self = Update::Checking(run(latest_version, wake));
            }
            Update::Available(version) => {
                let version = version.clone();
                let wanted = version.clone();
                *self = if installs_itself() {
                    Update::Installing(version, run(move || install(&wanted), wake))
                } else {
                    let page = format!("{RELEASES}/tag/v{wanted}");
                    Update::Opening(version, run(move || open(OsStr::new(&page)), wake))
                };
            }
            Update::Checking(_)
            | Update::Installing(..)
            | Update::Opening(..)
            | Update::Installed(_) => {}
        }
    }

    /// Takes in finished work; returns whether any finished.
    pub fn finish(&mut self) -> bool {
        let next = match self {
            Update::Checking(result) => match received(result) {
                None => return false,
                Some(Ok(latest)) if is_newer(&latest, env!("CARGO_PKG_VERSION")) => {
                    Update::Available(latest)
                }
                Some(Ok(_)) => Update::UpToDate,
                Some(Err(error)) => Update::Failed(error),
            },
            Update::Installing(version, result) => match received(result) {
                None => return false,
                Some(Ok(())) => Update::Installed(version.clone()),
                Some(Err(error)) => Update::Failed(error),
            },
            Update::Opening(version, result) => match received(result) {
                None => return false,
                Some(Ok(())) => Update::Available(version.clone()),
                Some(Err(error)) => Update::Failed(error),
            },
            _ => return false,
        };
        *self = next;
        true
    }

    pub fn is_busy(&self) -> bool {
        matches!(
            self,
            Update::Checking(_) | Update::Installing(..) | Update::Opening(..)
        )
    }
}

fn run<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
    wake: impl FnOnce() + Send + 'static,
) -> Receiver<Result<T, String>> {
    let (sender, result) = mpsc::channel();
    thread::spawn(move || {
        if sender.send(work()).is_ok() {
            wake();
        }
    });
    result
}

fn received<T>(result: &Receiver<Result<T, String>>) -> Option<Result<T, String>> {
    match result.try_recv() {
        Ok(finished) => Some(finished),
        Err(TryRecvError::Empty) => None,
        Err(TryRecvError::Disconnected) => Some(Err("It stopped unexpectedly.".into())),
    }
}

/// Whether this copy was put in place by the install script, which can then replace it. A build
/// run from elsewhere, or on a system the script doesn't cover, gets the release page.
pub fn installs_itself() -> bool {
    if cfg!(not(target_os = "linux")) {
        return false;
    }
    let app = env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".local").join("pececito.app"))
        .and_then(|app| app.canonicalize().ok());
    let running = env::current_exe()
        .ok()
        .and_then(|running| running.canonicalize().ok());
    match (app, running) {
        (Some(app), Some(running)) => running.starts_with(app),
        _ => false,
    }
}

/// A command that doesn't flash a console window on Windows.
fn quiet(program: &str) -> Command {
    #[cfg_attr(not(windows), expect(unused_mut))]
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// Opens a file or web page in the desktop's default app for it.
pub fn open(target: &OsStr) -> Result<(), String> {
    // `start` takes its first quoted argument as a window title, so it gets an empty one.
    let (program, arguments): (&str, &[&str]) = if cfg!(target_os = "windows") {
        ("cmd", &["/C", "start", ""])
    } else {
        ("xdg-open", &[])
    };
    let status = quiet(program)
        .args(arguments)
        .arg(target)
        .status()
        .map_err(|error| format!("Can't run {program}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} couldn't open it ({status})."))
    }
}

/// The latest release's version. GitHub redirects `releases/latest` to the release's own page,
/// which is named after its tag, so this needs no API call.
fn latest_version() -> Result<String, String> {
    let nowhere = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let output = quiet("curl")
        .args(["-fsS", "--max-time", "20", "-o", nowhere])
        .args(["-w", "%{redirect_url}"])
        .arg(format!("{RELEASES}/latest"))
        .output()
        .map_err(|error| format!("Can't run curl to check for updates: {error}"))?;
    if !output.status.success() {
        let reason = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Couldn't reach GitHub. {}", reason.trim()));
    }
    version_in(&String::from_utf8_lossy(&output.stdout))
        .ok_or_else(|| "GitHub didn't say which release is the latest.".to_string())
}

/// The version in a release page's address, such as `.../releases/tag/v0.4.0`.
fn version_in(address: &str) -> Option<String> {
    let (_, version) = address.trim().rsplit_once("/tag/v")?;
    numbers(version).map(|_| version.to_string())
}

fn numbers(version: &str) -> Option<[u64; 3]> {
    let mut parts = version.split('.').map(|part| part.parse().ok());
    let numbers = [parts.next()??, parts.next()??, parts.next()??];
    parts.next().is_none().then_some(numbers)
}

fn is_newer(latest: &str, current: &str) -> bool {
    match (numbers(latest), numbers(current)) {
        (Some(latest), Some(current)) => latest > current,
        _ => false,
    }
}

/// Runs the install script for `version`, as installing by hand does.
fn install(version: &str) -> Result<(), String> {
    let failed = |error: std::io::Error| format!("Can't run the installer: {error}");
    let mut installer = Command::new("sh")
        .arg("-s")
        .env("PECECITO_VERSION", version)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(failed)?;
    if let Some(mut script) = installer.stdin.take() {
        script
            .write_all(INSTALL_SCRIPT.as_bytes())
            .map_err(failed)?;
    }
    let output = installer.wait_with_output().map_err(failed)?;
    if output.status.success() {
        return Ok(());
    }
    // The script says what went wrong on its last line; curl's progress bar comes before it.
    let said = [output.stderr, output.stdout].map(|stream| {
        String::from_utf8_lossy(&stream)
            .split(['\n', '\r'])
            .map(str::trim)
            .rfind(|line| !line.is_empty())
            .map(str::to_string)
    });
    let [errors, messages] = said;
    let reason = errors
        .or(messages)
        .unwrap_or_else(|| format!("The installer failed ({}).", output.status));
    // curl prints its error on the same line as the progress bar.
    let reason = reason
        .find("curl: ")
        .map_or(reason.as_str(), |start| &reason[start..]);
    Err(format!("Couldn't install version {version}. {reason}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_version_from_a_release_address() {
        let address = "https://github.com/acidtib/pececito/releases/tag/v0.4.0\n";
        assert_eq!(version_in(address).as_deref(), Some("0.4.0"));
        assert_eq!(
            version_in("https://github.com/acidtib/pececito/releases"),
            None
        );
        assert_eq!(version_in(".../releases/tag/vnext"), None);
        assert_eq!(version_in(""), None);
    }

    #[test]
    fn compares_versions_by_number() {
        assert!(is_newer("0.10.0", "0.9.3"));
        assert!(is_newer("1.0.0", "0.9.3"));
        assert!(!is_newer("0.3.0", "0.3.0"));
        assert!(!is_newer("0.2.9", "0.3.0"));
        assert!(!is_newer("soon", "0.3.0"));
    }
}
