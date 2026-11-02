//! Key derivation from the recovery key (Account Entropy Pool).
//!
//! All derivations are HKDF-SHA256, mirroring libsignal
//! (`rust/account-keys/src/backup.rs`, `rust/message-backup/src/key.rs`).

use std::fmt;
use std::str::FromStr;

use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::crypto::hkdf_sha256;
use crate::{Error, Result};

/// The 64-character recovery key. Kept zeroized and never printed.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct RecoveryKey([u8; RecoveryKey::LEN]);

impl RecoveryKey {
    pub const LEN: usize = 64;

    /// Signal's alphabet: digits and lowercase ASCII letters.
    fn is_valid_char(c: u8) -> bool {
        c.is_ascii_digit() || c.is_ascii_lowercase()
    }

    pub fn as_bytes(&self) -> &[u8; Self::LEN] {
        &self.0
    }
}

impl FromStr for RecoveryKey {
    type Err = Error;

    /// Accepts the key as displayed or typed: case-insensitive, with spaces,
    /// dashes or line breaks between groups. Like libsignal's validator,
    /// `#` is read as `o` and `=` as `0`.
    fn from_str(input: &str) -> Result<Self> {
        let mut key = [0u8; Self::LEN];
        let mut len = 0;
        for c in input.bytes() {
            let c = match c {
                b' ' | b'-' | b'\t' | b'\r' | b'\n' => continue,
                b'#' => b'o',
                b'=' => b'0',
                c => c.to_ascii_lowercase(),
            };
            if !Self::is_valid_char(c) {
                key.zeroize();
                return Err(Error::InvalidRecoveryKey("unexpected character"));
            }
            if len == Self::LEN {
                key.zeroize();
                return Err(Error::InvalidRecoveryKey("more than 64 characters"));
            }
            key[len] = c;
            len += 1;
        }
        if len != Self::LEN {
            key.zeroize();
            return Err(Error::InvalidRecoveryKey("fewer than 64 characters"));
        }
        Ok(Self(key))
    }
}

impl fmt::Debug for RecoveryKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RecoveryKey(<redacted>)")
    }
}

/// Root key for backups, derived from the recovery key.
pub struct BackupKey(Zeroizing<[u8; 32]>);

/// Identifies the backup; derived from the backup key and the account ACI.
/// Local backups store it encrypted in the `metadata` file.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BackupId(pub [u8; 16]);

impl fmt::Debug for BackupId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("BackupId(<redacted>)")
    }
}

/// Keys for the `main` file: HMAC-SHA256 and AES-256-CBC.
pub struct MessageBackupKey {
    pub hmac_key: Zeroizing<[u8; 32]>,
    pub aes_key: Zeroizing<[u8; 32]>,
}

impl BackupKey {
    pub fn from_recovery_key(key: &RecoveryKey) -> Self {
        let mut out = Zeroizing::new([0u8; 32]);
        hkdf_sha256(
            key.as_bytes(),
            None,
            &[b"20240801_SIGNAL_BACKUP_KEY"],
            &mut *out,
        );
        Self(out)
    }

    /// Key that encrypts the backup ID in a local backup's `metadata` file.
    pub fn local_metadata_key(&self) -> Zeroizing<[u8; 32]> {
        let mut out = Zeroizing::new([0u8; 32]);
        hkdf_sha256(
            &*self.0,
            None,
            &[b"20241011_SIGNAL_LOCAL_BACKUP_METADATA_KEY"],
            &mut *out,
        );
        out
    }

    /// Backup ID for an account. `aci` is the 16-byte account UUID (the
    /// binary service ID of an ACI has no type prefix).
    pub fn backup_id(&self, aci: &[u8; 16]) -> BackupId {
        let mut out = [0u8; 16];
        hkdf_sha256(
            &*self.0,
            None,
            &[b"20241024_SIGNAL_BACKUP_ID:", aci],
            &mut out,
        );
        BackupId(out)
    }

    /// Keys for the `main` file of a local backup. Local backups have no
    /// forward-secrecy token, so the original (2024-10-07) info string is
    /// used without salt.
    pub fn message_backup_key(&self, backup_id: &BackupId) -> MessageBackupKey {
        let mut out = Zeroizing::new([0u8; 64]);
        hkdf_sha256(
            &*self.0,
            None,
            &[
                b"20241007_SIGNAL_BACKUP_ENCRYPT_MESSAGE_BACKUP:",
                &backup_id.0,
            ],
            &mut *out,
        );
        let mut hmac_key = Zeroizing::new([0u8; 32]);
        let mut aes_key = Zeroizing::new([0u8; 32]);
        hmac_key.copy_from_slice(&out[..32]);
        aes_key.copy_from_slice(&out[32..]);
        MessageBackupKey { hmac_key, aes_key }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test vectors from libsignal `rust/message-backup/src/key.rs`
    // (`message_backup_key_legacy`), v0.106.1. Synthetic values.
    const LIBSIGNAL_FAKE_AEP: &str =
        "dtjs858asj6tv0jzsqrsmj0ubp335pisj98e9ssnss8myoc08drhtcktyawvx45l"; // gitleaks:allow
    const LIBSIGNAL_FAKE_ACI: &str = "659aa5f4a28dfcc11ea1b997537a3d95";
    const EXPECTED_HMAC_KEY: &str =
        "f425e22a607c529717e1e1b29f9fe139f9d1c7e7d01e371c7753c544a3026649";
    const EXPECTED_AES_KEY: &str =
        "e143f4ad5668d8bfed2f88562f0693f53bda2c0e55c9d71730f30e24695fd6df";

    #[test]
    fn matches_libsignal_test_vector() {
        let recovery: RecoveryKey = LIBSIGNAL_FAKE_AEP.parse().unwrap();
        let aci: [u8; 16] = hex::decode(LIBSIGNAL_FAKE_ACI).unwrap().try_into().unwrap();
        let backup_key = BackupKey::from_recovery_key(&recovery);
        let key = backup_key.message_backup_key(&backup_key.backup_id(&aci));
        assert_eq!(hex::encode(*key.hmac_key), EXPECTED_HMAC_KEY);
        assert_eq!(hex::encode(*key.aes_key), EXPECTED_AES_KEY);
    }

    #[test]
    fn parses_display_formats() {
        let canonical: RecoveryKey = LIBSIGNAL_FAKE_AEP.parse().unwrap();
        let grouped = LIBSIGNAL_FAKE_AEP
            .as_bytes()
            .chunks(4)
            .map(|c| std::str::from_utf8(c).unwrap().to_ascii_uppercase())
            .collect::<Vec<_>>()
            .join(" ");
        let parsed: RecoveryKey = grouped.parse().unwrap();
        assert_eq!(parsed.as_bytes(), canonical.as_bytes());
    }

    #[test]
    fn rejects_invalid_keys() {
        assert!("abc".parse::<RecoveryKey>().is_err());
        assert!(
            format!("{LIBSIGNAL_FAKE_AEP}a")
                .parse::<RecoveryKey>()
                .is_err()
        );
        assert!(
            LIBSIGNAL_FAKE_AEP
                .replace('d', "!")
                .parse::<RecoveryKey>()
                .is_err()
        );
    }

    #[test]
    fn debug_output_is_redacted() {
        let key: RecoveryKey = LIBSIGNAL_FAKE_AEP.parse().unwrap();
        assert_eq!(format!("{key:?}"), "RecoveryKey(<redacted>)");
    }
}
