//! The unencrypted `metadata` file of a snapshot (`LocalBackup.proto`).
//!
//! It stores the backup ID encrypted with AES-256-CTR under the local
//! metadata key, so the recovery key alone is enough to open the backup.

use std::fs;
use std::path::Path;

use prost::Message;

use crate::crypto::aes256_ctr32;
use crate::key::{BackupId, BackupKey};
use crate::proto::local::Metadata;
use crate::{Error, Result};

/// Highest metadata version this reader understands (same as Signal-Android).
pub const SUPPORTED_METADATA_VERSION: u32 = 1;
const MAX_METADATA_LEN: u64 = 16 * 1024;

#[derive(Debug, Clone)]
pub struct LocalMetadata {
    pub version: u32,
    iv: [u8; 12],
    encrypted_id: [u8; 16],
}

impl LocalMetadata {
    pub fn read(path: &Path) -> Result<Self> {
        if fs::metadata(path)?.len() > MAX_METADATA_LEN {
            return Err(Error::Malformed("metadata file is too large"));
        }
        Self::decode(&fs::read(path)?)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let metadata = Metadata::decode(bytes)?;
        if metadata.version > SUPPORTED_METADATA_VERSION {
            return Err(Error::UnsupportedMetadataVersion {
                found: metadata.version,
                supported: SUPPORTED_METADATA_VERSION,
            });
        }
        let id = metadata
            .backup_id
            .ok_or(Error::Malformed("metadata has no backup ID"))?;
        let iv = id
            .iv
            .as_slice()
            .try_into()
            .map_err(|_| Error::Malformed("metadata IV must be 12 bytes"))?;
        let encrypted_id = id
            .encrypted_id
            .as_slice()
            .try_into()
            .map_err(|_| Error::Malformed("encrypted backup ID must be 16 bytes"))?;
        Ok(Self {
            version: metadata.version,
            iv,
            encrypted_id,
        })
    }

    /// Decrypts the backup ID. There is no integrity check on this field:
    /// a wrong key yields a wrong ID, which is detected by the HMAC of the
    /// `main` file.
    pub fn backup_id(&self, backup_key: &BackupKey) -> BackupId {
        let mut id = self.encrypted_id;
        aes256_ctr32(&backup_key.local_metadata_key(), &self.iv, &mut id);
        BackupId(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::local::metadata::EncryptedBackupId;

    #[test]
    fn rejects_newer_version() {
        let bytes = Metadata {
            version: 2,
            backup_id: Some(EncryptedBackupId {
                iv: vec![0; 12],
                encrypted_id: vec![0; 16],
            }),
        }
        .encode_to_vec();
        assert!(matches!(
            LocalMetadata::decode(&bytes),
            Err(Error::UnsupportedMetadataVersion {
                found: 2,
                supported: 1
            })
        ));
    }

    #[test]
    fn rejects_missing_backup_id() {
        let bytes = Metadata {
            version: 1,
            backup_id: None,
        }
        .encode_to_vec();
        assert!(matches!(
            LocalMetadata::decode(&bytes),
            Err(Error::Malformed(_))
        ));
    }
}
