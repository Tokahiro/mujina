//! The per-user home app setting of the full screen experience.
//!
//! Windows' own settings page writes the same value; on builds where the page lost its picker
//! this is the only way to choose a home app.

use mujina_application::ports::{HomeAppRegistry, PortError, PortResult};
use mujina_winutil::registry::{self, Hive, RegistryError};

const KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\GamingConfiguration";
const VALUE: &str = "GamingHomeApp";
const BACKUP_VALUE: &str = "GamingHomeApp.MujinaBackup";

#[derive(Debug, Default)]
pub struct WindowsHomeAppRegistry;

fn failed(error: RegistryError) -> PortError {
    PortError::Failed(format!("{KEY}: {error}"))
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|text| !text.is_empty())
}

impl HomeAppRegistry for WindowsHomeAppRegistry {
    fn current(&self) -> PortResult<Option<String>> {
        registry::read_string(Hive::CurrentUser, KEY, VALUE)
            .map(non_empty)
            .map_err(failed)
    }

    fn set(&self, app_user_model_id: &str) -> PortResult<()> {
        registry::write_string(Hive::CurrentUser, KEY, VALUE, app_user_model_id).map_err(failed)
    }

    fn clear(&self) -> PortResult<()> {
        registry::delete_value(Hive::CurrentUser, KEY, VALUE).map_err(failed)
    }

    fn backup(&self) -> PortResult<Option<String>> {
        registry::read_string(Hive::CurrentUser, KEY, BACKUP_VALUE)
            .map(non_empty)
            .map_err(failed)
    }

    fn set_backup(&self, app_user_model_id: Option<&str>) -> PortResult<()> {
        match app_user_model_id {
            Some(id) => registry::write_string(Hive::CurrentUser, KEY, BACKUP_VALUE, id),
            None => registry::delete_value(Hive::CurrentUser, KEY, BACKUP_VALUE),
        }
        .map_err(failed)
    }
}
