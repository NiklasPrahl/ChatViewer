use std::path::PathBuf;

/// Errors never contain message content or key material.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid recovery key: {0}")]
    InvalidRecoveryKey(&'static str),

    #[error("not a Signal backup folder: {}", .0.display())]
    NotABackup(PathBuf),

    #[error(
        "backup created by a newer Signal version (metadata version {found}); \
         this viewer supports version {supported}"
    )]
    UnsupportedMetadataVersion { found: u32, supported: u32 },

    #[error(
        "backup uses forward secrecy (server-hosted backup format), which needs a server token"
    )]
    ForwardSecrecyNotSupported,

    #[error("wrong recovery key or damaged backup (authentication failed)")]
    AuthenticationFailed,

    #[error("malformed backup: {0}")]
    Malformed(&'static str),

    #[error("frame of {0} bytes exceeds the size limit")]
    FrameTooLarge(u64),

    #[error("protobuf decoding failed: {0}")]
    Decode(#[from] prost::DecodeError),

    #[error("attachment is damaged or does not match its key (authentication failed)")]
    MediaAuthenticationFailed,
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
