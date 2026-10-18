//! Task Scheduler tasks, through Windows' own `schtasks.exe`.

use std::io;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

/// Keeps console tools from flashing a window.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn schtasks() -> PathBuf {
    std::env::var_os("SystemRoot")
        .map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from)
        .join(r"System32\schtasks.exe")
}

fn run(args: &[&str]) -> io::Result<std::process::Output> {
    Command::new(schtasks())
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
}

/// Create or replace the task. The XML goes through a file, in the UTF-16
/// its declaration names; the file is removed again.
pub fn schedule(name: &str, xml: &str) -> io::Result<()> {
    let file = std::env::temp_dir().join(format!("wattclean-task-{}.xml", std::process::id()));
    let mut bytes = vec![0xff, 0xfe];
    bytes.extend(xml.encode_utf16().flat_map(u16::to_le_bytes));
    std::fs::write(&file, bytes)?;
    let path = file.to_string_lossy().into_owned();
    let output = run(&["/Create", "/TN", name, "/XML", &path, "/F"]);
    let _ = std::fs::remove_file(&file);
    let output = output?;
    if !output.status.success() {
        return Err(io::Error::other(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    Ok(())
}

pub fn exists(name: &str) -> io::Result<bool> {
    Ok(run(&["/Query", "/TN", name])?.status.success())
}

/// Delete the task; a task that is not there is already deleted.
pub fn delete(name: &str) -> io::Result<()> {
    let query = run(&["/Query", "/TN", name])?;
    if !query.status.success() {
        return Ok(());
    }
    let output = run(&["/Delete", "/TN", name, "/F"])?;
    if !output.status.success() {
        return Err(io::Error::other(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    Ok(())
}
