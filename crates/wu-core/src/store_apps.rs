//! Store packages are managed by Windows, never by the leftover scanner.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StoreApp {
    pub id: String,
    pub name: String,
    pub publisher: String,
    pub version: String,
}

/// Package full names contain no paths, quoting, wildcards or shell syntax.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 255
        && id.split('_').count() == 5
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-_".contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_package_identities_are_accepted() {
        assert!(valid_id("Contoso.Notes_1.0.0.0_x64__abcdefghijklm"));
        for id in [
            "",
            "*",
            "../app",
            "a_b_c_d_e;exit",
            "a_b_c_d_'",
            "a_b_c_d_\n",
        ] {
            assert!(!valid_id(id), "{id:?}");
        }
    }
}
