//! Finding a program (`lookup`) and running it, with administrator rights
//! when asked: through polkit's `pkexec` on Linux and
//! the system's own password prompt (`osascript`) on macOS. Arguments are
//! always passed one by one, never through a shell, except to macOS's
//! prompt, which takes one command line: there every argument is quoted.

use std::io::{self, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use wcc_core::system::ProgramOutput;

mod lookup;

pub use lookup::{find, find_trusted};

/// Runs `program` with `args` and waits for it; with `admin`, asks for
/// administrator rights first unless this program already has them. `env`
/// sets variables for this one run.
pub fn run(
    program: &Path,
    args: &[String],
    admin: bool,
    env: &[(&str, &str)],
) -> io::Result<ProgramOutput> {
    let (mut command, prompted) = prepare(program, args, admin, env, crate::sys::is_elevated())?;
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

/// The command [`run`] starts, and whether it goes through the password
/// prompt. A program run as administrator was chosen with
/// [`find_trusted`] when the run was planned; it is checked again here,
/// and its checked real path is what starts. Variables cannot be passed
/// through the prompt (pkexec and macOS's prompt start the program with an
/// environment of their own), so asking for both is refused rather than
/// the variables silently dropped.
fn prepare(
    program: &Path,
    args: &[String],
    admin: bool,
    env: &[(&str, &str)],
    elevated: bool,
) -> io::Result<(Command, bool)> {
    let program = if admin {
        lookup::trusted(program)?
    } else {
        program.to_path_buf()
    };
    let prompted = admin && !elevated;
    if prompted && !env.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "{} cannot be given settings through the administrator prompt",
                program.display()
            ),
        ));
    }
    let mut command = if prompted {
        as_admin(&program, args)?
    } else {
        let mut command = Command::new(&program);
        command.args(args);
        command
    };
    // Output in one language, so listings parse the same everywhere.
    command
        .envs(env.iter().copied())
        .env("LC_ALL", "C")
        .stdin(Stdio::null());
    Ok((command, prompted))
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
    let pkexec = find_trusted("pkexec").ok_or_else(|| {
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
/// account may not; osascript reports error -128 (worded "User canceled" or
/// "User cancelled", and translated in other languages).
fn refused(status: &std::process::ExitStatus, stderr: &str) -> bool {
    if cfg!(target_os = "macos") {
        !status.success() && osascript_dismissed(stderr)
    } else {
        match status.code() {
            Some(126) => true,
            Some(127) => stderr.contains("Not authorized"),
            _ => false,
        }
    }
}

/// osascript's message ends with the error number, so a command that merely
/// prints "(-128)" is not taken for a dismissed prompt.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn osascript_dismissed(stderr: &str) -> bool {
    stderr.trim_end().ends_with("(-128)")
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
    fn a_program_runs_with_its_arguments_unparsed() {
        let sh = find("sh", None).unwrap();
        let args = vec![
            "-c".to_string(),
            "printf %s \"$1\"".to_string(),
            "x".to_string(),
            "a b;c".to_string(),
        ];
        let output = run(&sh, &args, false, &[]).unwrap();
        assert!(output.success);
        assert_eq!(output.stdout, "a b;c");
        let failed = run(&sh, &["-c".to_string(), "exit 3".to_string()], false, &[]).unwrap();
        assert!(!failed.success);
    }

    /// Variables reach the program run as the account; through the
    /// password prompt they could not, so that is refused. Nothing here
    /// is started with administrator rights: `prepare` only builds the
    /// command.
    #[test]
    fn settings_reach_the_program_but_never_through_the_prompt() {
        let sh = find("sh", None).unwrap();
        let env = [("HOMEBREW_NO_AUTOREMOVE", "1")];
        let args = vec![
            "-c".to_string(),
            "printf %s \"$HOMEBREW_NO_AUTOREMOVE\"".to_string(),
        ];
        assert_eq!(run(&sh, &args, false, &env).unwrap().stdout, "1");
        assert_eq!(run(&sh, &args, false, &[]).unwrap().stdout, "");
        let refused = prepare(Path::new("/bin/sh"), &args, true, &env, false).unwrap_err();
        assert_eq!(refused.kind(), io::ErrorKind::InvalidInput);
        let (command, prompted) = prepare(Path::new("/bin/sh"), &args, true, &env, true).unwrap();
        assert!(!prompted);
        assert!(
            command
                .get_envs()
                .any(|(name, value)| name == "HOMEBREW_NO_AUTOREMOVE"
                    && value == Some(std::ffi::OsStr::new("1")))
        );
    }

    /// A program the account could have changed is refused when it would
    /// run as administrator (checked again, though planning chose it with
    /// `find_trusted`), whether or not WattClean already has the rights.
    #[test]
    fn an_administrator_command_starts_only_a_checked_system_program() {
        use std::os::unix::fs::PermissionsExt;
        let planted = tempfile::tempdir().unwrap();
        let port = planted.path().join("port");
        std::fs::write(&port, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&port, std::fs::Permissions::from_mode(0o755)).unwrap();
        for elevated in [false, true] {
            let refused = prepare(&port, &[], true, &[], elevated).unwrap_err();
            assert_eq!(refused.kind(), io::ErrorKind::PermissionDenied);
        }
        assert!(prepare(&port, &[], false, &[], false).is_ok());

        let real = std::fs::canonicalize("/bin/sh").unwrap();
        let (command, prompted) = prepare(Path::new("/bin/sh"), &[], true, &[], true).unwrap();
        assert!(!prompted);
        assert_eq!(command.get_program(), real.as_os_str());
        match prepare(Path::new("/bin/sh"), &["-x".to_string()], true, &[], false) {
            Ok((command, prompted)) => {
                assert!(prompted);
                let args: Vec<String> = command
                    .get_args()
                    .map(|arg| arg.to_string_lossy().into_owned())
                    .collect();
                if cfg!(target_os = "macos") {
                    assert_eq!(command.get_program(), "/usr/bin/osascript");
                    let script = format!("'{}' '-x'", real.display());
                    assert!(args[1].contains(&script), "{args:?}");
                } else {
                    assert_eq!(Some(command.get_program().into()), find_trusted("pkexec"));
                    assert_eq!(args, [real.to_string_lossy().into_owned(), "-x".into()]);
                }
            }
            // A Linux machine without polkit says how to get the rights.
            Err(error) => assert!(
                !cfg!(target_os = "macos") && find_trusted("pkexec").is_none(),
                "{error}"
            ),
        }
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
    fn a_dismissed_password_prompt_is_recognised_in_either_spelling() {
        assert!(osascript_dismissed(
            "0:17: execution error: User cancelled. (-128)\n"
        ));
        assert!(osascript_dismissed(
            "0:17: execution error: User canceled. (-128)"
        ));
        assert!(!osascript_dismissed(
            "0:51: execution error: oops (-128) (3)"
        ));
        assert!(!osascript_dismissed(
            "0:26: execution error: The command exited with a non-zero status. (128)"
        ));
    }

    #[test]
    fn quoting_keeps_every_argument_one_word() {
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
        assert_eq!(applescript_escape(r#"a "b" \c"#), r#"a \"b\" \\c"#);
    }
}
