/* ==========================================================================
Access-token encryption (DPAPI on Windows)
========================================================================== */
//! Encryption of connection access tokens stored in `settings.json`
//! (roadmap 010, task 004; scheme fixed by ADR-0024).
//!
//! On Windows the token is encrypted with DPAPI (`CryptProtectData` /
//! `CryptUnprotectData`) in the current user's context and stored as a
//! base64 string at the same `access_token` position in the file. On other
//! platforms there is no encryption: the token passes through unchanged
//! (plaintext in the file, as at baseline), and no cryptographic code is
//! compiled — every DPAPI call sits behind `cfg(windows)`.
//!
//! A stored value that cannot be decrypted (another Windows profile or
//! machine, corrupted blob) yields a fixed, content-free error: the caller
//! treats the token as unavailable and the user re-enters it through the
//! connection edit form. The error never contains the stored value.

use anyhow::Result;

/// Fixed, content-free message used when a stored token cannot be decrypted.
///
/// Every decryption failure — invalid base64, a blob from another Windows
/// profile or machine, corrupted data, invalid UTF-8 — is reported as this
/// single predictable message so no part of the stored value can leak into
/// logs, errors, or the UI.
pub(crate) const UNDECRYPTABLE_TOKEN: &str = "stored access token cannot be decrypted for this Windows user (the settings file was moved from another profile or machine, or the value is corrupted); re-enter the token in the connection settings";

/// Encrypt a token for storage in `settings.json`.
///
/// Windows: DPAPI-protect in the current user's context, base64-encode.
/// Other platforms: identity (plaintext, as at baseline). An empty token is
/// stored as an empty string on every platform so baseline behavior for
/// "token present but empty" is preserved exactly.
pub(crate) fn encrypt_token(plaintext: &str) -> Result<String> {
    if plaintext.is_empty() {
        return Ok(String::new());
    }
    encrypt_impl(plaintext)
}

/// Decrypt a token read from `settings.json`.
///
/// Windows: base64-decode, DPAPI-unprotect in the current user's context.
/// Other platforms: identity. Any failure is reported as the fixed
/// [`UNDECRYPTABLE_TOKEN`] message; the stored value never appears in the
/// error text.
pub(crate) fn decrypt_token(stored: &str) -> Result<String> {
    if stored.is_empty() {
        return Ok(String::new());
    }
    decrypt_impl(stored)
}

#[cfg(windows)]
fn encrypt_impl(plaintext: &str) -> Result<String> {
    Ok(base64::encode(&dpapi::protect(plaintext.as_bytes())?))
}

#[cfg(not(windows))]
fn encrypt_impl(plaintext: &str) -> Result<String> {
    Ok(plaintext.to_string())
}

#[cfg(windows)]
fn decrypt_impl(stored: &str) -> Result<String> {
    // Every failure kind — bad base64, a blob from another profile or
    // machine, corruption, invalid UTF-8 — collapses into the same fixed,
    // content-free message.
    let blob = base64::decode(stored).map_err(|_| undecryptable())?;
    let bytes = dpapi::unprotect(&blob).map_err(|_| undecryptable())?;
    String::from_utf8(bytes).map_err(|_| undecryptable())
}

#[cfg(not(windows))]
fn decrypt_impl(stored: &str) -> Result<String> {
    Ok(stored.to_string())
}

#[cfg(windows)]
fn undecryptable() -> anyhow::Error {
    anyhow::anyhow!(UNDECRYPTABLE_TOKEN)
}

