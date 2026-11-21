mod capture;
mod process;
mod recorder;

pub use capture::{run_capture, AudioSource, CaptureStatus, CAPTURE_CHANNELS, CAPTURE_SAMPLE_RATE};
pub use process::find_pid;
pub use recorder::{spawn_recorder, RecordStats};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("ошибка захвата звука: {0}")]
    Capture(String),
    #[error("ошибка записи WAV: {0}")]
    Recording(String),
}
