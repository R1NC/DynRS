//! The crypto entry points.
//!
//! Data travels through out-parameters (a buffer plus its length) and the *return value* carries the
//! status, so "the arguments were refused", "the operation ran and failed" and "the result is empty"
//! are three different answers rather than three readings of one null pointer. Release a non-null
//! buffer with `ngenrs_free_bytes(ptr)` and a non-null C string with `ngenrs_free_cstr(ptr)`.

use crate::DynrsStatus;
use crate::c::util::{bytes_to_c, cbytes_to_rust, rust_to_cstr};
use crate::core::crypto::{
    CryptoError, aes_decrypt, aes_encrypt, aes_gcm_decrypt, aes_gcm_encrypt, base64_decode,
    base64_encode, hash_md5, hash_sha1, hash_sha256, rand, rsa_decrypt, rsa_encrypt, rsa_gen_key,
};
use std::os::raw::c_char;

/// Maps a portable-layer failure onto the status the C caller sees.
fn status_of(error: CryptoError) -> DynrsStatus {
    match error {
        CryptoError::InvalidArgument => DynrsStatus::InvalidArgument,
        CryptoError::InvalidKey => DynrsStatus::InvalidArgument,
        CryptoError::OperationFailed => DynrsStatus::Failed,
        CryptoError::OutOfMemory => DynrsStatus::Failed,
    }
}

/// Turns a portable-layer result into the ABI's shape: the status is returned and the bytes go to
/// the caller's out-parameters.
///
/// A null `out` is refused rather than served, because a buffer the caller cannot be handed would
/// simply be leaked. Both out-pointers are written on every path, so a caller that ignores the
/// status still cannot read stale values.
fn bytes_to_status(
    result: Result<Vec<u8>, CryptoError>,
    out: *mut *mut u8,
    out_len: *mut usize,
) -> DynrsStatus {
    if out.is_null() {
        if !out_len.is_null() {
            unsafe { *out_len = 0 };
        }
        return DynrsStatus::InvalidArgument;
    }

    match result {
        Ok(bytes) => {
            unsafe { *out = bytes_to_c(bytes, out_len) };
            DynrsStatus::Ok
        }
        Err(error) => {
            unsafe {
                *out = std::ptr::null_mut();
                if !out_len.is_null() {
                    *out_len = 0;
                }
            }
            status_of(error)
        }
    }
}

/// Reads the input and the key, then hands both to `operation` and copies the bytes out.
///
/// The return type is spelled out rather than left to inference: `Result<Vec<u8>, _>` appears only
/// in the closure's body, so the compiler has nothing to deduce it from.
fn with_input_and_key(
    in_bytes: *const u8,
    in_len: usize,
    key_bytes: *const u8,
    key_len: usize,
    out: *mut *mut u8,
    out_len: *mut usize,
    operation: impl FnOnce(&[u8], &[u8]) -> Result<Vec<u8>, CryptoError>,
) -> DynrsStatus {
    let Some(input) = cbytes_to_rust(in_bytes, in_len) else {
        return bytes_to_status(Err(CryptoError::InvalidArgument), out, out_len);
    };
    let Some(key) = cbytes_to_rust(key_bytes, key_len) else {
        return bytes_to_status(Err(CryptoError::InvalidArgument), out, out_len);
    };
    bytes_to_status(operation(input, key), out, out_len)
}

/// The largest random buffer this entry point will produce.
///
/// A single request for randomness is a key, an IV or a nonce, so 64 MiB is far beyond any real
/// use and keeps an absurd length from turning into a multi-exabyte allocation attempt.
const MAX_RAND_BYTES: usize = 64 * 1024 * 1024;

