//! Checking downloads: an Ed25519 signature over the release's `SHA256SUMS.txt`, then each
//! file's SHA-256 against that list.
//!
//! The release workflow signs the list with OpenSSL (`openssl pkeyutl -sign -rawin`) and uploads
//! the base64 signature as `SHA256SUMS.txt.sig`. The app carries the public key, so a release
//! whose files were swapped, even by someone who can publish releases, fails the check unless
//! they also hold the private key.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};

use crate::Error;

/// The checksum list's file name in every release.
pub const SUMS: &str = "SHA256SUMS.txt";
/// The signature's file name in every release.
pub const SIGNATURE: &str = "SHA256SUMS.txt.sig";
/// The largest checksum list or signature the updater reads.
pub const MAX_LIST_BYTES: u64 = 256 * 1024;

/// A 32-byte Ed25519 public key from its base64 text. Also accepts the 44-byte
/// SubjectPublicKeyInfo that `openssl pkey -pubout -outform DER` writes.
pub fn decode_public_key(b64: &str) -> Result<[u8; 32], Error> {
    let bytes = STANDARD.decode(b64.trim()).map_err(|e| Error::Verify(format!("the update key isn't base64: {e}")))?;
    let raw = match bytes.len() {
        32 => bytes.as_slice(),
        44 if bytes.starts_with(&[0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00]) => bytes.get(12..).unwrap_or_default(),
        n => return Err(Error::Verify(format!("the update key has {n} bytes, not 32"))),
    };
    let raw: [u8; 32] = raw.try_into().map_err(|_| Error::Verify("the update key has the wrong length".into()))?;
    let key = VerifyingKey::from_bytes(&raw).map_err(|_| Error::Verify("the update key isn't a valid Ed25519 key".into()))?;
    if key.is_weak() {
        return Err(Error::Verify("the update key is a weak Ed25519 key".into()));
    }
    Ok(raw)
}

/// A verified checksum list: file name to SHA-256.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sums {
    entries: Vec<(String, [u8; 32])>,
}

impl Sums {
    /// Checks `signature` (base64 text) over `list` with `key`, then reads the list.
    pub fn verified(list: &[u8], signature: &str, key: &[u8; 32]) -> Result<Sums, Error> {
        let key = VerifyingKey::from_bytes(key).map_err(|_| Error::Verify("the update key isn't a valid Ed25519 key".into()))?;
        let sig = STANDARD.decode(signature.trim()).map_err(|_| Error::Verify("the signature isn't base64".into()))?;
        let sig: [u8; 64] = sig.as_slice().try_into().map_err(|_| Error::Verify("the signature has the wrong length".into()))?;
        key.verify_strict(list, &Signature::from_bytes(&sig)).map_err(|_| Error::Verify("the checksum list's signature doesn't match".into()))?;
        let text = std::str::from_utf8(list).map_err(|_| Error::Verify("the checksum list isn't text".into()))?;
        Ok(Sums::parse(text))
    }

    /// Reads `sha256sum` output (`<hex>  <name>` or `<hex> *<name>` per line), skipping lines
    /// that aren't entries. Only for lists already verified or in tests.
    fn parse(text: &str) -> Sums {
        let entries = text
            .lines()
            .filter_map(|line| {
                let (hex, name) = line.split_once(' ')?;
                let name = name.trim_start_matches([' ', '*']).trim_end();
                Some((name.to_owned(), from_hex(hex)?))
            })
            .collect();
        Sums { entries }
    }

    /// The listed SHA-256 of `name`.
    pub fn get(&self, name: &str) -> Option<&[u8; 32]> {
        self.entries.iter().find(|(n, _)| n == name).map(|(_, h)| h)
    }
}

fn from_hex(s: &str) -> Option<[u8; 32]> {
    let b = s.as_bytes();
    if b.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    let (pairs, _) = b.as_chunks::<2>();
    for (o, &[hi, lo]) in out.iter_mut().zip(pairs) {
        let hi = (hi as char).to_digit(16)?;
        let lo = (lo as char).to_digit(16)?;
        *o = u8::try_from(hi * 16 + lo).ok()?;
    }
    Some(out)
}

/// A running SHA-256, for hashing a download as it arrives.
#[derive(Clone, Default)]
pub struct Hasher(Sha256);

impl Hasher {
    /// Adds bytes.
    pub fn update(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }

