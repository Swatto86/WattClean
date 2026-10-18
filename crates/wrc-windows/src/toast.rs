//! A Windows notification under the app's own name and icon. Windows takes
//! both from the Start menu shortcut the installer made, which carries the
//! app's AppUserModelID; a copy not installed that way (a development
//! build) has no shortcut, and Windows quietly shows nothing.

use std::io;

use windows::Data::Xml::Dom::XmlDocument;
use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
use windows::core::HSTRING;

/// The installer's `identifier` in `tauri.conf.json`.
const APP_ID: &str = "co.swatto.wattclean";

pub fn show(title: &str, body: &str) -> io::Result<()> {
    let xml = format!(
        "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual></toast>",
        escape(title),
        escape(body)
    );
    let document = XmlDocument::new().map_err(io::Error::other)?;
    document
        .LoadXml(&HSTRING::from(xml))
        .map_err(io::Error::other)?;
    let toast = ToastNotification::CreateToastNotification(&document).map_err(io::Error::other)?;
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_ID))
        .and_then(|notifier| notifier.Show(&toast))
        .map_err(io::Error::other)
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
