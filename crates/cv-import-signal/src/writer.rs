//! Writes encrypted local backups with synthetic content, for tests and
//! `tools/fixture-gen`. Never used on real data.
//!
//! The output follows the same layout and encryption as Signal's own local
//! backups (see the crate docs), so it can be cross-checked with
//! libsignal's `validator`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use aes::Aes256;
use aes::cipher::block_padding::Pkcs7;
use aes::cipher::{BlockModeEncrypt, KeyIvInit};
use flate2::Compression;
use flate2::write::GzEncoder;
use hmac::Mac;
use prost::Message;
use rand::{Rng, RngCore};
use sha2::{Digest, Sha256};

use crate::Result;
use crate::crypto::{HmacSha256, aes256_ctr32};
use crate::key::{BackupId, BackupKey, MessageBackupKey, RecoveryKey};
use crate::layout::{FILES_DIR, FILES_LIST, MAIN_FILE, METADATA_FILE, SNAPSHOT_PREFIX};
use crate::media::{local_media_name, padded_size};
use crate::proto::backup::{BackupInfo, Frame};
use crate::proto::local::{FilesFrame, Metadata, files_frame, metadata::EncryptedBackupId};

pub mod synth;

/// An attachment to store in the shared `files/` directory.
pub struct MediaBlob {
    pub local_key: [u8; 64],
    pub plaintext: Vec<u8>,
}

impl MediaBlob {
    pub fn plaintext_hash(&self) -> [u8; 32] {
        Sha256::digest(&self.plaintext).into()
    }

    pub fn media_name(&self) -> String {
        local_media_name(&self.plaintext_hash(), &self.local_key)
    }
}

/// Generates a random recovery key in Signal's canonical form.
pub fn generate_recovery_key(rng: &mut impl Rng) -> String {
    const ALPHABET: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    (0..RecoveryKey::LEN)
        .map(|_| ALPHABET[rng.random_range(0..ALPHABET.len())] as char)
        .collect()
}

/// Writes one snapshot (plus shared media) below `root`. Returns the
/// snapshot directory.
pub fn write_local_backup(
    root: &Path,
    recovery_key: &RecoveryKey,
    aci: &[u8; 16],
    info: &BackupInfo,
    frames: impl IntoIterator<Item = Frame>,
    media: &[MediaBlob],
    rng: &mut impl RngCore,
) -> Result<PathBuf> {
    let backup_key = BackupKey::from_recovery_key(recovery_key);
    let backup_id = backup_key.backup_id(aci);
    let message_key = backup_key.message_backup_key(&backup_id);

    let snapshot = root.join(snapshot_name(info.backup_time_ms));
    fs::create_dir_all(&snapshot)?;
    fs::write(
        snapshot.join(METADATA_FILE),
        encrypt_metadata(&backup_key, &backup_id, rng),
    )?;
    fs::write(
        snapshot.join(MAIN_FILE),
        encrypt_main(&message_key, info, frames, rng)?,
    )?;

    let mut list = Vec::new();
    for blob in media {
        let name = blob.media_name();
        let dir = root.join(FILES_DIR).join(&name[..2]);
        fs::create_dir_all(&dir)?;
        fs::write(dir.join(&name), encrypt_attachment(blob, rng))?;
        FilesFrame {
            item: Some(files_frame::Item::MediaName(name)),
        }
        .encode_length_delimited(&mut list)
        .expect("Vec has capacity");
    }
    fs::write(snapshot.join(FILES_LIST), list)?;
    Ok(snapshot)
}

pub fn encrypt_metadata(
    backup_key: &BackupKey,
    backup_id: &BackupId,
    rng: &mut impl RngCore,
) -> Vec<u8> {
    let mut iv = [0u8; 12];
    rng.fill_bytes(&mut iv);
    let mut id = backup_id.0;
    aes256_ctr32(&backup_key.local_metadata_key(), &iv, &mut id);
    Metadata {
        version: crate::metadata::SUPPORTED_METADATA_VERSION,
        backup_id: Some(EncryptedBackupId {
            iv: iv.to_vec(),
            encrypted_id: id.to_vec(),
        }),
    }
    .encode_to_vec()
}

/// `IV ‖ AES-256-CBC(gzip(delimited frames)) ‖ HMAC`.
pub fn encrypt_main(
    key: &MessageBackupKey,
    info: &BackupInfo,
    frames: impl IntoIterator<Item = Frame>,
    rng: &mut impl RngCore,
) -> Result<Vec<u8>> {
    let mut gz = GzEncoder::new(Vec::new(), Compression::default());
    let mut buf = Vec::new();
    info.encode_length_delimited(&mut buf)
        .expect("Vec has capacity");
    gz.write_all(&buf)?;
    for frame in frames {
        buf.clear();
        frame
            .encode_length_delimited(&mut buf)
            .expect("Vec has capacity");
        gz.write_all(&buf)?;
    }
    let compressed = gz.finish()?;
    Ok(encrypt_then_mac(
        &key.aes_key,
        &*key.hmac_key,
        &compressed,
        rng,
    ))
}

/// Pads with zeros to Signal's bucket size, then encrypts like `main`.
pub fn encrypt_attachment(blob: &MediaBlob, rng: &mut impl RngCore) -> Vec<u8> {
    let mut padded = blob.plaintext.clone();
    padded.resize(padded_size(blob.plaintext.len() as u64) as usize, 0);
    let aes_key: &[u8; 32] = blob.local_key[..32].try_into().expect("64-byte key");
    encrypt_then_mac(aes_key, &blob.local_key[32..], &padded, rng)
}

fn encrypt_then_mac(
    aes_key: &[u8; 32],
    hmac_key: &[u8],
    plaintext: &[u8],
    rng: &mut impl RngCore,
) -> Vec<u8> {
    let mut iv = [0u8; 16];
    rng.fill_bytes(&mut iv);
    let ciphertext = cbc::Encryptor::<Aes256>::new(aes_key.into(), &iv.into())
        .encrypt_padded_vec::<Pkcs7>(plaintext);
    let mut out = Vec::with_capacity(16 + ciphertext.len() + 32);
    out.extend_from_slice(&iv);
    out.extend_from_slice(&ciphertext);
    let mut mac = <HmacSha256 as hmac::KeyInit>::new_from_slice(hmac_key).expect("any key length");
    mac.update(&out);
    out.extend_from_slice(&mac.finalize().into_bytes());
    out
}

/// `signal-backup-yyyy-MM-dd-HH-mm-ss` in UTC, like Signal-Android.
pub fn snapshot_name(time_ms: u64) -> String {
    let secs = time_ms / 1000;
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let (y, m, d) = civil_from_days(days as i64);
    format!(
        "{SNAPSHOT_PREFIX}{y:04}-{m:02}-{d:02}-{:02}-{:02}-{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Days since 1970-01-01 to a proleptic Gregorian date (H. Hinnant).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_names_use_utc_dates() {
        assert_eq!(snapshot_name(0), "signal-backup-1970-01-01-00-00-00");
        assert_eq!(
            snapshot_name(1_709_251_199_000),
            "signal-backup-2024-02-29-23-59-59"
        );
    }
}
