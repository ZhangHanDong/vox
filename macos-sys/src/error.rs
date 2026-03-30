use thiserror::Error;

#[derive(Error, Debug)]
pub enum MacosError {
    #[error("Failed to create CGEvent tap — accessibility permission required")]
    EventTapCreationFailed,

    #[error("Failed to create run loop source")]
    RunLoopSourceFailed,

    #[error("Status bar creation failed")]
    StatusBarFailed,

    #[error("Input source not found: {0}")]
    InputSourceNotFound(String),

    #[error("Input source switch failed: {0}")]
    InputSourceSwitchFailed(String),

    #[error("Platform error: {0}")]
    Platform(String),
}
