//! A second Ctrl+C ends the console program at once with code 130, even
//! when it arrives in the middle of a write to the screen. (The standard
//! library's exit, run from the signal handler there, aborted instead.)
//!
//! It runs itself again as the program: a plain `main` without the test
//! harness, so its one thread is the one the signal lands on, blocked
//! writing to a pipe nobody reads.

#[cfg(unix)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::io::{Read, Write};
    use std::os::unix::process::ExitStatusExt;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    if std::env::var_os("WCC_CTRL_C_CHILD").is_some() {
        wcc_unix::stop_on_ctrl_c()?;
        let mut out = std::io::stdout();
        loop {
            out.write_all(&[b'x'; 4096])?;
        }
    }
    let mut child = Command::new(std::env::current_exe()?)
        .env("WCC_CTRL_C_CHILD", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    // Its first byte shows the handler is in place (however slowly it
    // started); then time to fill the pipe and block in the write.
    let mut first = [0u8; 1];
    child
        .stdout
        .as_mut()
        .ok_or("no output pipe")?
        .read_exact(&mut first)?;
    std::thread::sleep(Duration::from_millis(500));
    for _ in 0..2 {
        let sent = Command::new("kill")
            .args(["-INT", &child.id().to_string()])
            .status()?;
        assert!(sent.success(), "kill -INT failed");
        std::thread::sleep(Duration::from_millis(200));
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() > deadline {
            child.kill()?;
            panic!("a second Ctrl+C did not end the program");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(
        status.code(),
        Some(130),
        "{status} (signal {:?})",
        status.signal()
    );
    println!("a second Ctrl+C ended the program with 130");
    Ok(())
}

#[cfg(not(unix))]
fn main() {}
