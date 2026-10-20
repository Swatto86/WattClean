//! Whether the clipboard holds anything, decided from the kinds of data it
//! lists, never from the data itself: an image, a PDF or a file copied in
//! Finder has no text, and what was copied is never read.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::process;

/// How long a clipboard query may take.
const QUICK: Duration = Duration::from_secs(3);

/// `find` looks a helper program up by name.
pub fn has_data(find: &dyn Fn(&str) -> Option<PathBuf>) -> bool {
    let ask =
        |name: &str, args: &[&str]| find(name).and_then(|tool| process::quick(&tool, args, QUICK));
    if cfg!(target_os = "macos") {
        let osascript = Path::new("/usr/bin/osascript");
        return process::quick(osascript, &["-e", "clipboard info"], QUICK)
            .is_some_and(|info| mac_kinds_have_data(&info));
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_some() && find("wl-paste").is_some() {
        return ask("wl-paste", &["--list-types"]).is_some_and(|kinds| !kinds.trim().is_empty());
    }
    if find("xclip").is_some() {
        return ask("xclip", &["-selection", "clipboard", "-o", "-t", "TARGETS"])
            .is_some_and(|targets| x11_targets_have_data(&targets));
    }
    // xsel cannot list the kinds without reading the data: the clear is
    // offered whenever xsel is there to do it.
    find("xsel").is_some()
}

/// AppleScript's `clipboard info` as osascript prints it: each kind of data
/// and its size in bytes (`«class PNGf», 81234, TIFF picture, 302144`). A
/// clipboard emptied by pbcopy may still list a kind with no bytes.
fn mac_kinds_have_data(info: &str) -> bool {
    info.split(',').any(|part| {
        part.trim()
            .trim_matches(['{', '}'])
            .trim()
            .parse::<u64>()
            .is_ok_and(|bytes| bytes > 0)
    })
}

/// The X11 clipboard's targets, one per line; those the selection protocol
/// itself adds are not data.
fn x11_targets_have_data(targets: &str) -> bool {
    const PROTOCOL: [&str; 4] = ["TARGETS", "TIMESTAMP", "MULTIPLE", "SAVE_TARGETS"];
    targets
        .lines()
        .map(str::trim)
        .any(|target| !target.is_empty() && !PROTOCOL.contains(&target))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_counts_any_kind_of_data_with_bytes() {
        for (info, has) in [
            (
                "«class PNGf», 81234, «class 8BPS», 0, TIFF picture, 302144",
                true,
            ),
            ("«class furl», 52, «class utf8», 18", true),
            ("{{«class PNGf», 81234}}", true),
            ("«class utf8», 0, string, 0, Unicode text, 0", false),
            ("", false),
        ] {
            assert_eq!(mac_kinds_have_data(info), has, "{info:?}");
        }
    }

    #[test]
    fn x11_counts_any_target_besides_the_protocols_own() {
        for (targets, has) in [
            ("TARGETS\nTIMESTAMP\nMULTIPLE\nimage/png\n", true),
            ("TARGETS\nUTF8_STRING\nSTRING\n", true),
            ("TARGETS\nTIMESTAMP\nMULTIPLE\nSAVE_TARGETS\n", false),
            ("", false),
        ] {
            assert_eq!(x11_targets_have_data(targets), has, "{targets:?}");
        }
    }
}
