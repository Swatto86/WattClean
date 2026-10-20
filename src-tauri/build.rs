use std::{env, fs, io, path::Path};

fn main() -> io::Result<()> {
    // tauri-build copies every bundle.externalBin beside the app and fails when
    // one is missing. scripts/bundle-cmd.mjs writes the real wattclean-cmd just
    // before bundling (beforeBundleCommand), so a check, test or dev build that
    // comes first gets an empty stand-in, which bundling always replaces. The
    // stand-in's copy can cover target/<profile>/wattclean-cmd; `cargo run -p
    // wattclean-cmd` puts the real one back.
    let target = env::var("TARGET").map_err(io::Error::other)?;
    let exe = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    let sidecar = Path::new("binaries").join(format!("wattclean-cmd-{target}{exe}"));
    if !sidecar.exists() {
        fs::create_dir_all("binaries")?;
        fs::write(&sidecar, b"")?;
    }
    tauri_build::build();
    Ok(())
}
