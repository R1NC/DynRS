use crate::c::util::{cbytes_to_rust, rust_to_cbytes, rust_to_cstr};
use crate::core::crypto::{
    aes_decrypt, aes_encrypt, aes_gcm_decrypt, aes_gcm_encrypt, base64_decode, base64_encode,
    hash_md5, hash_sha1, hash_sha256, rand, rsa_decrypt, rsa_encrypt, rsa_gen_key,
};
use std::os::raw::c_char;

/// Copies a Rust buffer into a C-owned allocation, reporting its length through `out_len`.
/// Empty results report a zero length and a null pointer; release non-null results with
/// `ngenrs_free_bytes(ptr, len)`.
unsafe fn bytes_to_c(out_bytes: Vec<u8>, out_len: *mut usize) -> *mut u8 {
    if out_bytes.is_empty() {
        if !out_len.is_null() {
            unsafe { *out_len = 0 };
        }
        return std::ptr::null_mut();
    }
    let (ptr, len) = rust_to_cbytes(out_bytes);
    if !out_len.is_null() {
        unsafe { *out_len = len };
    }
    ptr
}

/// Zeroes the caller's output length and returns null: the empty result that every entry
/// point reports when it rejects its arguments. `out_len` must be null or point to a valid
/// `usize`.
fn empty_result(out_len: *mut usize) -> *mut u8 {
    if !out_len.is_null() {
        unsafe { *out_len = 0 };
    }
    std::ptr::null_mut()
}

unsafe fn common_crypto_process<F>(
    data: *const u8,
    data_len: usize,
    out_len: *mut usize,
    op_fn: F,
) -> *mut u8
where
    F: FnOnce(&[u8]) -> Vec<u8>,
{
    let data_bytes = match cbytes_to_rust(data, data_len) {
        Some(bytes) => bytes,
        None => {
            if !out_len.is_null() {
                unsafe { *out_len = 0 };
            }
            return std::ptr::null_mut();
        }
    };
    unsafe { bytes_to_c(op_fn(data_bytes), out_len) }
}

/// Mirrors `dynxx_crypto_rand`.
/// @return random bytes, release with `ngenrs_free_bytes(ptr, len)`
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_crypto_rand(len: usize) -> *mut u8 {
    if len == 0 {
        return std::ptr::null_mut();
    }
    let (ptr, _) = rust_to_cbytes(rand(len));
    ptr
}

/// Mirrors `dynxx_crypto_aes_encrypt` (AES-ECB + PKCS7, key length MUST BE 16/24/32).
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_crypto_aes_encrypt(
    in_bytes: *const u8,
    in_len: usize,
    key_bytes: *const u8,
    key_len: usize,
    out_len: *mut usize,
) -> *mut u8 {
    let Some(key) = cbytes_to_rust(key_bytes, key_len) else {
        return empty_result(out_len);
    };
    unsafe { common_crypto_process(in_bytes, in_len, out_len, |data| aes_encrypt(data, key)) }
}

