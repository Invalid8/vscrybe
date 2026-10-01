use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Couldn't open {0}.")]
    Unreadable(PathBuf),
    #[error("The file is damaged or isn't an audio format that can be decoded.")]
    Undecodable(String),
    #[error("Transcription failed: {0}")]
    Transcription(String),
}
