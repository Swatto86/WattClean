//! The password box sudo shows when Homebrew needs an administrator's
//! password on macOS. Homebrew runs some of a cask's steps (deleting what
//! its uninstall step names, forgetting its installer packages) through
//! sudo, and an app has no terminal for sudo to ask in, so Homebrew's
//! removals run with `SUDO_ASKPASS` naming this script: sudo runs it and
//! reads the password it prints, typed in macOS's own dialog. WattClean
//! never sees the password. The script's text is fixed (no name from
//! outside goes into it, so it says "the program you chose"), and it is
//! written afresh, for the account alone, before each use.

use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// The dialog, in AppleScript, a line each. Cancelling it, or leaving it
/// for two minutes, ends the script with an error, so sudo gives up
/// rather than waiting.
const LINES: [&str; 4] = [
    "activate",
    "set reply to display dialog \"Homebrew needs an administrator password to remove the program you chose in WattClean.\" with title \"WattClean\" default answer \"\" with hidden answer with icon caution giving up after 120",
    "if gave up of reply then error number -128",
    "return text returned of reply",
];

/// The script: macOS's `osascript` running [`LINES`].
fn script() -> String {
    let lines: Vec<String> = LINES.iter().map(|line| format!("-e '{line}'")).collect();
    format!(
        "#!/bin/sh\n# WattClean: the administrator password Homebrew's sudo asks for.\nexec /usr/bin/osascript {}\n",
        lines.join(" \\\n  ")
    )
}

/// Writes the script into `folder` (a new file, then renamed over the old
/// one) and returns its path.
pub fn write(folder: &Path) -> io::Result<PathBuf> {
    fs::create_dir_all(folder)?;
    let path = folder.join("askpass.sh");
    let new = folder.join("askpass.sh.new");
    match fs::remove_file(&new) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o700)
        .open(&new)?;
    file.write_all(script().as_bytes())?;
    file.sync_all()?;
    drop(file);
    fs::set_permissions(&new, fs::Permissions::from_mode(0o700))?;
    fs::rename(&new, &path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_script_is_written_afresh_for_the_account_alone() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("uninstall");
        let path = write(&folder).unwrap();
        assert_eq!(path, folder.join("askpass.sh"));
        assert_eq!(fs::read_to_string(&path).unwrap(), script());
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);

        fs::write(&path, "#!/bin/sh\necho stolen\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o777)).unwrap();
        fs::write(folder.join("askpass.sh.new"), "left over").unwrap();
        write(&folder).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), script());
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
        assert!(!folder.join("askpass.sh.new").exists());
    }

    /// Each line goes to the shell inside single quotes.
    #[test]
    fn the_script_is_valid_shell() {
        assert!(LINES.iter().all(|line| !line.contains('\'')));
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path()).unwrap();
        let checked = std::process::Command::new("/bin/sh")
            .arg("-n")
            .arg(&path)
            .status()
            .unwrap();
        assert!(checked.success());
    }

    /// Compiled only, never run: running it would put the dialog on screen.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_dialog_is_valid_applescript() {
        let dir = tempfile::tempdir().unwrap();
        let mut compile = std::process::Command::new("/usr/bin/osacompile");
        compile.arg("-o").arg(dir.path().join("check.scpt"));
        for line in LINES {
            compile.arg("-e").arg(line);
        }
        let output = compile.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
