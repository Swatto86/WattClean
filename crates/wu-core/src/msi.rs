//! Windows Installer's own records of a product: the packed form of its
//! product code, and the files its components registered.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::model::{Hive, RawValue, RegPath};
use crate::paths::{embedded_path, is_within};
use crate::system::{System, subkeys_or_empty, text_value, values_or_empty};

pub const COMPONENTS: &str =
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\Installer\UserData\S-1-5-18\Components";
pub const PRODUCTS: &str = r"SOFTWARE\Classes\Installer\Products";
pub const UPGRADE_CODES: &str = r"SOFTWARE\Classes\Installer\UpgradeCodes";
pub const USER_UPGRADE_CODES: &str = r"Software\Microsoft\Installer\UpgradeCodes";

/// `{CODE}` written the way Windows Installer names its keys: each group
/// reversed, and the last two groups byte-wise with nibbles swapped.
pub fn packed_guid(code: &str) -> Option<String> {
    let hex: String = code.chars().filter(char::is_ascii_hexdigit).collect();
    if hex.len() != 32 {
        return None;
    }
    let reversed = |range: std::ops::Range<usize>| hex[range].chars().rev().collect::<String>();
    let mut packed = reversed(0..8) + &reversed(8..12) + &reversed(12..16);
    for at in (16..32).step_by(2) {
        packed.push_str(&reversed(at..at + 2));
    }
    Some(packed.to_uppercase())
}

/// The component keys whose values name this product, with the file each
/// registered. Windows Installer leaves these behind when a product's
/// uninstall is interrupted or its files were deleted by hand.
pub fn component_files(system: &dyn System, packed: &str) -> Vec<(RegPath, PathBuf)> {
    let components = RegPath::new(Hive::LocalMachine, COMPONENTS);
    let mut found = Vec::new();
    for name in subkeys_or_empty(system, &components) {
        let key = components.child(&name);
        for value in values_or_empty(system, &key) {
            if value.name.eq_ignore_ascii_case(packed)
                && let Some(path) = value.as_text().as_deref().and_then(component_path)
            {
                found.push((key.clone(), path));
            }
        }
    }
    found
}

/// A component's path as Windows Installer writes it: a plain file path, or
/// `02:\SOFTWARE\…` for a registry component (no file).
fn component_path(data: &str) -> Option<PathBuf> {
    let text = data.trim();
    if text.get(1..3) == Some("?\\") || text.starts_with("02:") || text.starts_with("22:") {
        return None;
    }
    embedded_path(text)
}

/// The folders of the files each product's own components registered, read
/// in one pass for every product (packed codes in upper case). A component
/// several products share, or one Windows keeps for good, names no
/// product's folder: it is not any one product's.
pub struct ComponentIndex(HashMap<String, Vec<PathBuf>>);

impl ComponentIndex {
    pub fn load(system: &dyn System) -> Self {
        let components = RegPath::new(Hive::LocalMachine, COMPONENTS);
        let mut dirs: HashMap<String, Vec<PathBuf>> = HashMap::new();
        for name in subkeys_or_empty(system, &components) {
            let values = values_or_empty(system, &components.child(&name));
            let [value] = values.as_slice() else {
                continue;
            };
            if let Some(dir) = value
                .as_text()
                .as_deref()
                .and_then(component_path)
                .and_then(|file| file.parent().map(Path::to_path_buf))
            {
                dirs.entry(value.name.to_uppercase()).or_default().push(dir);
            }
        }
        Self(dirs)
    }

    /// The folders the product's files live in, shallowest first, with a
    /// folder inside another of them dropped: a product registering every
    /// file has hundreds of folders, all inside its own.
    pub fn product_dirs(&self, system: &dyn System, packed: &str) -> Vec<PathBuf> {
        let mut dirs = self
            .0
            .get(&packed.to_uppercase())
            .cloned()
            .unwrap_or_default();
        let products = RegPath::new(Hive::LocalMachine, PRODUCTS).child(packed);
        let icon = text_value(&values_or_empty(system, &products), "ProductIcon");
        if let Some(dir) = icon
            .as_deref()
            .and_then(embedded_path)
            .and_then(|icon| icon.parent().map(Path::to_path_buf))
        {
            dirs.push(dir);
        }
        dirs.sort_by_key(|dir| dir.components().count());
        let mut top: Vec<PathBuf> = Vec::new();
        for dir in dirs {
            if !top.iter().any(|known| is_within(&dir, known)) {
                top.push(dir);
            }
        }
        top
    }
}

/// [`ComponentIndex::product_dirs`] for one product.
pub fn product_dirs(system: &dyn System, packed: &str) -> Vec<PathBuf> {
    ComponentIndex::load(system).product_dirs(system, packed)
}

/// The product's name as Windows Installer recorded it.
pub fn product_name(system: &dyn System, packed: &str) -> Option<String> {
    let products = RegPath::new(Hive::LocalMachine, PRODUCTS).child(packed);
    text_value(&values_or_empty(system, &products), "ProductName")
}

/// Upgrade-code keys whose values name this product.
pub fn upgrade_entries(system: &dyn System, packed: &str) -> Vec<(RegPath, RawValue)> {
    let mut found = Vec::new();
    for root in [
        RegPath::new(Hive::LocalMachine, UPGRADE_CODES),
        RegPath::new(Hive::CurrentUser, USER_UPGRADE_CODES),
    ] {
        for name in subkeys_or_empty(system, &root) {
            let key = root.child(&name);
            for value in values_or_empty(system, &key) {
                if value.name.eq_ignore_ascii_case(packed) {
                    found.push((key.clone(), value));
                }
            }
        }
    }
    found
}

#[cfg(test)]
#[cfg(windows)]
mod tests {
    use super::*;

    #[test]
    fn product_codes_pack_the_way_windows_installer_writes_them() {
        assert_eq!(
            packed_guid("{0A1B2C3D-1111-2222-3333-444455556666}").as_deref(),
            Some("D3C2B1A0111122223333444455556666")
        );
        assert_eq!(
            packed_guid("{AC76BA86-7AD7-1033-7B44-AC0F074E4100}").as_deref(),
            Some("68AB67CA7DA73301B744CAF070E41400")
        );
        assert_eq!(packed_guid("not a guid"), None);
    }

    #[test]
    fn component_paths_skip_registry_components() {
        assert_eq!(
            component_path(r"C:\Program Files\A\b.exe"),
            Some(PathBuf::from(r"C:\Program Files\A\b.exe"))
        );
        assert_eq!(component_path(r"02:\SOFTWARE\A\Key\"), None);
        assert_eq!(component_path(r"C?\Program Files\A\b.exe"), None);
    }
}