/// Mirrors `dynxx_crypto_aes_decrypt` (AES-ECB + PKCS7, key length MUST BE 16/24/32).
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_crypto_aes_decrypt(
    in_bytes: *const u8,
    in_len: usize,
    key_bytes: *const u8,
    key_len: usize,
    out_len: *mut usize,
) -> *mut u8 {
    let Some(key) = cbytes_to_rust(key_bytes, key_len) else {
        return empty_result(out_len);
    };
    unsafe { common_crypto_process(in_bytes, in_len, out_len, |data| aes_decrypt(data, key)) }
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
    out_len: *mut usize,
) -> *mut u8 {
    let Some(key) = cbytes_to_rust(key_bytes, key_len) else {
        return empty_result(out_len);
    };
    let Some(init_vector) = cbytes_to_rust(init_vector_bytes, init_vector_len) else {
        return empty_result(out_len);
    };
    let aad = if aad_bytes.is_null() || aad_len == 0 {
        &[][..]
    } else {
        match cbytes_to_rust(aad_bytes, aad_len) {
            Some(aad) => aad,
            None => return empty_result(out_len),
        }
    };
    unsafe {
        common_crypto_process(in_bytes, in_len, out_len, |data| {
            aes_gcm_encrypt(data, key, init_vector, aad, tag_bits)
        })
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
    out_len: *mut usize,
) -> *mut u8 {
    let Some(key) = cbytes_to_rust(key_bytes, key_len) else {
        return empty_result(out_len);
    };
    let Some(init_vector) = cbytes_to_rust(init_vector_bytes, init_vector_len) else {
        return empty_result(out_len);
    };
    let aad = if aad_bytes.is_null() || aad_len == 0 {
        &[][..]
    } else {
        match cbytes_to_rust(aad_bytes, aad_len) {
            Some(aad) => aad,
            None => return empty_result(out_len),
        }
    };
    unsafe {
        common_crypto_process(in_bytes, in_len, out_len, |data| {
            aes_gcm_decrypt(data, key, init_vector, aad, tag_bits)
        })
    }
}

/// Mirrors `dynxx_crypto_rsa_gen_key`: wraps a base64 DER blob into PEM text.
/// @return PEM text, release with `ngenrs_free_cstr`
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_crypto_rsa_gen_key(base64: *const c_char, is_public: bool) -> *mut c_char {
    let Some(base64) = crate::c::util::cstr_to_rust(base64) else {
        return std::ptr::null_mut();
    };
    let pem = rsa_gen_key(base64, is_public);
    if pem.is_empty() {
        return std::ptr::null_mut();
    }
    rust_to_cstr(pem)
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_crypto_rsa_encrypt(
    in_bytes: *const u8,
    in_len: usize,
    key_bytes: *const u8,
    key_len: usize,
    padding: i32,
    out_len: *mut usize,
) -> *mut u8 {
    let Some(key) = cbytes_to_rust(key_bytes, key_len) else {
        return empty_result(out_len);
    };
    unsafe {
        common_crypto_process(in_bytes, in_len, out_len, |data| {
            rsa_encrypt(data, key, padding)
        })
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_crypto_rsa_decrypt(
    in_bytes: *const u8,
    in_len: usize,
    key_bytes: *const u8,
    key_len: usize,
    padding: i32,
    out_len: *mut usize,
) -> *mut u8 {
    let Some(key) = cbytes_to_rust(key_bytes, key_len) else {
        return empty_result(out_len);
    };
    unsafe {
        common_crypto_process(in_bytes, in_len, out_len, |data| {
            rsa_decrypt(data, key, padding)
        })
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_crypto_hash_md5(
    data: *const u8,
    data_len: usize,
    out_len: *mut usize,
) -> *mut u8 {
    unsafe { common_crypto_process(data, data_len, out_len, hash_md5) }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_crypto_hash_sha1(
    data: *const u8,
    data_len: usize,
    out_len: *mut usize,
) -> *mut u8 {
    unsafe { common_crypto_process(data, data_len, out_len, hash_sha1) }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_crypto_hash_sha256(
    data: *const u8,
    data_len: usize,
    out_len: *mut usize,
) -> *mut u8 {
    unsafe { common_crypto_process(data, data_len, out_len, hash_sha256) }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_crypto_base64_encode(
    in_bytes: *const u8,
    in_len: usize,
    no_new_lines: bool,
    out_len: *mut usize,
) -> *mut u8 {
    unsafe {
        common_crypto_process(in_bytes, in_len, out_len, |data| {
            base64_encode(data, no_new_lines)
        })
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_crypto_base64_decode(
    in_bytes: *const u8,
    in_len: usize,
    no_new_lines: bool,
    out_len: *mut usize,
) -> *mut u8 {
    unsafe {
        common_crypto_process(in_bytes, in_len, out_len, |data| {
            base64_decode(data, no_new_lines)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::c::util::{ngenrs_free_bytes, ngenrs_free_cstr};
    use std::ffi::{CStr, CString};

    /// Copies a result out and frees the C-owned buffer, the way a C caller would.
    fn take_bytes(ptr: *mut u8, len: usize) -> Vec<u8> {
        if ptr.is_null() {
            assert_eq!(len, 0, "an empty result reports a zero length");
            return Vec::new();
        }
        let bytes = unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec();
        ngenrs_free_bytes(ptr, len);
        bytes
    }

    #[test]
    fn empty_inputs_report_a_null_pointer_and_a_zero_length() {
        let mut out_len = usize::MAX;

        // A null data pointer is refused.
        assert!(ngenrs_crypto_hash_md5(std::ptr::null(), 0, &mut out_len).is_null());
        assert_eq!(out_len, 0);

        // A zero length is a valid, empty input whose digest is empty, like DynXX.
        let data = [0u8];
        out_len = usize::MAX;
        assert!(ngenrs_crypto_hash_md5(data.as_ptr(), 0, &mut out_len).is_null());
        assert_eq!(out_len, 0);

        // A null output length pointer is tolerated.
        assert!(ngenrs_crypto_hash_md5(data.as_ptr(), 0, std::ptr::null_mut()).is_null());
    }

    #[test]
    fn rejected_keys_report_an_empty_result() {
        let data = b"data";
        let mut out_len = usize::MAX;

        // 15 bytes is not a valid AES key length.
        let key = [0u8; 15];
        let ptr = ngenrs_crypto_aes_encrypt(
            data.as_ptr(),
            data.len(),
            key.as_ptr(),
            key.len(),
            &mut out_len,
        );
        assert!(ptr.is_null());
        assert_eq!(out_len, 0);

        // A null key pointer is refused just as well.
        let key = [0u8; 16];
        out_len = usize::MAX;
        let ptr = ngenrs_crypto_aes_encrypt(
            data.as_ptr(),
            data.len(),
            std::ptr::null(),
            key.len(),
            &mut out_len,
        );
        assert!(ptr.is_null());
        assert_eq!(out_len, 0);

        // GCM needs a 12 byte IV.
        let iv = [0u8; 11];
        out_len = usize::MAX;
        let ptr = ngenrs_crypto_aes_gcm_encrypt(
            data.as_ptr(),
            data.len(),
            key.as_ptr(),
            key.len(),
            iv.as_ptr(),
            iv.len(),
            std::ptr::null(),
            0,
            128,
            &mut out_len,
        );
        assert!(ptr.is_null());
        assert_eq!(out_len, 0);
    }

    #[test]
    fn hash_outputs_match_the_known_digests() {
        let data = b"abc";
        let mut out_len = 0;

        let md5 = take_bytes(
            ngenrs_crypto_hash_md5(data.as_ptr(), data.len(), &mut out_len),
            out_len,
        );
        assert_eq!(hex::encode(&md5), "900150983cd24fb0d6963f7d28e17f72");

        let sha1 = take_bytes(
            ngenrs_crypto_hash_sha1(data.as_ptr(), data.len(), &mut out_len),
            out_len,
        );
        assert_eq!(
            hex::encode(&sha1),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );

        let sha256 = take_bytes(
            ngenrs_crypto_hash_sha256(data.as_ptr(), data.len(), &mut out_len),
            out_len,
        );
        assert_eq!(
            hex::encode(&sha256),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn base64_round_trips_through_the_c_abi() {
        let data = b"hello C";
        let mut out_len = 0;

        let encoded = take_bytes(
            ngenrs_crypto_base64_encode(data.as_ptr(), data.len(), true, &mut out_len),
            out_len,
        );
        assert_eq!(std::str::from_utf8(&encoded).unwrap(), "aGVsbG8gQw==");

        let decoded = take_bytes(
            ngenrs_crypto_base64_decode(encoded.as_ptr(), encoded.len(), true, &mut out_len),
            out_len,
        );
        assert_eq!(decoded, data);
    }

    #[test]
    fn aes_ecb_and_gcm_round_trip_through_the_c_abi() {
        let key = [7u8; 16];
        let data = b"secret data".to_vec();
        let mut out_len = 0;

        let encrypted = take_bytes(
            ngenrs_crypto_aes_encrypt(
                data.as_ptr(),
                data.len(),
                key.as_ptr(),
                key.len(),
                &mut out_len,
            ),
            out_len,
        );
        assert_ne!(encrypted, data);

        let decrypted = take_bytes(
            ngenrs_crypto_aes_decrypt(
                encrypted.as_ptr(),
                encrypted.len(),
                key.as_ptr(),
                key.len(),
                &mut out_len,
            ),
            out_len,
        );
        assert_eq!(decrypted, data);

        // GCM appends the tag to the payload.
        let iv = [1u8; 12];
        let sealed = take_bytes(
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
                &mut out_len,
            ),
            out_len,
        );
        assert_eq!(sealed.len(), data.len() + 16);

        let opened = take_bytes(
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
                &mut out_len,
            ),
            out_len,
        );
        assert_eq!(opened, data);

        // A modified payload fails authentication and yields nothing.
        let mut tampered = sealed.clone();
        tampered[0] ^= 0xff;
        let mut tampered_len = usize::MAX;
        let ptr = ngenrs_crypto_aes_gcm_decrypt(
            tampered.as_ptr(),
            tampered.len(),
            key.as_ptr(),
            key.len(),
            iv.as_ptr(),
            iv.len(),
            std::ptr::null(),
            0,
            128,
            &mut tampered_len,
        );
        assert!(ptr.is_null());
        assert_eq!(
            tampered_len, 0,
            "a failed authentication reports an empty result"
        );
    }

    #[test]
    fn rand_reports_bytes_and_refuses_a_zero_length_request() {
        assert!(ngenrs_crypto_rand(0).is_null());

        let bytes = take_bytes(ngenrs_crypto_rand(16), 16);
        assert_eq!(bytes.len(), 16);
    }

    #[test]
    fn rsa_gen_key_wraps_base64_into_pem_text() {
        let body = CString::new("MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8A").unwrap();
        let pem = ngenrs_crypto_rsa_gen_key(body.as_ptr(), true);
        assert!(!pem.is_null());
        let text = unsafe { CStr::from_ptr(pem) }.to_str().unwrap().to_string();
        assert!(text.starts_with("-----BEGIN PUBLIC KEY-----\n"));
        assert!(text.trim_end().ends_with("-----END PUBLIC KEY-----"));
        ngenrs_free_cstr(pem);

        // Text that is not base64, and a null pointer, yield nothing.
        let broken = CString::new("not base64!!").unwrap();
        assert!(ngenrs_crypto_rsa_gen_key(broken.as_ptr(), true).is_null());
        assert!(ngenrs_crypto_rsa_gen_key(std::ptr::null(), true).is_null());
    }
}
