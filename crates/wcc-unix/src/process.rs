//! Finding a program in the system's own folders and running it, with
//! administrator rights when asked: through polkit's `pkexec` on Linux and
//! the system's own password prompt (`osascript`) on macOS. Arguments are
//! always passed one by one, never through a shell, except to macOS's
//! prompt, which takes one command line: there every argument is quoted.

use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use wcc_core::system::ProgramOutput;

/// Where package managers and developer tools install themselves, searched
/// before `PATH` (a desktop app's `PATH` is often short, and on macOS lacks
/// Homebrew and MacPorts). `~` is the home folder.
const FOLDERS: &[&str] = &[
    "/usr/local/sbin",
    "/usr/local/bin",
    "/usr/sbin",
    "/usr/bin",
    "/sbin",
    "/bin",
    "/opt/homebrew/bin",
    "/opt/local/bin",
    "/home/linuxbrew/.linuxbrew/bin",
    "~/.linuxbrew/bin",
    "/snap/bin",
    "/nix/var/nix/profiles/default/bin",
    "~/.nix-profile/bin",
    "/usr/local/go/bin",
    "/usr/lib/go/bin",
    "~/.local/bin",
    "~/.local/share/pnpm",
    "~/.bun/bin",
    "~/miniconda3/bin",
    "~/miniforge3/bin",
    "~/anaconda3/bin",
    "/opt/conda/bin",
    "/Applications/Docker.app/Contents/Resources/bin",
];

/// The full path of an executable program called `name`; `None` when none
/// is installed. `name` is a bare name: anything with a separator is
/// refused.
pub fn find(name: &str, home: Option<&Path>) -> Option<PathBuf> {
    if name.is_empty() || name.contains('/') {
        return None;
    }
    let from_path = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .unwrap_or_default();
    FOLDERS
        .iter()
        .filter_map(|folder| match folder.strip_prefix("~/") {
            Some(rest) => home.map(|home| home.join(rest)),
            None => Some(PathBuf::from(folder)),
        })
        .chain(from_path.into_iter().filter(|dir| dir.is_absolute()))
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

/// Runs `program` with `args` and waits for it; with `admin`, asks for
/// administrator rights first unless this program already has them.
pub fn run(program: &Path, args: &[String], admin: bool) -> io::Result<ProgramOutput> {
    let prompted = admin && !crate::sys::is_elevated();
    let mut command = if prompted {
        as_admin(program, args)?
    } else {
        let mut command = Command::new(program);
        command.args(args);
        command
    };
    // Output in one language, so listings parse the same everywhere.
    command.env("LC_ALL", "C").stdin(Stdio::null());
    let output = command.output().map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("{} could not start: {error}", program.display()),
        )
    })?;
    let result = ProgramOutput {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    };
    if prompted && refused(&output.status, &result.stderr) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "administrator rights were not granted",
        ));
    }
    Ok(result)
}

/// Runs a quick read-only command (a clipboard or process query), ending it
/// if it has not finished within `limit`. `None` when it could not run,
/// failed or took too long.
pub fn quick(program: &Path, args: &[&str], limit: Duration) -> Option<String> {
    let mut child = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    // Read on another thread: a helper the program started may hold the
    // output open after it ends, and must not hold this caller up.
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut text = Vec::new();
        if stdout.read_to_end(&mut text).is_ok() {
            let _ = send.send(text);
        }
    });
    let deadline = Instant::now() + limit;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let text = receive
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .ok()?;
    status
        .success()
        .then(|| String::from_utf8_lossy(&text).into_owned())
}

#[cfg(not(target_os = "macos"))]
fn as_admin(program: &Path, args: &[String]) -> io::Result<Command> {
    let pkexec = find("pkexec", None).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "this needs administrator rights: install polkit (pkexec) or run WattClean as root",
        )
    })?;
    let mut command = Command::new(pkexec);
    command.arg(program).args(args);
    Ok(command)
}

/// pkexec ends with 126 when the prompt was dismissed and 127 when the
/// account may not; osascript says "User canceled".
fn refused(status: &std::process::ExitStatus, stderr: &str) -> bool {
    if cfg!(target_os = "macos") {
        !status.success() && stderr.contains("User canceled")
    } else {
        match status.code() {
            Some(126) => true,
            Some(127) => stderr.contains("Not authorized"),
            _ => false,
        }
    }
}

#[cfg(target_os = "macos")]
fn as_admin(program: &Path, args: &[String]) -> io::Result<Command> {
    let mut line = shell_quote(&program.to_string_lossy());
    for arg in args {
        line.push(' ');
        line.push_str(&shell_quote(arg));
    }
    let script = format!(
        "do shell script \"{}\" with administrator privileges",
        applescript_escape(&line)
    );
    let mut command = Command::new("/usr/bin/osascript");
    command.arg("-e").arg(script);
    Ok(command)
}

/// One shell word: single quotes around it, any single quote closed,
/// escaped and opened again.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// Text inside an AppleScript string literal.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn applescript_escape(text: &str) -> String {
    text.replace('\\', r"\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn programs_are_found_by_bare_name_only() {
        assert!(find("sh", None).is_some());
        assert!(find("/bin/sh", None).is_none());
        assert!(find("../sh", None).is_none());
        assert!(find("no-such-program-wattclean", None).is_none());
    }

    #[test]
    fn a_program_runs_with_its_arguments_unparsed() {
        let sh = find("sh", None).unwrap();
        let args = vec![
            "-c".to_string(),
            "printf %s \"$1\"".to_string(),
            "x".to_string(),
            "a b;c".to_string(),
        ];
        let output = run(&sh, &args, false).unwrap();
        assert!(output.success);
        assert_eq!(output.stdout, "a b;c");
        let failed = run(&sh, &["-c".to_string(), "exit 3".to_string()], false).unwrap();
        assert!(!failed.success);
    }

    #[test]
    fn a_quick_query_that_hangs_is_ended() {
        let sh = find("sh", None).unwrap();
        let started = Instant::now();
        assert_eq!(
            quick(&sh, &["-c", "sleep 5"], Duration::from_millis(200)),
            None
        );
        assert!(started.elapsed() < Duration::from_secs(3));
        assert_eq!(
            quick(&sh, &["-c", "echo hi"], Duration::from_secs(5)).as_deref(),
            Some("hi\n")
        );
    }

    #[test]
    fn quoting_keeps_every_argument_one_word() {
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
        assert_eq!(applescript_escape(r#"a "b" \c"#), r#"a \"b\" \\c"#);
    }
}
