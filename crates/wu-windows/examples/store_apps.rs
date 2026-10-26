//! Read-only check of the same inventory used by the desktop application.
#[cfg(windows)]
fn main() -> std::io::Result<()> {
    let apps = wu_windows::store_apps::list()?;
    for app in &apps {
        println!("{}\t{}\t{}", app.name, app.version, app.id);
    }
    println!("{} removable Store apps", apps.len());
    Ok(())
}

#[cfg(not(windows))]
fn main() {}