/// Mirrors `dynxx_crypto_rand`. `len` of zero is an empty request rather than a failure; a length
/// beyond `MAX_RAND_BYTES` (64 MiB) is refused instead of being handed to an allocation.
/// @return random bytes through `out` and `out_len`; release with `ngenrs_free_bytes(ptr)`
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_crypto_rand(
    len: usize,
    out: *mut *mut u8,
    out_len: *mut usize,
) -> DynrsStatus {
    ffi_return! {
        if len > MAX_RAND_BYTES {
            return bytes_to_status(Err(CryptoError::InvalidArgument), out, out_len);
        }
        bytes_to_status(rand(len), out, out_len)
    }
}

/// Mirrors `dynxx_crypto_aes_encrypt` (AES-ECB + PKCS7, key length MUST BE 16/24/32).
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_crypto_aes_encrypt(
    in_bytes: *const u8,
    in_len: usize,
    key_bytes: *const u8,
    key_len: usize,
    out: *mut *mut u8,
    out_len: *mut usize,
) -> DynrsStatus {
    ffi_return! {
        with_input_and_key(
            in_bytes,
            in_len,
            key_bytes,
            key_len,
            out,
            out_len,
            aes_encrypt,
        )
    }
}

/// Mirrors `dynxx_crypto_aes_decrypt` (AES-ECB + PKCS7, key length MUST BE 16/24/32).
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_crypto_aes_decrypt(
    in_bytes: *const u8,
    in_len: usize,
    key_bytes: *const u8,
    key_len: usize,
    out: *mut *mut u8,
    out_len: *mut usize,
) -> DynrsStatus {
    ffi_return! {
        with_input_and_key(
            in_bytes,
            in_len,
            key_bytes,
            key_len,
            out,
            out_len,
            aes_decrypt,
        )
    }
}

/// Mirrors `dynxx_crypto_aes_gcm_encrypt`; the tag is appended to the output.
/// `init_vector_len` MUST BE 12, `aad_len` MUST BE <= 16, `tag_bits` MUST BE 96/104/112/120/128.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_crypto_aes_gcm_encrypt(
    in_bytes: *const u8,
    in_len: usize,
    key_bytes: *const u8,
    key_len: usize,
    init_vector_bytes: *const u8,
    init_vector_len: usize,
    aad_bytes: *const u8,
    aad_len: usize,
    tag_bits: usize,
    out: *mut *mut u8,
    out_len: *mut usize,
) -> DynrsStatus {
    ffi_return! {
        let Some(input) = cbytes_to_rust(in_bytes, in_len) else {
            return bytes_to_status(Err(CryptoError::InvalidArgument), out, out_len);
        };
        let Some(key) = cbytes_to_rust(key_bytes, key_len) else {
            return bytes_to_status(Err(CryptoError::InvalidArgument), out, out_len);
        };
        let Some(init_vector) = cbytes_to_rust(init_vector_bytes, init_vector_len) else {
            return bytes_to_status(Err(CryptoError::InvalidArgument), out, out_len);
        };
        // A null AAD pointer with a zero length means "no associated data", not a bad argument.
        let aad = if aad_bytes.is_null() && aad_len == 0 {
            &[][..]
        } else {
            match cbytes_to_rust(aad_bytes, aad_len) {
                Some(aad) => aad,
                None => return bytes_to_status(Err(CryptoError::InvalidArgument), out, out_len),
            }
        };
        bytes_to_status(
            aes_gcm_encrypt(input, key, init_vector, aad, tag_bits),
            out,
            out_len,
        )
    }
}

