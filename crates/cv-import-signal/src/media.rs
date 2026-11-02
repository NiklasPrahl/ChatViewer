//! Encrypted media (attachments) of a local backup.
//!
//! Each file is `IV (16) ‖ AES-256-CBC/PKCS7(plaintext ‖ zero padding) ‖ HMAC-SHA256 (32)`
//! under the attachment's 64-byte `localKey` (AES key ‖ HMAC key). The
//! file name is `hex(SHA-256(plaintextHash ‖ localKey))`.

use std::fs;
use std::path::Path;

use hmac::Mac;
use sha2::{Digest, Sha256};

use crate::crypto::{AES_BLOCK, HMAC_LEN, HmacSha256, aes256_cbc_decrypt, macs_equal};
use crate::proto::backup::FilePointer;
use crate::proto::backup::file_pointer::locator_info::IntegrityCheck;
use crate::{Error, Result};

pub const LOCAL_KEY_LEN: usize = 64;

/// What a `FilePointer` says about its local copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalMedia {
    pub media_name: String,
    pub local_key: Vec<u8>,
    pub plaintext_hash: Vec<u8>,
    pub size: u64,
}

impl LocalMedia {
    /// `None` if the attachment was not on the device when the backup was
    /// made (no `localKey`) or was never downloaded (no `plaintextHash`).
    pub fn from_pointer(pointer: &FilePointer) -> Option<Self> {
        let locator = pointer.locator_info.as_ref()?;
        let local_key = locator.local_key.as_ref()?;
        let Some(IntegrityCheck::PlaintextHash(hash)) = &locator.integrity_check else {
            return None;
        };
        Some(Self {
            media_name: local_media_name(hash, local_key),
            local_key: local_key.clone(),
            plaintext_hash: hash.clone(),
            size: u64::from(locator.size),
        })
    }
}

pub fn local_media_name(plaintext_hash: &[u8], local_key: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(plaintext_hash);
    hasher.update(local_key);
    hex::encode(hasher.finalize())
}

/// Signal's padding bucket size for a plaintext of `size` bytes.
pub fn padded_size(size: u64) -> u64 {
    let size = size.max(1) as f64;
    let padded = 1.05f64.powf((size.ln() / 1.05f64.ln()).ceil()).floor();
    (padded as u64).max(541)
}

/// Reads a snapshot's `files` list: the media names it references.
pub fn read_files_list(path: &Path) -> Result<Vec<String>> {
    use prost::Message;

    use crate::proto::local::FilesFrame;
    use crate::proto::local::files_frame::Item;

    let bytes = fs::read(path)?;
    let mut reader = bytes.as_slice();
    let mut buf = Vec::new();
    let mut names = Vec::new();
    while crate::main_file::read_delimited(&mut reader, &mut buf)? {
        if let Some(Item::MediaName(name)) = FilesFrame::decode(buf.as_slice())?.item {
            names.push(name);
        }
    }
    Ok(names)
}

/// How far a decrypted attachment could be verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Integrity {
    /// The content matches the recorded plaintext hash.
    Verified,
    /// The file is authentic (its HMAC under the attachment key is valid),
    /// but its content differs from the recorded size or hash. Signal
    /// Desktop backs up its local sticker copies this way and does not
    /// check them either. Shown as a warning, not an error.
    AuthenticatedOnly,
}

#[derive(Debug)]
pub struct DecryptedMedia {
    pub data: Vec<u8>,
    pub integrity: Integrity,
}

/// Decrypts one attachment file.
pub fn decrypt_file(path: &Path, media: &LocalMedia) -> Result<DecryptedMedia> {
    decrypt(&fs::read(path)?, media)
}

/// Decrypts an attachment. Fails only if the file is not authentic; a
/// size or hash mismatch is reported through [`Integrity`].
pub fn decrypt(file: &[u8], media: &LocalMedia) -> Result<DecryptedMedia> {
    let mut raw = decrypt_raw(file, &media.local_key)?;
    let hash_ok = |data: &[u8]| Sha256::digest(data).as_slice() == media.plaintext_hash.as_slice();
    let size = media.size as usize;

    if raw.len() >= size && hash_ok(&raw[..size]) {
        raw.truncate(size);
        return Ok(DecryptedMedia {
            data: raw,
            integrity: Integrity::Verified,
        });
    }
    // Stored without padding, but with a different recorded size.
    if hash_ok(&raw) {
        return Ok(DecryptedMedia {
            data: raw,
            integrity: Integrity::Verified,
        });
    }
    raw.truncate(size);
    Ok(DecryptedMedia {
        data: raw,
        integrity: Integrity::AuthenticatedOnly,
    })
}

