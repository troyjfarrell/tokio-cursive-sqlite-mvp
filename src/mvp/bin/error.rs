use std::fmt::{Display, Formatter};

use crate::controller::ControllerState;
use crate::model::ModelState;
use crate::ui::UiState;

#[derive(Debug)]
pub enum Error {
    ControllerMissingStateReceiver,
    ControllerStateSenderClosed,
    InvalidControllerStateTransition(ControllerState, ControllerState),
    InvalidModelStateTransition(ModelState, ModelState),
    InvalidUiStateTransition(UiState, UiState),
    ModelMissingStateReceiver,
    ModelSchemaMigrateDownFailed(rusqlite_migration::Error),
    ModelSchemaMigrateToLatestFailed(rusqlite_migration::Error),
    ModelSchemaMigrateUpFailed(rusqlite_migration::Error),
    ModelSchemaMigrateDownNotAvailable,
    ModelSchemaMigrateUpNotAvailable,
    ModelSchemaValuesNotAvailable,
    UiMissingStateReceiver,
    UiWouldClobberStateReceiver,
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter) -> Result<(), std::fmt::Error> {
        match &*self {
            Self::ControllerMissingStateReceiver => {
                write!(f, "Controller is missing the state receiver")
            }
            Self::ControllerStateSenderClosed => {
                write!(f, "Controller state sender is already closed")
            }
            Self::InvalidControllerStateTransition(from, to) => write!(
                f,
                "Invalid ControllerState transition from {:?} to {:?}",
                from, to
            ),
            Self::InvalidModelStateTransition(from, to) => write!(
                f,
                "Invalid ModelState transition from {:?} to {:?}",
                from, to
            ),
            Self::InvalidUiStateTransition(from, to) => {
                write!(f, "Invalid UiState transition from {:?} to {:?}", from, to)
            }
            Self::ModelMissingStateReceiver => {
                write!(f, "Model is missing the state receiver")
            }
            Self::ModelSchemaMigrateDownFailed(msg) => {
                write!(
                    f,
                    "The requested model schema migration to a lower version failed: {}",
                    msg
                )
            }
            Self::ModelSchemaMigrateToLatestFailed(msg) => {
                write!(
                    f,
                    "The requested model schema migration to the latest version failed: {}",
                    msg
                )
            }
            Self::ModelSchemaMigrateUpFailed(msg) => {
                write!(
                    f,
                    "The requested model schema migration to a higher version failed: {}",
                    msg
                )
            }
            Self::ModelSchemaMigrateDownNotAvailable => {
                write!(
                    f,
                    "The requested model schema migration to a lower version is not available"
                )
            }
            Self::ModelSchemaMigrateUpNotAvailable => {
                write!(
                    f,
                    "The requested model schema migration to a lower version is not available"
                )
            }
            Self::ModelSchemaValuesNotAvailable => {
                write!(f, "The requested values are not available in the current version of the model schema")
            }
            Self::UiMissingStateReceiver => {
                write!(f, "Ui is missing the state receiver")
            }
            Self::UiWouldClobberStateReceiver => {
                write!(f, "Ui would clobber the state receiver")
            }
        }?;
        Ok(())
    }
}
