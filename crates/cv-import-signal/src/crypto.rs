//! Cryptographic building blocks used by Signal backups.

use std::io::{self, Read};

use aes::Aes256;
use aes::cipher::generic_array::GenericArray;
use aes::cipher::{BlockDecryptMut, KeyIvInit, StreamCipher};
use hkdf::Hkdf;
use hmac::Hmac;
use sha2::Sha256;
use subtle::ConstantTimeEq;

use crate::{Error, Result};

pub const AES_BLOCK: usize = 16;
pub const HMAC_LEN: usize = 32;

pub type HmacSha256 = Hmac<Sha256>;
type Aes256CbcDec = cbc::Decryptor<Aes256>;

/// HKDF-SHA256 expand with the info given as concatenated parts.
pub fn hkdf_sha256(ikm: &[u8], salt: Option<&[u8]>, info: &[&[u8]], out: &mut [u8]) {
    Hkdf::<Sha256>::new(salt, ikm)
        .expand_multi_info(info, out)
        .expect("output length is valid for HKDF-SHA256");
}

/// AES-256-CTR with a 12-byte nonce and a 32-bit big-endian block counter
/// starting at 0 (Signal's `Aes256Ctr32`). Encryption and decryption are
/// the same operation.
pub fn aes256_ctr32(key: &[u8; 32], nonce: &[u8; 12], data: &mut [u8]) {
    let mut iv = [0u8; AES_BLOCK];
    iv[..12].copy_from_slice(nonce);
    let mut cipher = ctr::Ctr32BE::<Aes256>::new(key.into(), &iv.into());
    cipher.apply_keystream(data);
}

/// Constant-time comparison of two MACs.
pub fn macs_equal(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && bool::from(a.ct_eq(b))
}

/// Removes PKCS#7 padding from a decrypted buffer.
pub fn strip_pkcs7(data: &[u8]) -> Result<&[u8]> {
    let pad = *data.last().ok_or(Error::Malformed("empty ciphertext"))? as usize;
    if pad == 0 || pad > AES_BLOCK || pad > data.len() {
        return Err(Error::Malformed("invalid padding"));
    }
    if !data[data.len() - pad..].iter().all(|&b| b as usize == pad) {
        return Err(Error::Malformed("invalid padding"));
    }
    Ok(&data[..data.len() - pad])
}

/// Decrypts an in-memory AES-256-CBC ciphertext with PKCS#7 padding.
pub fn aes256_cbc_decrypt(key: &[u8; 32], iv: &[u8; 16], ciphertext: &[u8]) -> Result<Vec<u8>> {
    if ciphertext.is_empty() || !ciphertext.len().is_multiple_of(AES_BLOCK) {
        return Err(Error::Malformed(
            "ciphertext length is not a multiple of the block size",
        ));
    }
    let mut buf = ciphertext.to_vec();
    let mut dec = Aes256CbcDec::new(key.into(), iv.into());
    for block in buf.as_chunks_mut::<AES_BLOCK>().0 {
        dec.decrypt_block_mut(GenericArray::from_mut_slice(block));
    }
    let len = strip_pkcs7(&buf)?.len();
    buf.truncate(len);
    Ok(buf)
}

/// Streaming AES-256-CBC decryption with PKCS#7 padding removal.
///
/// The inner reader must yield exactly the ciphertext (without IV or MAC).
/// The last block is held back until the end of input so the padding can
/// be removed.
pub struct CbcDecryptReader<R> {
    inner: R,
    dec: Aes256CbcDec,
    /// Decrypted bytes ready to be returned.
    out: Vec<u8>,
    out_pos: usize,
    /// Last decrypted block, withheld until we know whether it is final.
    held: Option<[u8; AES_BLOCK]>,
    done: bool,
    chunk: Vec<u8>,
}

impl<R: Read> CbcDecryptReader<R> {
    const CHUNK: usize = 64 * 1024;

    pub fn new(inner: R, key: &[u8; 32], iv: &[u8; 16]) -> Self {
        Self {
            inner,
            dec: Aes256CbcDec::new(key.into(), iv.into()),
            out: Vec::with_capacity(Self::CHUNK + AES_BLOCK),
            out_pos: 0,
            held: None,
            done: false,
            chunk: vec![0; Self::CHUNK],
        }
    }

