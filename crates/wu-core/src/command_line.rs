//! Turning an uninstall entry's command text into a program to start.

use std::path::{Path, PathBuf};

use crate::folders::Folders;
use crate::model::Program;
use crate::paths::expand;
use crate::system::{Launch, System};
use crate::{Error, Result};

/// How to start `program`'s own uninstaller. `quiet` asks for an unattended
/// run where the entry or its installer type says how; otherwise the normal
/// interactive uninstaller runs.
pub fn uninstall_launch(
    system: &dyn System,
    folders: &Folders,
    program: &Program,
    quiet: bool,
) -> Result<Launch> {
    let msiexec = folders.windows.join(r"System32\msiexec.exe");
    if let Some(code) = &program.msi_product_code {
        let args = if quiet {
            format!("/X{code} /qb- /norestart")
        } else {
            format!("/X{code}")
        };
        return Ok(Launch {
            program: msiexec,
            args,
        });
    }
    let quiet_command = program.quiet_uninstall_command.as_ref().filter(|_| quiet);
    let command = quiet_command
        .or(program.uninstall_command.as_ref())
        .ok_or_else(|| Error::Invalid(format!("{} has no uninstall command", program.name)))?;
    let command = expand(system, command);
    let (exe, mut args) = split_command(&command, |path| path.is_file()).ok_or_else(|| {
        Error::Invalid(format!(
            "the uninstall command of {} is empty",
            program.name
        ))
    })?;
    let program_path = resolve_program(folders, &exe);
    if is_named(&program_path, "msiexec.exe") {
        // `/I{code}` opens the change/repair screen; removal is `/X`.
        args = msi_install_to_remove(&args);
    } else if quiet && quiet_command.is_none() {
        args = add_quiet_switches(&program_path, args);
    }
    Ok(Launch {
        program: program_path,
        args,
    })
}

/// How to run an installer the person picked: an `.exe` as it is, an `.msi`
/// through Windows Installer. Anything else, or a path that is not an
/// existing file, is refused.
pub fn installer_launch(folders: &Folders, path: &Path) -> Result<Launch> {
    if !path.is_absolute() || !path.is_file() {
        return Err(Error::Invalid(format!(
            "{} is not an existing file",
            path.display()
        )));
    }
    let extension = path
        .extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "exe" => Ok(Launch {
            program: path.to_path_buf(),
            args: String::new(),
        }),
        "msi" => Ok(Launch {
            program: folders.windows.join(r"System32\msiexec.exe"),
            args: format!("/i \"{}\"", path.display()),
        }),
        _ => Err(Error::Invalid(
            "Choose a setup program (.exe) or a Windows Installer package (.msi).".into(),
        )),
    }
}

/// Split a command line into the executable and the rest, as `CreateProcess`
/// would find it: a quoted path is taken whole; an unquoted one grows word by
/// word until it names an existing file, which handles
/// `C:\Program Files\App\uninst.exe /x` written without quotes.
pub fn split_command(command: &str, exists: impl Fn(&Path) -> bool) -> Option<(String, String)> {
    let command = command.trim();
    if command.is_empty() {
        return None;
    }
    if let Some(rest) = command.strip_prefix('"') {
        let (exe, args) = rest.split_once('"').unwrap_or((rest, ""));
        return Some((exe.trim().to_owned(), args.trim().to_owned()));
    }
    let words: Vec<&str> = command.split(' ').collect();
    for end in 1..=words.len() {
        let candidate = words[..end].join(" ");
        let with_exe = format!("{candidate}.exe");
        if exists(Path::new(&candidate)) || exists(Path::new(&with_exe)) {
            let args = words[end..].join(" ");
            return Some((candidate, args.trim().to_owned()));
        }
    }
    let (exe, args) = command.split_once(' ').unwrap_or((command, ""));
    Some((exe.to_owned(), args.trim().to_owned()))
}

/// A bare system tool name (`MsiExec.exe`, `rundll32`) is taken from
/// System32, never from the current folder or the search path.
fn resolve_program(folders: &Folders, exe: &str) -> PathBuf {
    let path = PathBuf::from(exe);
    if path.components().count() > 1 {
        return path;
    }
    let mut name = exe.to_owned();
    if Path::new(&name).extension().is_none() {
        name.push_str(".exe");
    }
    folders.windows.join("System32").join(name)
}

fn is_named(path: &Path, name: &str) -> bool {
    path.file_name()
        .is_some_and(|file| file.to_string_lossy().eq_ignore_ascii_case(name))
}