/// Mirrors `dynxx_crypto_aes_gcm_decrypt`; the tag is expected at the tail of the input.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_crypto_aes_gcm_decrypt(
    in_bytes: *const u8,
    in_len: usize,
    key_bytes: *const u8,
    key_len: usize,
    init_vector_bytes: *const u8,
    init_vector_len: usize,
    aad_bytes: *const u8,
    aad_len: usize,
    tag_bits: usize,
    out: *mut *mut u8,
    out_len: *mut usize,
) -> DynrsStatus {
    ffi_return! {
        let Some(input) = cbytes_to_rust(in_bytes, in_len) else {
            return bytes_to_status(Err(CryptoError::InvalidArgument), out, out_len);
        };
        let Some(key) = cbytes_to_rust(key_bytes, key_len) else {
            return bytes_to_status(Err(CryptoError::InvalidArgument), out, out_len);
        };
        let Some(init_vector) = cbytes_to_rust(init_vector_bytes, init_vector_len) else {
            return bytes_to_status(Err(CryptoError::InvalidArgument), out, out_len);
        };
        let aad = if aad_bytes.is_null() && aad_len == 0 {
            &[][..]
        } else {
            match cbytes_to_rust(aad_bytes, aad_len) {
                Some(aad) => aad,
                None => return bytes_to_status(Err(CryptoError::InvalidArgument), out, out_len),
            }
        };
        bytes_to_status(
            aes_gcm_decrypt(input, key, init_vector, aad, tag_bits),
            out,
            out_len,
        )
    }
}

/// Mirrors `dynxx_crypto_rsa_gen_key`: wraps a base64 DER blob into PEM text.
/// @return PEM text through `out`, release with `ngenrs_free_cstr(ptr)`
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_crypto_rsa_gen_key(
    base64: *const c_char,
    is_public: bool,
    out: *mut *mut c_char,
) -> DynrsStatus {
    ffi_return! {
        let clear = |out: *mut *mut c_char| {
            if !out.is_null() {
                unsafe { *out = std::ptr::null_mut() };
            }
        };
        let Some(base64) = crate::c::util::cstr_to_rust(base64) else {
            clear(out);
            return DynrsStatus::InvalidArgument;
        };
        match rsa_gen_key(base64, is_public) {
            Ok(pem) => match rust_to_cstr(pem) {
                Ok(pem) => {
                    if !out.is_null() {
                        unsafe { *out = pem };
                    }
                    DynrsStatus::Ok
                }
                // Base64 with dashes and newlines cannot hold a NUL, so this is a bug in the wrapper
                // rather than a bad argument — reported as a failure, not as a null that reads like
                // "no key here".
                Err(_) => DynrsStatus::Failed,
            },
            Err(error) => {
                clear(out);
                status_of(error)
            }
        }
    }
}

/// Mirrors `dynxx_crypto_rsa_encrypt`; the key is a PEM public key.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_crypto_rsa_encrypt(
    in_bytes: *const u8,
    in_len: usize,
    key_bytes: *const u8,
    key_len: usize,
    padding: i32,
    out: *mut *mut u8,
    out_len: *mut usize,
) -> DynrsStatus {
    ffi_return! {
        with_input_and_key(
            in_bytes,
            in_len,
            key_bytes,
            key_len,
            out,
            out_len,
            |data, key| rsa_encrypt(data, key, padding),
        )
    }
}

/// Mirrors `dynxx_crypto_rsa_decrypt`; the key is a PEM private key.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_crypto_rsa_decrypt(
    in_bytes: *const u8,
    in_len: usize,
    key_bytes: *const u8,
    key_len: usize,
    padding: i32,
    out: *mut *mut u8,
    out_len: *mut usize,
) -> DynrsStatus {
    ffi_return! {
        with_input_and_key(
            in_bytes,
            in_len,
            key_bytes,
            key_len,
            out,
            out_len,
            |data, key| rsa_decrypt(data, key, padding),
        )
    }
}