    /// The hash of everything added.
    pub fn finish(self) -> [u8; 32] {
        self.0.finalize().into()
    }
}

/// Fails unless `actual` is the hash `sums` lists for `name`.
pub fn check(sums: &Sums, name: &str, actual: &[u8; 32]) -> Result<(), Error> {
    let expected = sums.get(name).ok_or_else(|| Error::Verify(format!("{name} isn't in the signed checksum list")))?;
    if expected == actual { Ok(()) } else { Err(Error::Verify(format!("{name} doesn't match its checksum"))) }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A throwaway key made with OpenSSL for these tests only (never used for a release):
    //   openssl genpkey -algorithm ed25519 -out test.pem
    //   openssl pkey -in test.pem -pubout -outform DER | base64
    //   openssl pkeyutl -sign -inkey test.pem -rawin -in SHA256SUMS.txt | base64 -w0
    const OPENSSL_PUBLIC_DER: &str = include_str!("../tests/data/test-key.pub.der.b64");
    const OPENSSL_SUMS: &[u8] = include_bytes!("../tests/data/SHA256SUMS.txt");
    const OPENSSL_SIGNATURE: &str = include_str!("../tests/data/SHA256SUMS.txt.sig");

    #[test]
    fn a_list_signed_by_openssl_verifies() {
        let key = decode_public_key(OPENSSL_PUBLIC_DER).unwrap();
        let sums = Sums::verified(OPENSSL_SUMS, OPENSSL_SIGNATURE, &key).unwrap();
        let mut h = Hasher::default();
        h.update(b"hello\n");
        check(&sums, "pdfcraft-1.2.0-linux-x86_64.AppImage", &h.finish()).unwrap();
        let mut other = Hasher::default();
        other.update(b"tampered\n");
        assert!(matches!(check(&sums, "pdfcraft-1.2.0-linux-x86_64.AppImage", &other.finish()), Err(Error::Verify(_))));
        assert!(matches!(check(&sums, "missing.msi", &[0; 32]), Err(Error::Verify(_))));
        // The raw 32-byte form of the same key works too.
        let raw = STANDARD.encode(key);
        assert_eq!(decode_public_key(&raw).unwrap(), key);
    }

    #[test]
    fn a_changed_list_or_wrong_key_fails() {
        let key = decode_public_key(OPENSSL_PUBLIC_DER).unwrap();
        let mut changed = OPENSSL_SUMS.to_vec();
        if let Some(b) = changed.first_mut() {
            *b ^= 1;
        }
        assert!(matches!(Sums::verified(&changed, OPENSSL_SIGNATURE, &key), Err(Error::Verify(_))));
        let mut wrong = key;
        wrong[0] ^= 0x55;
        assert!(Sums::verified(OPENSSL_SUMS, OPENSSL_SIGNATURE, &wrong).is_err());
        for bad_sig in ["", "!!!", "AAAA", &STANDARD.encode([0u8; 64])] {
            assert!(Sums::verified(OPENSSL_SUMS, bad_sig, &key).is_err(), "{bad_sig}");
        }
        for bad_key in ["", "xyz", &STANDARD.encode([1u8; 31]), &STANDARD.encode([1u8; 44])] {
            assert!(decode_public_key(bad_key).is_err(), "{bad_key}");
        }
    }

    #[test]
    fn weak_public_keys_cannot_authorize_an_unsigned_manifest() {
        let mut identity = [0u8; 32];
        identity[0] = 1;
        let mut forged_signature = [0u8; 64];
        forged_signature[0] = 1;
        let signature = STANDARD.encode(forged_signature);
        assert!(Sums::verified(OPENSSL_SUMS, &signature, &identity).is_err());
        assert!(decode_public_key(&STANDARD.encode(identity)).is_err());
        assert!(decode_public_key(&STANDARD.encode([0u8; 32])).is_err());
    }

    #[test]
    fn checksum_lines_are_read_leniently() {
        let h = "ab".repeat(32);
        let s = Sums::parse(&format!("{h}  a.msi\n{h} *b.deb\nnot a line\n{}  short\n", "ab".repeat(3)));
        assert!(s.get("a.msi").is_some());
        assert!(s.get("b.deb").is_some());
        assert!(s.get("short").is_none());
        assert_eq!(from_hex(&"zz".repeat(32)), None);
    }
}
