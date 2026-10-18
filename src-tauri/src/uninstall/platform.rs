//! Which machine the app acts on: Windows itself, or (in the WebDriver
//! suite's `fake-system` build) a seeded fake under the data folder.

use std::path::Path;
use std::sync::Arc;

use wu_core::System;

#[cfg(feature = "fake-system")]
pub fn machine(data_dir: &Path) -> std::io::Result<(Arc<dyn System>, bool)> {
    let fake = wu_core::fake::FakeSystem::persistent(&data_dir.join("machine"))?;
    if std::env::var_os("WATTCLEAN_TEST_NOT_ELEVATED").is_some() {
        fake.set_elevated(false);
    }
    Ok((Arc::new(fake), true))
}

#[cfg(all(windows, not(feature = "fake-system")))]
pub fn machine(_data_dir: &Path) -> std::io::Result<(Arc<dyn System>, bool)> {
    Ok((Arc::new(wu_windows::WindowsSystem::new()), false))
}

/// The test machine's own setup, so the WebDriver suite needs no dialog.
#[cfg(feature = "fake-system")]
pub fn pick_installer(_app: &tauri::AppHandle, data_dir: &Path) -> Option<std::path::PathBuf> {
    Some(data_dir.join("machine").join(wu_core::fake::TAILSPIN_SETUP))
}

/// The native Open dialog, filtered to setup programs and MSI packages.
#[cfg(not(feature = "fake-system"))]
pub fn pick_installer(app: &tauri::AppHandle, _data_dir: &Path) -> Option<std::path::PathBuf> {
    use tauri_plugin_dialog::DialogExt;
    app.dialog()
        .file()
        .set_title("Choose an installer to trace")
        .add_filter("Installers", &["exe", "msi"])
        .blocking_pick_file()
        .and_then(|file| file.into_path().ok())
}