/// Verifies the HMAC and decrypts, without removing Signal's zero padding
/// and without checking the plaintext hash.
pub fn decrypt_raw(file: &[u8], local_key: &[u8]) -> Result<Vec<u8>> {
    if local_key.len() != LOCAL_KEY_LEN {
        return Err(Error::Malformed("attachment key must be 64 bytes"));
    }
    if file.len() < AES_BLOCK * 2 + HMAC_LEN {
        return Err(Error::Malformed("attachment file is too short"));
    }
    let aes_key: &[u8; 32] = local_key[..32].try_into().expect("checked length");
    let hmac_key = &local_key[32..];

    let (content, mac) = file.split_at(file.len() - HMAC_LEN);
    let mut hmac = <HmacSha256 as Mac>::new_from_slice(hmac_key).expect("any key length");
    hmac.update(content);
    if !macs_equal(&hmac.finalize().into_bytes(), mac) {
        return Err(Error::MediaAuthenticationFailed);
    }

    let (iv, ciphertext) = content.split_at(AES_BLOCK);
    aes256_cbc_decrypt(aes_key, iv.try_into().expect("16 bytes"), ciphertext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padding_matches_signal_buckets() {
        assert_eq!(padded_size(0), 541);
        assert_eq!(padded_size(541), 541);
        assert_eq!(padded_size(542), 568);
        for size in [600u64, 10_000, 1_000_000] {
            let padded = padded_size(size);
            assert!(padded >= size && (padded as f64) < size as f64 * 1.06);
        }
    }

    /// Encrypts like Signal, optionally without zero padding (as Signal
    /// Desktop stores local sticker copies).
    fn encrypt(local_key: &[u8; 64], plaintext: &[u8], pad: bool) -> Vec<u8> {
        use aes::Aes256;
        use aes::cipher::block_padding::Pkcs7;
        use aes::cipher::{BlockEncryptMut, KeyIvInit};

        let mut data = plaintext.to_vec();
        if pad {
            data.resize(padded_size(plaintext.len() as u64) as usize, 0);
        }
        let iv = [3u8; 16];
        let key: &[u8; 32] = local_key[..32].try_into().unwrap();
        let mut out = iv.to_vec();
        out.extend(
            cbc::Encryptor::<Aes256>::new(key.into(), &iv.into())
                .encrypt_padded_vec_mut::<Pkcs7>(&data),
        );
        let mut mac = <HmacSha256 as Mac>::new_from_slice(&local_key[32..]).unwrap();
        mac.update(&out);
        out.extend_from_slice(&mac.finalize().into_bytes());
        out
    }

    fn media(local_key: &[u8; 64], hash_of: &[u8], size: usize) -> LocalMedia {
        LocalMedia {
            media_name: String::new(),
            local_key: local_key.to_vec(),
            plaintext_hash: Sha256::digest(hash_of).to_vec(),
            size: size as u64,
        }
    }

    #[test]
    fn padded_attachment_is_verified() {
        let key = [5u8; 64];
        let plain = b"synthetic image bytes".repeat(10);
        let d = decrypt(
            &encrypt(&key, &plain, true),
            &media(&key, &plain, plain.len()),
        )
        .unwrap();
        assert_eq!(d.integrity, Integrity::Verified);
        assert_eq!(d.data, plain);
    }

    #[test]
    fn unpadded_attachment_with_wrong_size_is_verified_by_hash() {
        let key = [5u8; 64];
        let plain = b"synthetic sticker".repeat(10);
        let d = decrypt(
            &encrypt(&key, &plain, false),
            &media(&key, &plain, plain.len() + 7),
        )
        .unwrap();
        assert_eq!(d.integrity, Integrity::Verified);
        assert_eq!(d.data, plain);
    }

    #[test]
    fn authentic_attachment_with_other_content_is_flagged_not_rejected() {
        let key = [5u8; 64];
        let plain = b"synthetic sticker, re-encoded".repeat(10);
        let d = decrypt(
            &encrypt(&key, &plain, false),
            &media(&key, b"original", plain.len() + 7),
        )
        .unwrap();
        assert_eq!(d.integrity, Integrity::AuthenticatedOnly);
        assert_eq!(d.data, plain);
    }

    #[test]
    fn foreign_key_is_rejected() {
        let plain = b"synthetic".repeat(10);
        let file = encrypt(&[5u8; 64], &plain, true);
        assert!(matches!(
            decrypt(&file, &media(&[6u8; 64], &plain, plain.len())),
            Err(Error::MediaAuthenticationFailed)
        ));
    }

    #[test]
    fn media_name_is_hex_sha256() {
        let name = local_media_name(&[1; 32], &[2; 64]);
        assert_eq!(name.len(), 64);
        assert!(name.bytes().all(|b| b.is_ascii_hexdigit()));
    }
}