/* ==========================================================================
DPAPI (Windows only)
========================================================================== */
#[cfg(windows)]
mod dpapi {
    use anyhow::{Context, Result};
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    /// DPAPI-protect `data` in the current user's context. No description
    /// string and no additional entropy: the description would be stored in
    /// the blob unencrypted, and the entropy would make the file
    /// undecryptable for no added value over the user context.
    pub(super) fn protect(data: &[u8]) -> Result<Vec<u8>> {
        unsafe {
            let input = blob_of(data)?;
            let mut output = CRYPT_INTEGER_BLOB::default();
            CryptProtectData(
                &input,
                PCWSTR::null(),
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
            .map_err(|error| anyhow::anyhow!("CryptProtectData failed: {error}"))?;
            Ok(take_blob(output))
        }
    }

    /// DPAPI-unprotect `blob` in the current user's context.
    pub(super) fn unprotect(blob: &[u8]) -> Result<Vec<u8>> {
        unsafe {
            let input = blob_of(blob)?;
            let mut output = CRYPT_INTEGER_BLOB::default();
            CryptUnprotectData(
                &input,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
            .map_err(|error| anyhow::anyhow!("CryptUnprotectData failed: {error}"))?;
            Ok(take_blob(output))
        }
    }

    fn blob_of(data: &[u8]) -> Result<CRYPT_INTEGER_BLOB> {
        Ok(CRYPT_INTEGER_BLOB {
            cbData: u32::try_from(data.len()).context("token data too large for DPAPI")?,
            pbData: data.as_ptr().cast_mut(),
        })
    }

    /// Copy the DPAPI output bytes and free the buffer the API allocated
    /// (`LocalFree`, as required by CryptProtectData/CryptUnprotectData).
    unsafe fn take_blob(output: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(output.pbData.cast())));
        bytes
    }
}

/* ==========================================================================
Base64 (RFC 4648, standard alphabet, padded)
========================================================================== */
/// Hand-rolled base64 codec used to store the DPAPI blob in JSON. Kept
/// local so the dependency set grows by exactly one crate family (the
/// `windows` crate) per task 004 / ADR-0024.
mod base64 {
    use anyhow::{anyhow, Result};

    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    const PAD: u8 = b'=';

    /// Encode `data` with padding (`fo` → `Zm8=`).
    pub(super) fn encode(data: &[u8]) -> String {
        let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
        for chunk in data.chunks(3) {
            let b0 = u32::from(chunk[0]);
            let b1 = chunk.get(1).map_or(0, |&b| u32::from(b));
            let b2 = chunk.get(2).map_or(0, |&b| u32::from(b));
            let n = (b0 << 16) | (b1 << 8) | b2;
            out.push(ALPHABET[(n >> 18) as usize & 63] as char);
            out.push(ALPHABET[(n >> 12) as usize & 63] as char);
            out.push(if chunk.len() > 1 {
                ALPHABET[(n >> 6) as usize & 63] as char
            } else {
                PAD as char
            });
            out.push(if chunk.len() > 2 {
                ALPHABET[n as usize & 63] as char
            } else {
                PAD as char
            });
        }
        out
    }

    /// Decode a padded base64 string. ASCII whitespace is ignored; anything
    /// else that is not valid canonical base64 is an error.
    pub(super) fn decode(text: &str) -> Result<Vec<u8>> {
        let bytes: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
        if !bytes.len().is_multiple_of(4) {
            return Err(anyhow!("invalid base64 length"));
        }
        let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
        for group in bytes.chunks(4) {
            let pad = group.iter().filter(|&&b| b == PAD).count();
            let data_len = 4 - pad;
            let valid_padding = pad <= 2
                && group[..data_len].iter().all(|&b| b != PAD)
                && group[data_len..].iter().all(|&b| b == PAD);
            if !valid_padding {
                return Err(anyhow!("invalid base64 padding"));
            }
            let mut n: u32 = 0;
            for &b in &group[..data_len] {
                let v = value(b).ok_or_else(|| anyhow!("invalid base64 character"))?;
                n = (n << 6) | v;
            }
            n <<= 6 * pad;
            out.push((n >> 16) as u8);
            if pad < 2 {
                out.push((n >> 8) as u8);
            }
            if pad < 1 {
                out.push(n as u8);
            }
        }
        Ok(out)
    }