/// Reads the data and hashes it. A digest of an empty input is a digest, not a failure.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_crypto_hash_md5(
    data: *const u8,
    data_len: usize,
    out: *mut *mut u8,
    out_len: *mut usize,
) -> DynrsStatus {
    ffi_return! {
        let Some(bytes) = cbytes_to_rust(data, data_len) else {
            return bytes_to_status(Err(CryptoError::InvalidArgument), out, out_len);
        };
        bytes_to_status(Ok(hash_md5(bytes)), out, out_len)
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_crypto_hash_sha1(
    data: *const u8,
    data_len: usize,
    out: *mut *mut u8,
    out_len: *mut usize,
) -> DynrsStatus {
    ffi_return! {
        let Some(bytes) = cbytes_to_rust(data, data_len) else {
            return bytes_to_status(Err(CryptoError::InvalidArgument), out, out_len);
        };
        bytes_to_status(Ok(hash_sha1(bytes)), out, out_len)
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_crypto_hash_sha256(
    data: *const u8,
    data_len: usize,
    out: *mut *mut u8,
    out_len: *mut usize,
) -> DynrsStatus {
    ffi_return! {
        let Some(bytes) = cbytes_to_rust(data, data_len) else {
            return bytes_to_status(Err(CryptoError::InvalidArgument), out, out_len);
        };
        bytes_to_status(Ok(hash_sha256(bytes)), out, out_len)
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_crypto_base64_encode(
    in_bytes: *const u8,
    in_len: usize,
    no_new_lines: bool,
    out: *mut *mut u8,
    out_len: *mut usize,
) -> DynrsStatus {
    ffi_return! {
        let Some(bytes) = cbytes_to_rust(in_bytes, in_len) else {
            return bytes_to_status(Err(CryptoError::InvalidArgument), out, out_len);
        };
        bytes_to_status(Ok(base64_encode(bytes, no_new_lines)), out, out_len)
    }
}

/// Base64 that does not validate is [`DynrsStatus::InvalidArgument`], not an empty result.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_crypto_base64_decode(
    in_bytes: *const u8,
    in_len: usize,
    no_new_lines: bool,
    out: *mut *mut u8,
    out_len: *mut usize,
) -> DynrsStatus {
    ffi_return! {
        let Some(bytes) = cbytes_to_rust(in_bytes, in_len) else {
            return bytes_to_status(Err(CryptoError::InvalidArgument), out, out_len);
        };
        bytes_to_status(base64_decode(bytes, no_new_lines), out, out_len)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::c::util::{ngenrs_free_bytes, ngenrs_free_cstr};
    use std::ffi::{CStr, CString};

    /// Runs one of the byte-producing entry points the way a C caller does: the status decides
    /// whether to look at the buffer, and the buffer is released when it is there.
    fn call_bytes(
        entry: impl FnOnce(*mut *mut u8, *mut usize) -> DynrsStatus,
    ) -> Result<Vec<u8>, DynrsStatus> {
        let mut ptr = std::ptr::null_mut();
        let mut len = usize::MAX;
        let status = entry(&mut ptr, &mut len);
        if status != DynrsStatus::Ok {
            assert!(ptr.is_null(), "a failed call reports no buffer");
            assert_eq!(len, 0, "a failed call reports a zero length");
            return Err(status);
        }
        let bytes = unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec();
        ngenrs_free_bytes(ptr);
        Ok(bytes)
    }

    /// A null output pointer is refused rather than producing a buffer nobody can be handed: there
    /// is nowhere to write it, so the call cannot report a result.
    #[test]
    fn a_null_out_pointer_is_refused() {
        let data = [0u8];
        let status =
            ngenrs_crypto_hash_md5(data.as_ptr(), 0, std::ptr::null_mut(), std::ptr::null_mut());
        assert_eq!(status, DynrsStatus::InvalidArgument);
    }

    /// A length beyond the ceiling is refused instead of being handed to an allocation. The core
    /// `rand` asks for its buffer fallibly as well, because `vec![0u8; len]` panics with "capacity
    /// overflow" above `isize::MAX` — and that panic would run from inside a C ABI entry point.
    #[test]
    fn an_impossible_random_length_is_refused_rather_than_panicking() {
        assert_eq!(
            call_bytes(|out, len| ngenrs_crypto_rand(usize::MAX, out, len)),
            Err(DynrsStatus::InvalidArgument)
        );

        // A length that is satisfiable is served, and reports what it allocated rather than what
        // was asked for.
        let bytes = call_bytes(|out, len| ngenrs_crypto_rand(8, out, len)).expect("8 bytes");
        assert_eq!(bytes.len(), 8);
    }

    /// Zero random bytes is an empty request, not a failure.
    #[test]
    fn zero_random_bytes_is_an_empty_result() {
        let bytes =
            call_bytes(|out, len| ngenrs_crypto_rand(0, out, len)).expect("an empty request");
        assert!(bytes.is_empty());
    }

    /// A digest of an empty input is a digest: the call ran and produced its full-length output.
    #[test]
    fn an_empty_input_produces_a_digest_not_an_error() {
        let data = [0u8];
        let bytes = call_bytes(|out, len| ngenrs_crypto_hash_md5(data.as_ptr(), 0, out, len))
            .expect("an empty input hashes");
        assert_eq!(bytes.len(), 16, "md5 of nothing is still 16 bytes");

        // A null data pointer, on the other hand, is a rejected argument.
        assert_eq!(
            call_bytes(|out, len| ngenrs_crypto_hash_md5(std::ptr::null(), 0, out, len)),
            Err(DynrsStatus::InvalidArgument)
        );
    }

    /// A key that cannot work is a rejected argument, and the status says so instead of leaving the
    /// caller to read it off a null pointer.
    #[test]
    fn a_rejected_key_is_reported_by_the_status() {
        let data = b"data";
        let short_key = [0u8; 15];
        assert_eq!(
            call_bytes(|out, len| ngenrs_crypto_aes_encrypt(
                data.as_ptr(),
                data.len(),
                short_key.as_ptr(),
                short_key.len(),
                out,
                len,
            )),
            Err(DynrsStatus::InvalidArgument)
        );

        let key = [0u8; 16];
        assert_eq!(
            call_bytes(|out, len| ngenrs_crypto_aes_encrypt(
                data.as_ptr(),
                data.len(),
                std::ptr::null(),
                key.len(),
                out,
                len,
            )),
            Err(DynrsStatus::InvalidArgument)
        );

        // GCM needs a 12 byte IV.
        let short_iv = [0u8; 11];
        assert_eq!(
            call_bytes(|out, len| ngenrs_crypto_aes_gcm_encrypt(
                data.as_ptr(),
                data.len(),
                key.as_ptr(),
                key.len(),
                short_iv.as_ptr(),
                short_iv.len(),
                std::ptr::null(),
                0,
                128,
                out,
                len,
            )),
            Err(DynrsStatus::InvalidArgument)
        );
    }

    #[test]
    fn hash_outputs_match_the_known_digests() {
        let data = b"abc";
        let md5 =
            call_bytes(|out, len| ngenrs_crypto_hash_md5(data.as_ptr(), data.len(), out, len))
                .expect("md5");
        assert_eq!(hex::encode(&md5), "900150983cd24fb0d6963f7d28e17f72");

        let sha1 =
            call_bytes(|out, len| ngenrs_crypto_hash_sha1(data.as_ptr(), data.len(), out, len))
                .expect("sha1");
        assert_eq!(
            hex::encode(&sha1),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );

        let sha256 =
            call_bytes(|out, len| ngenrs_crypto_hash_sha256(data.as_ptr(), data.len(), out, len))
                .expect("sha256");
        assert_eq!(
            hex::encode(&sha256),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn base64_round_trips_through_the_c_abi() {
        let data = b"hello C";
        let encoded = call_bytes(|out, len| {
            ngenrs_crypto_base64_encode(data.as_ptr(), data.len(), true, out, len)
        })
        .expect("encoding works");
        assert_eq!(std::str::from_utf8(&encoded).unwrap(), "aGVsbG8gQw==");

        let decoded = call_bytes(|out, len| {
            ngenrs_crypto_base64_decode(encoded.as_ptr(), encoded.len(), true, out, len)
        })
        .expect("decoding works");
        assert_eq!(decoded, data);

        // Text that is not base64 is an argument error rather than an empty result.
        assert_eq!(
            call_bytes(|out, len| ngenrs_crypto_base64_decode(
                b"not base64!!".as_ptr(),
                12,
                true,
                out,
                len,
            )),
            Err(DynrsStatus::InvalidArgument)
        );
    }

    #[test]
    fn aes_ecb_and_gcm_round_trip_through_the_c_abi() {
        let key = [7u8; 16];
        let data = b"secret data".to_vec();

        let encrypted = call_bytes(|out, len| {
            ngenrs_crypto_aes_encrypt(data.as_ptr(), data.len(), key.as_ptr(), key.len(), out, len)
        })
        .expect("AES encrypts");
        assert_ne!(encrypted, data);

        let decrypted = call_bytes(|out, len| {
            ngenrs_crypto_aes_decrypt(
                encrypted.as_ptr(),
                encrypted.len(),
                key.as_ptr(),
                key.len(),
                out,
                len,
            )
        })
        .expect("AES decrypts");
        assert_eq!(decrypted, data);

        // GCM appends the tag to the payload.
        let iv = [1u8; 12];
        let sealed = call_bytes(|out, len| {
            ngenrs_crypto_aes_gcm_encrypt(
                data.as_ptr(),
                data.len(),
                key.as_ptr(),
                key.len(),
                iv.as_ptr(),
                iv.len(),
                std::ptr::null(),
                0,
                128,
                out,
                len,
            )
        })
        .expect("GCM seals");
        assert_eq!(sealed.len(), data.len() + 16);

        let opened = call_bytes(|out, len| {
            ngenrs_crypto_aes_gcm_decrypt(
                sealed.as_ptr(),
                sealed.len(),
                key.as_ptr(),
                key.len(),
                iv.as_ptr(),
                iv.len(),
                std::ptr::null(),
                0,
                128,
                out,
                len,
            )
        })
        .expect("GCM opens");
        assert_eq!(opened, data);

        // A modified payload fails authentication. The arguments were fine, so this is
        // `DynrsStatus::Failed` — the operation ran and refused — rather than the
        // `InvalidArgument` a bad key length produces, and rather than an empty result.
        let mut tampered = sealed.clone();
        tampered[0] ^= 0xff;
        assert_eq!(
            call_bytes(|out, len| ngenrs_crypto_aes_gcm_decrypt(
                tampered.as_ptr(),
                tampered.len(),
                key.as_ptr(),
                key.len(),
                iv.as_ptr(),
                iv.len(),
                std::ptr::null(),
                0,
                128,
                out,
                len,
            )),
            Err(DynrsStatus::Failed),
            "a tag that does not verify is a failure of the operation"
        );
    }

    /// No status is `Ok` while leaving the buffer pointer unset, and none is non-`Ok` while leaving
    /// a buffer for the caller to leak.
    #[test]
    fn the_status_and_the_out_parameters_always_agree() {
        let data = b"data";
        let key = [9u8; 16];

        let mut ptr = std::ptr::null_mut();
        let mut len = 0;
        assert_eq!(
            ngenrs_crypto_aes_encrypt(
                data.as_ptr(),
                data.len(),
                key.as_ptr(),
                key.len(),
                &mut ptr,
                &mut len
            ),
            DynrsStatus::Ok
        );
        assert!(!ptr.is_null());
        assert_eq!(len, 16, "PKCS7 pads a 4 byte input up to one block");
        ngenrs_free_bytes(ptr);

        let mut ptr = std::ptr::null_mut();
        let mut len = 0;
        // An AAD pointer that is null *with a length* is a broken description.
        assert_eq!(
            ngenrs_crypto_aes_gcm_encrypt(
                data.as_ptr(),
                data.len(),
                key.as_ptr(),
                key.len(),
                [0u8; 12].as_ptr(),
                12,
                std::ptr::null(),
                4,
                128,
                &mut ptr,
                &mut len,
            ),
            DynrsStatus::InvalidArgument
        );
        assert!(ptr.is_null());
        assert_eq!(len, 0);
    }

    #[test]
    fn rsa_gen_key_wraps_base64_into_pem_text() {
        let body = CString::new("MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8A").unwrap();
        let mut pem = std::ptr::null_mut();
        assert_eq!(
            ngenrs_crypto_rsa_gen_key(body.as_ptr(), true, &mut pem),
            DynrsStatus::Ok
        );
        assert!(!pem.is_null());
        let text = unsafe { CStr::from_ptr(pem) }.to_str().unwrap().to_string();
        assert!(text.starts_with("-----BEGIN PUBLIC KEY-----\n"));
        assert!(text.trim_end().ends_with("-----END PUBLIC KEY-----"));
        ngenrs_free_cstr(pem);

        // Text that is not base64 is a rejected argument, not an empty string.
        let broken = CString::new("not base64!!").unwrap();
        let mut pem = std::ptr::null_mut();
        assert_eq!(
            ngenrs_crypto_rsa_gen_key(broken.as_ptr(), true, &mut pem),
            DynrsStatus::InvalidArgument
        );
        assert!(pem.is_null(), "a failed call leaves no string behind");

        // So is a null pointer.
        let mut pem = std::ptr::null_mut();
        assert_eq!(
            ngenrs_crypto_rsa_gen_key(std::ptr::null(), true, &mut pem),
            DynrsStatus::InvalidArgument
        );
        assert!(pem.is_null());
    }

    /// The RSA entry points, which take the key as bytes and hand the ciphertext back.
    #[test]
    fn rsa_round_trips_through_the_c_abi() {
        use rsa::pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding};
        use rsa::rand_core::OsRng;
        use rsa::{RsaPrivateKey, RsaPublicKey};

        let private = RsaPrivateKey::new(&mut OsRng, 1024).expect("a key pair is generated");
        let public = RsaPublicKey::from(&private);
        let private_pem = private
            .to_pkcs8_pem(LineEnding::LF)
            .expect("the private key encodes to PEM");
        let public_pem = public
            .to_public_key_pem(LineEnding::LF)
            .expect("the public key encodes to PEM");

        let message = b"c abi payload";

        // 1 = PKCS#1 v1.5, the padding both sides support.
        let encrypted = call_bytes(|out, len| {
            ngenrs_crypto_rsa_encrypt(
                message.as_ptr(),
                message.len(),
                public_pem.as_bytes().as_ptr(),
                public_pem.len(),
                1,
                out,
                len,
            )
        })
        .expect("the message encrypts");
        assert!(!encrypted.is_empty());
        assert_ne!(encrypted.as_slice(), message.as_slice());

        let decrypted = call_bytes(|out, len| {
            ngenrs_crypto_rsa_decrypt(
                encrypted.as_ptr(),
                encrypted.len(),
                private_pem.as_bytes().as_ptr(),
                private_pem.len(),
                1,
                out,
                len,
            )
        })
        .expect("the message decrypts");
        assert_eq!(decrypted, message.to_vec());

        // A key that is not a PEM at all is a rejected argument.
        assert_eq!(
            call_bytes(|out, len| ngenrs_crypto_rsa_encrypt(
                message.as_ptr(),
                message.len(),
                b"not a pem".as_ptr(),
                9,
                1,
                out,
                len,
            )),
            Err(DynrsStatus::InvalidArgument)
        );
        assert_eq!(
            call_bytes(|out, len| ngenrs_crypto_rsa_decrypt(
                message.as_ptr(),
                message.len(),
                b"not a pem".as_ptr(),
                9,
                1,
                out,
                len,
            )),
            Err(DynrsStatus::InvalidArgument)
        );
    }
}
