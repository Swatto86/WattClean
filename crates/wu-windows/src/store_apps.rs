//! Windows' inbox Appx cmdlets own package deployment and its data cleanup.
//! Fixed script, no profiles, no shell interpolation of package identities.

use std::io;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};

use wu_core::store_apps::{StoreApp, valid_id};

fn run(id: Option<&str>) -> io::Result<Vec<u8>> {
    run_script(include_str!("store_apps.ps1"), id)
}

fn run_script(script: &str, id: Option<&str>) -> io::Result<Vec<u8>> {
    let root = std::env::var_os("SystemRoot")
        .ok_or_else(|| io::Error::other("Windows SystemRoot is unavailable."))?;
    let powershell =
        std::path::PathBuf::from(root).join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    let output = Command::new(powershell)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
        ])
        .env("WATTCLEAN_REMOVE_PACKAGE", id.unwrap_or_default())
        .env("WATTCLEAN_SELF_EXE", std::env::current_exe()?)
        .stdin(Stdio::null())
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "Windows Store operation failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(output.stdout)
}

pub fn list() -> io::Result<Vec<StoreApp>> {
    decode(&run(None)?)
}

fn decode(bytes: &[u8]) -> io::Result<Vec<StoreApp>> {
    let apps: Vec<StoreApp> = serde_json::from_slice(bytes).map_err(io::Error::other)?;
    if apps
        .iter()
        .any(|app| !valid_id(&app.id) || app.name.trim().is_empty())
    {
        return Err(io::Error::other(
            "Windows returned an invalid package identity.",
        ));
    }
    Ok(apps)
}

pub fn remove(id: &str) -> io::Result<()> {
    if !valid_id(id) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid package identity.",
        ));
    }
    run(Some(id)).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inbox_script_filters_rechecks_and_verifies_with_stubbed_cmdlets() {
        // Functions shadow the cmdlets: even a broken filter cannot remove
        // real software. Exercise the production process arguments and script.
        let script = format!(
            "{}\n& {{\n{}\n}}",
            include_str!("store_apps_test.ps1"),
            include_str!("store_apps.ps1")
        );
        let apps = decode(&run_script(&script, None).unwrap()).unwrap();
        assert_eq!(apps.len(), 3);
        assert!(apps.iter().any(|a| a.name == "Contoso café"));
        assert!(
            !apps
                .iter()
                .any(|a| a.id.contains("Protected") || a.id.contains("Framework"))
        );
        let id = |name| format!("Contoso.{name}_1.0.0.0_x64__abcdefghijklm");
        run_script(&script, Some(&id("Notes"))).unwrap();
        for name in ["Protected", "Framework", "Missing", "Blocked", "Sticky"] {
            assert!(run_script(&script, Some(&id(name))).is_err(), "{name}");
        }
    }

    #[test]
    fn inventory_requires_a_valid_array_and_identity() {
        let apps = decode(br#"[{"id":"Contoso.Notes_1.0.0.0_x64__abcdefghijklm","name":"Notes","publisher":"Contoso","version":"1.0.0.0"}]"#).unwrap();
        assert_eq!(apps[0].name, "Notes");
        assert!(decode(b"[]").unwrap().is_empty());
        for bad in [
            b"null".as_slice(),
            b"{}",
            b"",
            br#"[{"id":"*","name":"Notes","publisher":"Contoso","version":"1"}]"#,
        ] {
            assert!(decode(bad).is_err());
        }
        assert!(remove("*; exit").is_err());
    }
}
