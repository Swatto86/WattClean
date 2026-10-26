//! Current-user Store packages have no registry-uninstall or leftover phase.

use std::sync::Arc;

use tauri::State;
use wu_core::history::{self, Counts, Entry, Uninstaller};
use wu_core::store_apps::{StoreApp, valid_id};

use super::{session::now, state::AppState};
use crate::error::{CommandResult, blocking};

#[tauri::command]
pub async fn un_list_store_apps(state: State<'_, Arc<AppState>>) -> CommandResult<Vec<StoreApp>> {
    let app = Arc::clone(&state);
    blocking(move || Ok(app.system.store_apps()?)).await
}

#[tauri::command]
pub async fn un_remove_store_app(
    state: State<'_, Arc<AppState>>,
    package_id: String,
) -> CommandResult<()> {
    let app = Arc::clone(&state);
    blocking(move || remove(&app, &package_id)).await
}

fn remove(app: &AppState, id: &str) -> CommandResult<()> {
    if !valid_id(id) {
        return Err("Invalid Store package identity.".into());
    }
    let _serial = app
        .store_operation
        .try_lock()
        .map_err(|_| "Another Store app operation is running.")?;
    let _removing = app.begin_removal();
    let package = app
        .system
        .store_apps()?
        .into_iter()
        .find(|p| p.id == id)
        .ok_or("That Store app is no longer installed; refresh the list.")?;
    let result = app.system.remove_store_app(id);
    let uninstaller = match &result {
        Ok(()) => Uninstaller::Finished,
        Err(error) => Uninstaller::Failed {
            error: error.to_string(),
        },
    };
    let record = history::append(
        &app.data_dir,
        Entry {
            id: format!("{}-store-{}", now(), package.id),
            time: now(),
            program: package.name,
            publisher: Some(package.publisher),
            version: Some(package.version),
            mode: None,
            uninstaller,
            removed: Counts::default(),
            at_restart: 0,
            failed: u32::from(result.is_err()),
            bytes: 0,
            backup_id: None,
        },
    );
    match (result, record) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error.into()),
        (Ok(()), Err(error)) => {
            Err(format!("The app was removed, but its history could not be saved: {error}").into())
        }
        (Err(error), Err(save)) => {
            Err(format!("{error} History could not be saved: {save}").into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wu_core::{System, fake::FakeSystem};

    #[test]
    fn store_removal_is_exact_and_records_success_and_failure() {
        let dir = tempfile::tempdir().unwrap();
        let system = Arc::new(FakeSystem::seeded(&dir.path().join("machine")).unwrap());
        let apps = system.store_apps().unwrap();
        assert_eq!(apps.len(), 2);
        let app = AppState::new(
            system.clone(),
            dir.path().join("uninstall"),
            dir.path(),
            true,
        );
        assert!(remove(&app, "*").is_err());
        assert!(remove(&app, &apps[1].id).is_err());
        assert_eq!(system.store_apps().unwrap().len(), 2);
        remove(&app, &apps[0].id).unwrap();
        assert_eq!(system.store_apps().unwrap(), vec![apps[1].clone()]);
        assert!(remove(&app, &apps[0].id).is_err());
        assert!(!app.is_removing());
        let entries = history::load(&app.data_dir).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(
            entries
                .iter()
                .any(|e| matches!(e.uninstaller, Uninstaller::Finished))
        );
        assert!(
            entries
                .iter()
                .any(|e| matches!(e.uninstaller, Uninstaller::Failed { .. }))
        );
    }
}