    fn fill(&mut self) -> io::Result<()> {
        self.out.clear();
        self.out_pos = 0;
        let n = read_full(&mut self.inner, &mut self.chunk)?;
        if n % AES_BLOCK != 0 {
            return Err(invalid_data(
                "ciphertext length is not a multiple of the block size",
            ));
        }
        if let Some(held) = self.held.take() {
            self.out.extend_from_slice(&held);
        }
        for block in self.chunk[..n].as_chunks_mut::<AES_BLOCK>().0 {
            self.dec
                .decrypt_block_mut(GenericArray::from_mut_slice(block));
        }
        self.out.extend_from_slice(&self.chunk[..n]);

        if n < self.chunk.len() {
            // End of input: the last block carries the padding.
            self.done = true;
            let len = strip_pkcs7(&self.out)
                .map_err(|_| invalid_data("invalid padding"))?
                .len();
            self.out.truncate(len);
        } else {
            let start = self.out.len() - AES_BLOCK;
            let mut held = [0u8; AES_BLOCK];
            held.copy_from_slice(&self.out[start..]);
            self.held = Some(held);
            self.out.truncate(start);
        }
        Ok(())
    }
}

impl<R: Read> Read for CbcDecryptReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        while self.out_pos == self.out.len() {
            if self.done {
                return Ok(0);
            }
            self.fill()?;
        }
        let n = buf.len().min(self.out.len() - self.out_pos);
        buf[..n].copy_from_slice(&self.out[self.out_pos..self.out_pos + n]);
        self.out_pos += n;
        Ok(n)
    }
}

/// Reads until `buf` is full or the reader is exhausted.
pub fn read_full(reader: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(filled)
}

fn invalid_data(msg: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

#[cfg(test)]
mod tests {
    use aes::cipher::BlockEncryptMut;
    use aes::cipher::block_padding::Pkcs7;

    use super::*;

    fn encrypt(key: &[u8; 32], iv: &[u8; 16], plain: &[u8]) -> Vec<u8> {
        cbc::Encryptor::<Aes256>::new(key.into(), iv.into()).encrypt_padded_vec_mut::<Pkcs7>(plain)
    }

    #[test]
    fn streaming_cbc_matches_in_memory_for_many_lengths() {
        let key = [7u8; 32];
        let iv = [9u8; 16];
        // Lengths around block and chunk boundaries.
        let chunk = CbcDecryptReader::<&[u8]>::CHUNK;
        for len in [
            0,
            1,
            15,
            16,
            17,
            chunk - 17,
            chunk - 16,
            chunk - 1,
            chunk,
            chunk + 1,
            3 * chunk + 5,
        ] {
            let plain: Vec<u8> = (0..len).map(|i| (i * 31 % 251) as u8).collect();
            let ct = encrypt(&key, &iv, &plain);
            assert_eq!(
                aes256_cbc_decrypt(&key, &iv, &ct).unwrap(),
                plain,
                "len {len}"
            );
            let mut out = Vec::new();
            CbcDecryptReader::new(ct.as_slice(), &key, &iv)
                .read_to_end(&mut out)
                .unwrap();
            assert_eq!(out, plain, "streaming len {len}");
        }
    }

    #[test]
    fn ctr_round_trips() {
        let key = [1u8; 32];
        let nonce = [2u8; 12];
        let mut data = *b"sixteen byte id!";
        aes256_ctr32(&key, &nonce, &mut data);
        assert_ne!(&data, b"sixteen byte id!");
        aes256_ctr32(&key, &nonce, &mut data);
        assert_eq!(&data, b"sixteen byte id!");
    }

    #[test]
    fn rejects_bad_padding() {
        assert!(strip_pkcs7(&[1, 2, 3, 0]).is_err());
        assert!(strip_pkcs7(&[1, 2, 2, 3]).is_err());
        assert_eq!(strip_pkcs7(&[9, 2, 2]).unwrap(), &[9]);
    }
}
