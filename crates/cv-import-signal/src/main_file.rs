//! The encrypted `main` file of a snapshot.
//!
//! ```text
//! IV (16) ‖ AES-256-CBC/PKCS7( gzip( varint-len ‖ BackupInfo ‖ { varint-len ‖ Frame }* ) ) ‖ HMAC-SHA256 (32)
//! ```
//!
//! The HMAC covers everything before it and is verified in a first pass
//! before any plaintext is used (as libsignal does).

use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom, Take};
use std::path::Path;

use flate2::read::GzDecoder;
use hmac::Mac;
use prost::Message;

use crate::crypto::{AES_BLOCK, CbcDecryptReader, HMAC_LEN, HmacSha256, macs_equal, read_full};
use crate::key::MessageBackupKey;
use crate::proto::backup::{BackupInfo, Frame};
use crate::{Error, Result};

/// Header of server-hosted backups with forward secrecy. Local backups do
/// not have it.
pub const FORWARD_SECRECY_MAGIC: &[u8; 8] = b"SBACKUP\x01";
/// Largest accepted frame (same limit as Signal-Android).
pub const MAX_FRAME_LEN: u64 = 25 * 1024 * 1024;

type Plaintext = BufReader<GzDecoder<BufReader<CbcDecryptReader<Take<BufReader<File>>>>>>;

/// Streams the frames of a verified `main` file.
pub struct MainReader {
    info: BackupInfo,
    plaintext: Plaintext,
    frame_buf: Vec<u8>,
}

impl MainReader {
    /// Verifies the HMAC, then opens the decrypted stream and reads the
    /// `BackupInfo` header.
    pub fn open(path: &Path, key: &MessageBackupKey) -> Result<Self> {
        let content_len = verify_hmac(path, key)?;

        let mut file = BufReader::with_capacity(256 * 1024, File::open(path)?);
        let mut iv = [0u8; AES_BLOCK];
        file.read_exact(&mut iv)?;
        let ciphertext = file.take(content_len - AES_BLOCK as u64);
        let decrypted = CbcDecryptReader::new(ciphertext, &key.aes_key, &iv);
        let mut plaintext = BufReader::with_capacity(
            256 * 1024,
            GzDecoder::new(BufReader::with_capacity(256 * 1024, decrypted)),
        );

        let mut frame_buf = Vec::new();
        if !read_delimited(&mut plaintext, &mut frame_buf)? {
            return Err(Error::Malformed("backup has no BackupInfo header"));
        }
        let info = BackupInfo::decode(frame_buf.as_slice())?;
        Ok(Self {
            info,
            plaintext,
            frame_buf,
        })
    }

    pub fn info(&self) -> &BackupInfo {
        &self.info
    }

    /// Encoded size of the frame returned last. If it differs from the
    /// decoded frame's `encoded_len()`, the frame had fields this schema
    /// does not know (prost drops them on decoding).
    pub fn last_frame_len(&self) -> usize {
        self.frame_buf.len()
    }

    /// Encoded bytes of the frame returned last (kept as raw record, ADR 0006).
    pub fn last_frame_bytes(&self) -> &[u8] {
        &self.frame_buf
    }

    /// Next frame, or `None` at the end of the backup.
    pub fn next_frame(&mut self) -> Result<Option<Frame>> {
        if !read_delimited(&mut self.plaintext, &mut self.frame_buf)? {
            return Ok(None);
        }
        Ok(Some(Frame::decode(self.frame_buf.as_slice())?))
    }
}

impl Iterator for MainReader {
    type Item = Result<Frame>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_frame().transpose()
    }
}

/// Checks the trailing HMAC. Returns the length of the authenticated
/// content (IV + ciphertext).
pub fn verify_hmac(path: &Path, key: &MessageBackupKey) -> Result<u64> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let min_len = (AES_BLOCK * 2 + HMAC_LEN) as u64;
    if len < min_len {
        return Err(Error::Malformed("main file is too short"));
    }

    let mut magic = [0u8; 8];
    file.read_exact(&mut magic)?;
    if &magic == FORWARD_SECRECY_MAGIC {
        return Err(Error::ForwardSecrecyNotSupported);
    }
    file.seek(SeekFrom::Start(0))?;

    let content_len = len - HMAC_LEN as u64;
    let mut mac =
        <HmacSha256 as hmac::KeyInit>::new_from_slice(&*key.hmac_key).expect("any key length");
    let mut content = BufReader::with_capacity(256 * 1024, file).take(content_len);
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = read_full(&mut content, &mut buf)?;
        if n == 0 {
            break;
        }
        mac.update(&buf[..n]);
    }
    let mut file = content.into_inner();
    let mut expected = [0u8; HMAC_LEN];
    file.read_exact(&mut expected)?;
    if !macs_equal(&mac.finalize().into_bytes(), &expected) {
        return Err(Error::AuthenticationFailed);
    }
    Ok(content_len)
}

/// Reads one varint-length-delimited record into `buf`. Returns `false`
/// on a clean end of stream.
pub fn read_delimited(reader: &mut impl Read, buf: &mut Vec<u8>) -> Result<bool> {
    let Some(len) = read_varint(reader)? else {
        return Ok(false);
    };
    if len > MAX_FRAME_LEN {
        return Err(Error::FrameTooLarge(len));
    }
    buf.clear();
    buf.resize(len as usize, 0);
    reader.read_exact(buf).map_err(|e| match e.kind() {
        io::ErrorKind::UnexpectedEof => Error::Malformed("truncated frame"),
        _ => Error::Io(e),
    })?;
    Ok(true)
}

fn read_varint(reader: &mut impl Read) -> Result<Option<u64>> {
    let mut value = 0u64;
    for i in 0..10 {
        let mut byte = [0u8];
        if reader.read(&mut byte)? == 0 {
            return if i == 0 {
                Ok(None)
            } else {
                Err(Error::Malformed("truncated length prefix"))
            };
        }
        value |= u64::from(byte[0] & 0x7f) << (7 * i);
        if byte[0] & 0x80 == 0 {
            return Ok(Some(value));
        }
    }
    Err(Error::Malformed("length prefix is too long"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_delimited_records() {
        let mut data = Vec::new();
        for payload in [&b""[..], b"a", &[7u8; 300]] {
            prost::encoding::encode_varint(payload.len() as u64, &mut data);
            data.extend_from_slice(payload);
        }
        let mut reader = data.as_slice();
        let mut buf = Vec::new();
        let mut lens = Vec::new();
        while read_delimited(&mut reader, &mut buf).unwrap() {
            lens.push(buf.len());
        }
        assert_eq!(lens, [0, 1, 300]);
    }

    #[test]
    fn rejects_truncated_and_oversized_records() {
        let mut buf = Vec::new();
        let mut truncated = &[5u8, 1, 2][..];
        assert!(matches!(
            read_delimited(&mut truncated, &mut buf),
            Err(Error::Malformed(_))
        ));
        let mut huge = Vec::new();
        prost::encoding::encode_varint(MAX_FRAME_LEN + 1, &mut huge);
        assert!(matches!(
            read_delimited(&mut huge.as_slice(), &mut buf),
            Err(Error::FrameTooLarge(_))
        ));
    }
}
