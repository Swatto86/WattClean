//! What the program's install log recorded it creating: certain leftovers,
//! in every mode, when they still exist after its uninstaller.

use super::{Confidence, Context, Found, Mode};
use crate::identity::Identity;
use crate::install_log::Recorded;
use crate::model::find_value;
use crate::scan::Target;
use crate::system::key_exists;

const REASON: &str = "Recorded when it was installed";

pub(super) fn scan(identity: &Identity, _mode: Mode, context: &Context<'_>, found: &mut Found) {
    let Some(log) = context.log else {
        return;
    };
    for item in log
        .items
        .iter()
        .filter(|item| item.recorded == Recorded::Created)
    {
        match &item.target {
            Target::Key { key } => {
                if key_exists(context.system, key) {
                    found.key(context, key.clone(), REASON, Confidence::High);
                }
            }
            Target::Value { key, name } => {
                let values = context.system.values(key).unwrap_or_default();
                if find_value(&values, name).is_some() {
                    found.value(context, key.clone(), name.clone(), REASON, Confidence::High);
                }
            }
            Target::Folder { path } | Target::File { path } => {
                found.path(context, identity, path.clone(), REASON, Confidence::High);
            }
        }
    }
}