    fn value(b: u8) -> Option<u32> {
        match b {
            b'A'..=b'Z' => Some(u32::from(b - b'A')),
            b'a'..=b'z' => Some(u32::from(b - b'a') + 26),
            b'0'..=b'9' => Some(u32::from(b - b'0') + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// RFC 4648 section 10 test vectors.
        #[test]
        fn rfc4648_vectors() {
            for (raw, encoded) in [
                ("", ""),
                ("f", "Zg=="),
                ("fo", "Zm8="),
                ("foo", "Zm9v"),
                ("foob", "Zm9vYg=="),
                ("fooba", "Zm9vYmE="),
                ("foobar", "Zm9vYmFy"),
            ] {
                assert_eq!(encode(raw.as_bytes()), encoded, "encoding {raw:?}");
                assert_eq!(
                    decode(encoded).unwrap(),
                    raw.as_bytes(),
                    "decoding {encoded}"
                );
            }
        }

        #[test]
        fn roundtrip_binary() {
            let data: Vec<u8> = (0..=255u8).chain(0..37u8).collect();
            assert_eq!(decode(&encode(&data)).unwrap(), data);
        }

        #[test]
        fn rejects_invalid_input() {
            assert!(decode("ABC").is_err()); // length not a multiple of 4
            assert!(decode("A*C=").is_err()); // invalid character
            assert!(decode("A===B===").is_err()); // three padding characters
            assert!(decode("=AAA").is_err()); // padding not at the end
            assert!(decode("AA=A").is_err()); // padding in the middle
        }

        #[test]
        fn ignores_whitespace() {
            assert_eq!(decode("Zm9v\nYmFy\r\n").unwrap(), b"foobar");
        }
    }
}

/* ==========================================================================
Tests (platform behavior)
========================================================================== */
#[cfg(test)]
mod tests {
    use super::*;

    /// Value used to assert that no part of a stored token ever appears in
    /// an error message.
    const TOKEN_SENTINEL: &str = "SENTINEL-TOKEN-q7x2-plaintext-secret";
    /// Bytes that are valid base64 but not a DPAPI blob.
    const GARBAGE: &[u8] = b"SENTINEL-BLOB-not-a-real-dpapi-blob";

    #[cfg(windows)]
    #[test]
    fn roundtrip_encrypts_and_restores_the_token() {
        let stored = encrypt_token(TOKEN_SENTINEL).unwrap();

        // The stored form does not contain the token, is valid base64 of a
        // DPAPI blob, and decrypts back to the plaintext token.
        assert_ne!(stored, TOKEN_SENTINEL);
        assert!(!stored.contains("SENTINEL"));
        let blob = base64::decode(&stored).unwrap();
        assert_ne!(blob, TOKEN_SENTINEL.as_bytes());
        assert_eq!(decrypt_token(&stored).unwrap(), TOKEN_SENTINEL);
    }

    #[cfg(windows)]
    #[test]
    fn undecryptable_blob_yields_fixed_error_without_content() {
        // A valid base64 string that is not a DPAPI blob (other machine /
        // profile / corruption scenario).
        let stored = base64::encode(GARBAGE);

        let error = decrypt_token(&stored).unwrap_err();
        let text = format!("{error:#}");
        assert_eq!(text, UNDECRYPTABLE_TOKEN);
        assert!(!text.contains("SENTINEL"));
        assert!(!text.contains(&String::from_utf8_lossy(GARBAGE).into_owned()));
        // The message tells the user what to do.
        assert!(text.contains("re-enter"));
    }

    #[cfg(windows)]
    #[test]
    fn empty_token_stays_empty_both_ways() {
        assert_eq!(encrypt_token("").unwrap(), "");
        assert_eq!(decrypt_token("").unwrap(), "");
    }

    #[cfg(not(windows))]
    #[test]
    fn non_windows_passes_the_token_through() {
        assert_eq!(encrypt_token(TOKEN_SENTINEL).unwrap(), TOKEN_SENTINEL);
        assert_eq!(decrypt_token(TOKEN_SENTINEL).unwrap(), TOKEN_SENTINEL);
        assert_eq!(encrypt_token("").unwrap(), "");
        assert_eq!(decrypt_token("").unwrap(), "");
    }
}
