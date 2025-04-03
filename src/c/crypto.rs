use crate::c::util::{cbytes_to_rust, rust_to_cbytes, ngenrs_free_ptr, box_into_raw_new};
use crate::core::crypto::{Aes256EcbPkcs5, rsa_enc, rsa_dec, hash_md5, hash_sha1, hash_sha256, base64_encode, base64_decode};
use std::os::raw::c_void;

unsafe fn common_crypto_process<F>(
    data: *const u8,
    data_len: usize,
    out_len: *mut usize,
    op_fn: F
) -> *mut u8
where
    F: FnOnce(&[u8]) -> Vec<u8>
{
    let data_bytes = match cbytes_to_rust(data, data_len) {
        Some(bytes) => bytes,
        None => return std::ptr::null_mut(),
    };
    let out_bytes = op_fn(data_bytes);
    let (ptr, len) = rust_to_cbytes(out_bytes);
    unsafe { *out_len = len };
    ptr
}

#[unsafe(no_mangle)]
pub extern "C"
fn ngenrs_crypto_aes256_ecb_pkcs5_init(key: *const u8, key_len: usize) -> *mut c_void {
    if key.is_null() || key_len != 32 {
        return std::ptr::null_mut();
    }
    let key_bytes = match { cbytes_to_rust(key, key_len) } {
        Some(bytes) => bytes,
        None => return std::ptr::null_mut(),
    };
    match Aes256EcbPkcs5::new(key_bytes) {
        Ok(cipher) => box_into_raw_new(cipher) as *mut c_void,
        Err(_) => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "C" 
fn ngenrs_crypto_aes256_ecb_pkcs5_encrypt(
    cipher: *mut c_void,
    data: *const u8,
    data_len: usize,
    out_len: *mut usize,
) -> *mut u8 {
    if cipher.is_null() {
        return std::ptr::null_mut();
    }
    let cipher_ref = unsafe { &*(cipher as *mut Aes256EcbPkcs5) };
    unsafe { 
        common_crypto_process(data, data_len, out_len, |bytes| {
            cipher_ref.enc(bytes)
        })
    }
}

#[unsafe(no_mangle)]
pub extern "C" 
fn ngenrs_crypto_aes256_ecb_pkcs5_decrypt(
    cipher: *mut Aes256EcbPkcs5,
    data: *const u8,
    data_len: usize,
    out_len: *mut usize,
) -> *mut u8 {
    if cipher.is_null() {
        return std::ptr::null_mut();
    }
    let cipher_ref = unsafe { &*cipher };
    unsafe {
        common_crypto_process(data, data_len, out_len, |bytes| {
            cipher_ref.dec(bytes).unwrap_or_default()
        })
    }
}

#[unsafe(no_mangle)]
pub extern "C" 
fn ngenrs_crypto_aes256_ecb_pkcs5_release(cipher: *mut c_void) {
    ngenrs_free_ptr(cipher);
}

#[unsafe(no_mangle)]
pub extern "C" 
fn ngenrs_crypto_rsa_encrypt(
    input: *const u8,
    input_len: usize,
    pub_key: *const u8,
    pub_key_len: usize,
    padding: i32,
    out_len: *mut usize,
) -> *mut u8 {
    if input.is_null() || pub_key.is_null() {
        return std::ptr::null_mut();
    }

    let input_bytes = match cbytes_to_rust(input, input_len) {
        Some(bytes) => bytes.to_vec(),
        None => return std::ptr::null_mut(),
    };

    let pub_key_bytes = match cbytes_to_rust(pub_key, pub_key_len) {
        Some(bytes) => bytes.to_vec(),
        None => return std::ptr::null_mut(),
    };

    let result = rsa_enc(input_bytes, pub_key_bytes, padding);
    let (ptr, len) = rust_to_cbytes(result);
    unsafe { *out_len = len };
    ptr
}

#[unsafe(no_mangle)]
pub extern "C" 
fn ngenrs_crypto_rsa_decrypt(
    input: *const u8,
    input_len: usize,
    priv_key: *const u8,
    priv_key_len: usize,
    padding: i32,
    out_len: *mut usize,
) -> *mut u8 {
    if input.is_null() || priv_key.is_null() {
        return std::ptr::null_mut();
    }

    let input_bytes = match cbytes_to_rust(input, input_len) {
        Some(bytes) => bytes.to_vec(),
        None => return std::ptr::null_mut(),
    };

    let priv_key_bytes = match cbytes_to_rust(priv_key, priv_key_len) {
        Some(bytes) => bytes.to_vec(),
        None => return std::ptr::null_mut(),
    };

    let result = rsa_dec(input_bytes, priv_key_bytes, padding);
    let (ptr, len) = rust_to_cbytes(result);
    unsafe { *out_len = len };
    ptr
}

#[unsafe(no_mangle)]
pub extern "C" 
fn ngenrs_crypto_hash_md5(data: *const u8, data_len: usize, out_len: *mut usize) -> *mut u8 {
    unsafe { common_crypto_process(data, data_len, out_len, hash_md5) }
}

#[unsafe(no_mangle)]
pub extern "C"
fn ngenrs_crypto_hash_sha1(data: *const u8, data_len: usize, out_len: *mut usize) -> *mut u8 {
    unsafe { common_crypto_process(data, data_len, out_len, hash_sha1) }
}

#[unsafe(no_mangle)]
pub extern "C"
fn ngenrs_crypto_hash_sha256(data: *const u8, data_len: usize, out_len: *mut usize) -> *mut u8 {
    unsafe { common_crypto_process(data, data_len, out_len, hash_sha256) }
}

#[unsafe(no_mangle)]
pub extern "C"
fn ngenrs_crypto_base64_encode(data: *const u8, data_len: usize, out_len: *mut usize) -> *mut u8 {
    unsafe { common_crypto_process(data, data_len, out_len, base64_encode) }
}

#[unsafe(no_mangle)]
pub extern "C"
fn ngenrs_crypto_base64_decode(data: *const u8, data_len: usize, out_len: *mut usize) -> *mut u8 {
    unsafe { common_crypto_process(data, data_len, out_len, base64_decode) }
}