fn msi_install_to_remove(args: &str) -> String {
    args.split(' ')
        .map(|word| {
            let lower = word.to_ascii_lowercase();
            if (lower.starts_with("/i") || lower.starts_with("-i")) && lower.contains('{') {
                format!("/X{}", &word[2..])
            } else {
                word.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Unattended switches for installer families recognisable by file name.
fn add_quiet_switches(program: &Path, args: String) -> String {
    let file = program
        .file_name()
        .map(|name| name.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    // Inno Setup names its uninstaller unins000.exe, unins001.exe, …
    let inno = file
        .strip_prefix("unins")
        .and_then(|rest| rest.strip_suffix(".exe"))
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()));
    let switches = if inno {
        "/VERYSILENT /SUPPRESSMSGBOXES /NORESTART"
    } else if file == "uninst.exe" || file == "uninstall.exe" {
        "/S"
    } else {
        return args;
    };
    if args.is_empty() {
        switches.to_owned()
    } else {
        format!("{args} {switches}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeSystem;
    use crate::programs::list_programs;

    #[test]
    fn splits_quoted_unquoted_and_spaced_paths() {
        let exists = |path: &Path| path == Path::new(r"C:\Program Files\Acme App\uninst.exe");
        let cases = [
            (r#""C:\A B\u.exe" /x /y"#, (r"C:\A B\u.exe", "/x /y")),
            (
                r"C:\Program Files\Acme App\uninst.exe /S",
                (r"C:\Program Files\Acme App\uninst.exe", "/S"),
            ),
            (
                r"C:\Program Files\Acme App\uninst /S",
                (r"C:\Program Files\Acme App\uninst", "/S"),
            ),
            (r"MsiExec.exe /I{X}", ("MsiExec.exe", "/I{X}")),
            (
                r"rundll32.exe a.dll,Remove",
                ("rundll32.exe", "a.dll,Remove"),
            ),
            (r#""C:\unterminated.exe"#, (r"C:\unterminated.exe", "")),
        ];
        for (command, (exe, args)) in cases {
            let got = split_command(command, exists).unwrap();
            assert_eq!(got, (exe.to_owned(), args.to_owned()), "{command}");
        }
        assert_eq!(split_command("   ", exists), None);
    }

    #[test]
    fn msi_entries_remove_by_product_code() {
        let dir = tempfile::tempdir().unwrap();
        let system = FakeSystem::seeded(dir.path()).unwrap();
        let folders = Folders::load(&system).unwrap();
        let viewer = list_programs(&system, false)
            .into_iter()
            .find(|p| p.name == "Fabrikam Viewer")
            .unwrap();
        let launch = uninstall_launch(&system, &folders, &viewer, false).unwrap();
        assert_eq!(
            launch.program,
            folders.windows.join(r"System32\msiexec.exe")
        );
        assert_eq!(launch.args, "/X{0A1B2C3D-1111-2222-3333-444455556666}");
        let quiet = uninstall_launch(&system, &folders, &viewer, true).unwrap();
        assert!(quiet.args.ends_with("/qb- /norestart"), "{}", quiet.args);
    }

    #[test]
    fn installers_run_directly_or_through_windows_installer() {
        let dir = tempfile::tempdir().unwrap();
        let system = FakeSystem::seeded(dir.path()).unwrap();
        let folders = Folders::load(&system).unwrap();
        let setup = dir.path().join(crate::fake::TAILSPIN_SETUP);
        let exe = installer_launch(&folders, &setup).unwrap();
        assert_eq!((exe.program, exe.args), (setup.clone(), String::new()));
        let package = dir.path().join("Viewer.MSI");
        std::fs::write(&package, b"msi").unwrap();
        let msi = installer_launch(&folders, &package).unwrap();
        assert_eq!(msi.program, folders.windows.join(r"System32\msiexec.exe"));
        assert_eq!(msi.args, format!("/i \"{}\"", package.display()));
        let notes = dir.path().join("readme.txt");
        std::fs::write(&notes, b"x").unwrap();
        for bad in [
            notes,
            dir.path().join("missing.exe"),
            PathBuf::from(r"relative\setup.exe"),
        ] {
            assert!(
                installer_launch(&folders, &bad).is_err(),
                "{}",
                bad.display()
            );
        }
    }

    #[test]
    fn msiexec_install_switch_becomes_remove() {
        assert_eq!(msi_install_to_remove("/I{ABC} /qn"), "/X{ABC} /qn");
        assert_eq!(msi_install_to_remove("/x{ABC}"), "/x{ABC}");
        assert_eq!(msi_install_to_remove("/i package.msi"), "/i package.msi");
    }

    #[test]
    fn quiet_switches_follow_the_installer_family() {
        let quiet = |file: &str, args: &str| add_quiet_switches(Path::new(file), args.to_owned());
        assert_eq!(
            quiet(r"C:\A\unins000.exe", ""),
            "/VERYSILENT /SUPPRESSMSGBOXES /NORESTART"
        );
        assert_eq!(quiet(r"C:\A\uninst.exe", "_?=C:\\A"), "_?=C:\\A /S");
        assert_eq!(quiet(r"C:\A\setup.exe", "/remove"), "/remove");
        assert_eq!(quiet(r"C:\A\uninsx.exe", ""), "");
    }

    #[test]
    fn expands_variables_and_finds_the_seeded_uninstaller() {
        let dir = tempfile::tempdir().unwrap();
        let system = FakeSystem::seeded(dir.path()).unwrap();
        let folders = Folders::load(&system).unwrap();
        let widget = list_programs(&system, false)
            .into_iter()
            .find(|p| p.name.starts_with("Acme Widget"))
            .unwrap();
        let launch = uninstall_launch(&system, &folders, &widget, false).unwrap();
        assert_eq!(
            launch.program,
            folders.program_files.join(r"Acme\Widget\uninstall.exe")
        );
        assert_eq!(launch.args, "/U");
        let bare = resolve_program(&folders, "rundll32");
        assert_eq!(bare, folders.windows.join(r"System32\rundll32.exe"));
    }
}
