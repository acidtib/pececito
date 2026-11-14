use std::fs;
use std::path::Path;

use crate::GameProcess;

/// Wine names the process after the exe; `/proc/<pid>/comm` keeps 15 bytes, which fits.
const PROCESS_NAME: &str = "Wow.exe";
/// The WoW Forever client is installed under `World of Warcraft\_forever_\`, which separates it
/// from retail or classic clients launched from the same Battle.net prefix.
const INSTALL_MARKER: &str = "_forever_";

pub(crate) fn find_process(proc_root: &Path) -> Result<Option<GameProcess>, String> {
    let entries = fs::read_dir(proc_root)
        .map_err(|error| format!("reading {}: {error}", proc_root.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("listing {}: {error}", proc_root.display()))?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        // Processes exit between listing and reading, and others' may be unreadable; neither is the game.
        let Ok(name) = fs::read_to_string(entry.path().join("comm")) else {
            continue;
        };
        if name.trim_end() != PROCESS_NAME {
            continue;
        }
        let Ok(command_line) = fs::read(entry.path().join("cmdline")) else {
            continue;
        };
        if let Some(process) = match_command_line(pid, &command_line) {
            return Ok(Some(process));
        }
    }
    Ok(None)
}

fn match_command_line(pid: u32, command_line: &[u8]) -> Option<GameProcess> {
    let mut arguments = command_line
        .split(|byte| *byte == 0)
        .map(String::from_utf8_lossy);
    let executable = arguments.next()?.into_owned();
    let is_forever = executable.to_ascii_lowercase().contains(INSTALL_MARKER);
    is_forever.then_some(GameProcess { pid, executable })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command_line(arguments: &[&str]) -> Vec<u8> {
        arguments
            .iter()
            .flat_map(|argument| argument.bytes().chain([0]))
            .collect()
    }

    #[test]
    fn matches_the_forever_client() {
        let line = command_line(&[
            r"C:\Program Files (x86)\World of Warcraft\_forever_\Wow.exe",
            "-launcherlogin",
            "-uid",
            "wowf",
        ]);
        let process = match_command_line(42, &line);
        assert_eq!(
            process,
            Some(GameProcess {
                pid: 42,
                executable: r"C:\Program Files (x86)\World of Warcraft\_forever_\Wow.exe".into()
            })
        );
    }

    #[test]
    fn ignores_other_clients() {
        let line = command_line(&[
            r"C:\Program Files (x86)\World of Warcraft\_retail_\Wow.exe",
            "-uid",
            "wow",
        ]);
        assert_eq!(match_command_line(42, &line), None);
    }

    #[test]
    fn finds_the_process_in_a_proc_tree() -> Result<(), String> {
        let root = std::env::temp_dir().join(format!("pececito-proc-{}", std::process::id()));
        let write = |pid: &str, name: &str, line: &[&str]| -> std::io::Result<()> {
            let directory = root.join(pid);
            fs::create_dir_all(&directory)?;
            fs::write(directory.join("comm"), format!("{name}\n"))?;
            fs::write(directory.join("cmdline"), command_line(line))
        };
        let setup = (|| -> std::io::Result<()> {
            write("100", "bash", &["/bin/bash"])?;
            write(
                "200",
                "Wow.exe",
                &[r"C:\World of Warcraft\_retail_\Wow.exe"],
            )?;
            write(
                "300",
                "Wow.exe",
                &[r"C:\World of Warcraft\_forever_\Wow.exe", "-uid", "wowf"],
            )?;
            fs::create_dir_all(root.join("self"))
        })();
        let found = setup
            .map_err(|error| error.to_string())
            .and_then(|()| find_process(&root));
        fs::remove_dir_all(&root).map_err(|error| error.to_string())?;
        assert_eq!(found?.map(|process| process.pid), Some(300));
        Ok(())
    }
}
