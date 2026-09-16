// `u*::is_multiple_of` is stable since Rust 1.87 while the crate only requires edition 2024
// (Rust 1.85), so the modulo checks stay written as they are.
#![allow(clippy::manual_is_multiple_of)]

use aes::{Aes128 as Aes128Cipher, Aes192 as Aes192Cipher, Aes256 as Aes256Cipher};
use aes_gcm::AesGcm;
use aes_gcm::aead::{AeadInOut, KeyInit};
use aes_gcm::aes::cipher::consts::{U12, U13, U14, U15, U16};
use aes_gcm::aes::{Aes128 as Aes128Gcm, Aes192 as Aes192Gcm, Aes256 as Aes256Gcm};
use base64::{Engine as _, engine::general_purpose};
use block_modes::block_padding::Pkcs7;
use block_modes::{BlockMode, Ecb};
use md5::{Digest, Md5};
use rand::RngCore;
use rsa::pkcs1::DecodeRsaPrivateKey;
use rsa::pkcs8::{DecodePrivateKey, DecodePublicKey};
use rsa::rand_core::OsRng;
use rsa::{Oaep, Pkcs1v15Encrypt, RsaPrivateKey, RsaPublicKey};
use sha1::Sha1;
use sha2::Sha256;
use std::error::Error;

/// `aes-gcm` encodes the tag length as a type parameter, so the lengths DynXX accepts
/// (96/104/112/120/128 bits) are dispatched through macros: the bounds required by the
/// crate's `AeadInOut` impl do not survive an extra layer of type parameters.
macro_rules! gcm_tag_impl {
    ($aes:ty, $tag_ty:ty, $key:expr, $nonce:expr, $aad:expr, $buffer:expr) => {{
        match AesGcm::<$aes, U12, $tag_ty>::new_from_slice($key) {
            Ok(cipher) => {
                match cipher.encrypt_inout_detached($nonce, $aad, $buffer.as_mut_slice().into()) {
                    Ok(tag) => Some(tag.as_slice().to_vec()),
                    Err(_) => None,
                }
            }
            Err(_) => None,
        }
    }};
}

macro_rules! gcm_tag_for_key {
    ($aes:ty, $key:expr, $nonce:expr, $aad:expr, $tag_bits:expr, $buffer:expr) => {
        match $tag_bits {
            96 => gcm_tag_impl!($aes, U12, $key, $nonce, $aad, $buffer),
            104 => gcm_tag_impl!($aes, U13, $key, $nonce, $aad, $buffer),
            112 => gcm_tag_impl!($aes, U14, $key, $nonce, $aad, $buffer),
            120 => gcm_tag_impl!($aes, U15, $key, $nonce, $aad, $buffer),
            128 => gcm_tag_impl!($aes, U16, $key, $nonce, $aad, $buffer),
            _ => None,
        }
    };
}

macro_rules! gcm_verify_impl {
    ($aes:ty, $tag_ty:ty, $key:expr, $nonce:expr, $aad:expr, $tag:expr, $buffer:expr) => {{
        match AesGcm::<$aes, U12, $tag_ty>::new_from_slice($key) {
            Ok(cipher) => match aes_gcm::Tag::<$tag_ty>::try_from($tag) {
                Ok(tag) => cipher
                    .decrypt_inout_detached($nonce, $aad, $buffer.as_mut_slice().into(), &tag)
                    .is_ok(),
                Err(_) => false,
            },
            Err(_) => false,
        }
    }};
}

macro_rules! gcm_verify_for_key {
    ($aes:ty, $key:expr, $nonce:expr, $aad:expr, $tag_bits:expr, $tag:expr, $buffer:expr) => {
        match $tag_bits {
            96 => gcm_verify_impl!($aes, U12, $key, $nonce, $aad, $tag, $buffer),
            104 => gcm_verify_impl!($aes, U13, $key, $nonce, $aad, $tag, $buffer),
            112 => gcm_verify_impl!($aes, U14, $key, $nonce, $aad, $tag, $buffer),
            120 => gcm_verify_impl!($aes, U15, $key, $nonce, $aad, $tag, $buffer),
            128 => gcm_verify_impl!($aes, U16, $key, $nonce, $aad, $tag, $buffer),
            _ => false,
        }
    };
}

pub fn str2bytes(s: String) -> Vec<u8> {
    s.into_bytes()
}

pub fn bytes2str(v: Vec<u8>) -> Result<String, Box<dyn Error>> {
    String::from_utf8(v).map_err(|e| e.into())
}

pub fn hex2bytes(hex: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    hex::decode(hex).map_err(|e| e.into())
}

pub fn bytes2hex(bytes: &[u8]) -> String {
    hex::encode(bytes)
}

type Aes128EcbCipher = Ecb<Aes128Cipher, Pkcs7>;
type Aes192EcbCipher = Ecb<Aes192Cipher, Pkcs7>;
type Aes256EcbCipher = Ecb<Aes256Cipher, Pkcs7>;

/// Mirrors `checkAesParams` in `Crypto-OpenSSL.cxx`:
/// input must be non-empty, key length a multiple of 8 within `16..=32`.
fn check_aes_params(input: &[u8], key: &[u8]) -> bool {
    !input.is_empty() && key.len() >= 16 && key.len() <= 32 && key.len() % 8 == 0
}

/// AES-ECB + PKCS7, mirroring DynXX `Core::Crypto::AES::encrypt`.
pub fn aes_encrypt(input: &[u8], key: &[u8]) -> Vec<u8> {
    if !check_aes_params(input, key) {
        return Vec::new();
    }
    match key.len() {
        16 => Aes128EcbCipher::new_from_slices(key, &[]).map(|c| c.encrypt_vec(input)),
        24 => Aes192EcbCipher::new_from_slices(key, &[]).map(|c| c.encrypt_vec(input)),
        32 => Aes256EcbCipher::new_from_slices(key, &[]).map(|c| c.encrypt_vec(input)),
        _ => Err(block_modes::InvalidKeyIvLength),
    }
    .unwrap_or_default()
}

/// AES-ECB + PKCS7, mirroring DynXX `Core::Crypto::AES::decrypt`.
pub fn aes_decrypt(input: &[u8], key: &[u8]) -> Vec<u8> {
    if !check_aes_params(input, key) {
        return Vec::new();
    }
    match key.len() {
        16 => Aes128EcbCipher::new_from_slices(key, &[])
            .ok()
            .and_then(|c| c.decrypt_vec(input).ok()),
        24 => Aes192EcbCipher::new_from_slices(key, &[])
            .ok()
            .and_then(|c| c.decrypt_vec(input).ok()),
        32 => Aes256EcbCipher::new_from_slices(key, &[])
            .ok()
            .and_then(|c| c.decrypt_vec(input).ok()),
        _ => None,
    }
    .unwrap_or_default()
}

/// Mirrors `checkAesGcmParams`: IV exactly 12 bytes, AAD at most 16 bytes,
/// tag 96..=128 bits with an 8-bit step.
fn check_aes_gcm_params(
    input: &[u8],
    key: &[u8],
    init_vector: &[u8],
    aad: &[u8],
    tag_bits: usize,
) -> bool {
    check_aes_params(input, key)
        && init_vector.len() == 12
        && aad.len() <= 16
        && tag_bits % 8 == 0
        && (96..=128).contains(&tag_bits)
}

/// AES-GCM encrypt; the tag is appended to the ciphertext, mirroring
/// DynXX `Core::Crypto::AES::gcmEncrypt`.
pub fn aes_gcm_encrypt(
    input: &[u8],
    key: &[u8],
    init_vector: &[u8],
    aad: &[u8],
    tag_bits: usize,
) -> Vec<u8> {
    if !check_aes_gcm_params(input, key, init_vector, aad, tag_bits) {
        return Vec::new();
    }
    let Ok(nonce) = aes_gcm::Nonce::<U12>::try_from(init_vector) else {
        return Vec::new();
    };
    let mut buffer = input.to_vec();
    let tag = match key.len() {
        16 => gcm_tag_for_key!(Aes128Gcm, key, &nonce, aad, tag_bits, buffer),
        24 => gcm_tag_for_key!(Aes192Gcm, key, &nonce, aad, tag_bits, buffer),
        32 => gcm_tag_for_key!(Aes256Gcm, key, &nonce, aad, tag_bits, buffer),
        _ => None,
    };
    match tag {
        Some(tag) => {
            buffer.extend_from_slice(&tag);
            buffer
        }
        None => Vec::new(),
    }
}

/// AES-GCM decrypt; the tag is expected at the tail of `input`, mirroring
/// DynXX `Core::Crypto::AES::gcmDecrypt`.
pub fn aes_gcm_decrypt(
    input: &[u8],
    key: &[u8],
    init_vector: &[u8],
    aad: &[u8],
    tag_bits: usize,
) -> Vec<u8> {
    if !check_aes_gcm_params(input, key, init_vector, aad, tag_bits) {
        return Vec::new();
    }
    let tag_len = tag_bits / 8;
    if input.len() < tag_len {
        return Vec::new();
    }
    let (cipher_text, tag_bytes) = input.split_at(input.len() - tag_len);
    let Ok(nonce) = aes_gcm::Nonce::<U12>::try_from(init_vector) else {
        return Vec::new();
    };
    let mut buffer = cipher_text.to_vec();
    let ok = match key.len() {
        16 => gcm_verify_for_key!(Aes128Gcm, key, &nonce, aad, tag_bits, tag_bytes, buffer),
        24 => gcm_verify_for_key!(Aes192Gcm, key, &nonce, aad, tag_bits, tag_bytes, buffer),
        32 => gcm_verify_for_key!(Aes256Gcm, key, &nonce, aad, tag_bits, tag_bytes, buffer),
        _ => false,
    };
    if ok { buffer } else { Vec::new() }
}

/// Mirrors `DynXXCryptoRSAPadding`: the discriminants are OpenSSL's `RSA_*_PADDING` values, which
/// `Crypto-OpenSSL.cxx` forwards to `EVP_PKEY_CTX_set_rsa_padding`.
///
/// Only `Pkcs1` and `Oaep` can encrypt or decrypt: those are the paddings OpenSSL 3.x provides for
/// `EVP_PKEY_encrypt` / `EVP_PKEY_decrypt` (`SslV23` was dropped in 3.0, while `NoPadding`, `X931`
/// and `Pss` are for signing). DynXX hands the other values to OpenSSL, which refuses them, and
/// answers with empty bytes; this does the same. OAEP uses SHA-1, OpenSSL's default OAEP digest.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(i32)]
pub enum RsaPadding {
    Pkcs1 = 1,
    SslV23 = 2,
    NoPadding = 3,
    Oaep = 4,
    X931 = 5,
    Pss = 6,
}

impl From<i32> for RsaPadding {
    fn from(value: i32) -> Self {
        match value {
            1 => RsaPadding::Pkcs1,
            2 => RsaPadding::SslV23,
            3 => RsaPadding::NoPadding,
            4 => RsaPadding::Oaep,
            5 => RsaPadding::X931,
            _ => RsaPadding::Pss,
        }
    }
}

fn rsa_public_key_from_pem(key: &[u8]) -> Option<RsaPublicKey> {
    let pem = std::str::from_utf8(key).ok()?;
    RsaPublicKey::from_public_key_pem(pem).ok()
}

fn rsa_private_key_from_pem(key: &[u8]) -> Option<RsaPrivateKey> {
    let pem = std::str::from_utf8(key).ok()?;
    RsaPrivateKey::from_pkcs8_pem(pem)
        .or_else(|_| RsaPrivateKey::from_pkcs1_pem(pem))
        .ok()
}

/// Mirrors DynXX `Core::Crypto::RSA::encrypt`; the key is a PEM public key.
pub fn rsa_encrypt(input: &[u8], key: &[u8], padding: i32) -> Vec<u8> {
    if input.is_empty() || key.is_empty() {
        return Vec::new();
    }
    let Some(public_key) = rsa_public_key_from_pem(key) else {
        return Vec::new();
    };
    let mut rng = OsRng;
    match RsaPadding::from(padding) {
        RsaPadding::Pkcs1 => public_key
            .encrypt(&mut rng, Pkcs1v15Encrypt, input)
            .unwrap_or_default(),
        RsaPadding::Oaep => public_key
            .encrypt(&mut rng, Oaep::new::<Sha1>(), input)
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Mirrors DynXX `Core::Crypto::RSA::decrypt`; the key is a PEM private key.
pub fn rsa_decrypt(input: &[u8], key: &[u8], padding: i32) -> Vec<u8> {
    if input.is_empty() || key.is_empty() {
        return Vec::new();
    }
    let Some(private_key) = rsa_private_key_from_pem(key) else {
        return Vec::new();
    };
    match RsaPadding::from(padding) {
        RsaPadding::Pkcs1 => private_key
            .decrypt(Pkcs1v15Encrypt, input)
            .unwrap_or_default(),
        RsaPadding::Oaep => private_key
            .decrypt(Oaep::new::<Sha1>(), input)
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Mirrors DynXX `evpHash`: empty input yields empty output.
pub fn hash<D: Digest>(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return Vec::new();
    }
    let mut hasher = D::new();
    hasher.update(data);
    hasher.finalize().to_vec()
}

pub fn hash_md5(data: &[u8]) -> Vec<u8> {
    hash::<Md5>(data)
}

pub fn hash_sha1(data: &[u8]) -> Vec<u8> {
    hash::<Sha1>(data)
}

pub fn hash_sha256(data: &[u8]) -> Vec<u8> {
    hash::<Sha256>(data)
}

/// Mirrors DynXX `Core::Crypto::rand` (OpenSSL `RAND_bytes`).
pub fn rand(len: usize) -> Vec<u8> {
    if len == 0 {
        return Vec::new();
    }
    let mut out = vec![0u8; len];
    rand::rng().fill_bytes(&mut out);
    out
}

/// Mirrors DynXX `Base64::validate`: non-empty, length a multiple of 4 and every
/// character inside the standard alphabet (`=` included).
pub fn base64_validate(input: &str) -> bool {
    !input.is_empty()
        && input.len() % 4 == 0
        && input
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'+' || c == b'/' || c == b'=')
}

/// Mirrors DynXX `Base64::encode(in, noNewLines)`; `no_new_lines == false` wraps the
/// output every 64 characters, like OpenSSL's `BIO_f_base64` default.
pub fn base64_encode(data: &[u8], no_new_lines: bool) -> Vec<u8> {
    if data.is_empty() {
        return Vec::new();
    }
    let encoded = general_purpose::STANDARD.encode(data);
    if no_new_lines {
        return encoded.into_bytes();
    }
    let mut out = Vec::with_capacity(encoded.len() + encoded.len() / 64 + 1);
    for chunk in encoded.as_bytes().chunks(64) {
        out.extend_from_slice(chunk);
        out.push(b'\n');
    }
    out
}

/// Mirrors DynXX `Base64::decode(in, noNewLines)`.
pub fn base64_decode(data: &[u8], no_new_lines: bool) -> Vec<u8> {
    if data.is_empty() {
        return Vec::new();
    }
    let raw: Vec<u8> = if no_new_lines {
        data.to_vec()
    } else {
        data.iter()
            .copied()
            .filter(|c| *c != b'\n' && *c != b'\r')
            .collect()
    };
    let Ok(s) = std::str::from_utf8(&raw) else {
        return Vec::new();
    };
    if !base64_validate(s) {
        return Vec::new();
    }
    general_purpose::STANDARD.decode(s).unwrap_or_default()
}

/// Mirrors DynXX `Core::Crypto::RSA::genKey`: wraps a base64 DER blob into PEM text
/// with 64-character lines. Despite the name it does not generate a key pair.
pub fn rsa_gen_key(base64: &str, is_public: bool) -> String {
    let cleaned: String = base64.split_whitespace().collect();
    if !base64_validate(&cleaned) {
        return String::new();
    }
    let label = if is_public { "PUBLIC" } else { "PRIVATE" };
    let mut pem = String::with_capacity(cleaned.len() + cleaned.len() / 64 + 64);
    pem.push_str("-----BEGIN ");
    pem.push_str(label);
    pem.push_str(" KEY-----\n");
    for chunk in cleaned.as_bytes().chunks(64) {
        pem.push_str(&String::from_utf8_lossy(chunk));
        pem.push('\n');
    }
    pem.push_str("-----END ");
    pem.push_str(label);
    pem.push_str(" KEY-----\n");
    pem
}

#[cfg(test)]
mod tests {
    use super::*;

    /// NIST GCM test case 2 (AES-128, empty AAD): validates encryption *and* the tag
    /// against the standard, not just a self-consistent round trip.
    #[test]
    fn aes_gcm_matches_nist_vector() {
        let key = [0u8; 16];
        let iv = [0u8; 12];
        let plain = [0u8; 16];
        let expected_cipher = hex::decode("0388dace60b6a392f328c2b971b2fe78").unwrap();
        let expected_tag = hex::decode("ab6e47d42cec13bdf53a67b21257bddf").unwrap();

        let out = aes_gcm_encrypt(&plain, &key, &iv, &[], 128);
        assert_eq!(&out[..16], &expected_cipher[..]);
        assert_eq!(&out[16..], &expected_tag[..]);

        let back = aes_gcm_decrypt(&out, &key, &iv, &[], 128);
        assert_eq!(back, plain.to_vec());
    }

    #[test]
    fn aes_gcm_round_trip_all_key_and_tag_sizes() {
        let plain = b"dynxx-gcm-payload";
        let aad = b"aad<=16bytes";
        for key_len in [16, 24, 32] {
            for tag_bits in [96, 104, 112, 120, 128] {
                let key = vec![0x11u8; key_len];
                let iv = vec![0x22u8; 12];
                let enc = aes_gcm_encrypt(plain, &key, &iv, aad, tag_bits);
                assert_eq!(enc.len(), plain.len() + tag_bits / 8);
                assert_eq!(
                    aes_gcm_decrypt(&enc, &key, &iv, aad, tag_bits),
                    plain.to_vec()
                );
                // A tampered tag must not decrypt.
                let mut bad = enc.clone();
                let last = bad.len() - 1;
                bad[last] ^= 0xFF;
                assert!(aes_gcm_decrypt(&bad, &key, &iv, aad, tag_bits).is_empty());
            }
        }
    }

    #[test]
    fn aes_ecb_pkcs7_matches_known_block() {
        let key = [0u8; 16];
        let plain = [0u8; 16];
        let out = aes_encrypt(&plain, &key);
        // PKCS7 always adds a full padding block here, so the output is two blocks long.
        assert_eq!(out.len(), 32);
        assert_eq!(hex::encode(&out[..16]), "66e94bd4ef8a2c3b884cfa59ca342b2e");
        assert_eq!(aes_decrypt(&out, &key), plain.to_vec());
    }

    #[test]
    fn base64_new_line_options() {
        let data = vec![0x41u8; 100];
        let flat = base64_encode(&data, true);
        assert!(!flat.contains(&b'\n'));
        assert_eq!(base64_decode(&flat, true), data);

        let wrapped = base64_encode(&data, false);
        assert!(wrapped.contains(&b'\n'));
        assert_eq!(base64_decode(&wrapped, false), data);
    }

    #[test]
    fn rsa_gen_key_wraps_base64_into_pem() {
        let body = "MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8A";
        let pem = rsa_gen_key(body, true);
        assert!(pem.starts_with("-----BEGIN PUBLIC KEY-----\n"));
        assert!(pem.trim_end().ends_with("-----END PUBLIC KEY-----"));
        assert!(rsa_gen_key("not base64!!", true).is_empty());
    }

    #[test]
    fn empty_input_yields_empty_output() {
        assert!(hash_md5(&[]).is_empty());
        assert!(aes_encrypt(&[], &[0u8; 16]).is_empty());
        assert!(base64_encode(&[], true).is_empty());
        assert!(rand(0).is_empty());
        assert_eq!(rand(16).len(), 16);
    }

    #[test]
    fn rsa_round_trips_with_the_paddings_encryption_supports() {
        use rsa::pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding};

        let private = RsaPrivateKey::new(&mut OsRng, 1024).expect("a key pair is generated");
        let public = RsaPublicKey::from(&private);
        let private_pem = private
            .to_pkcs8_pem(LineEnding::LF)
            .expect("the private key encodes to PEM");
        let public_pem = public
            .to_public_key_pem(LineEnding::LF)
            .expect("the public key encodes to PEM");

        let message = b"rsa payload";

        // 1 = PKCS#1 v1.5, 4 = OAEP, the two paddings OpenSSL accepts for encryption.
        for padding in [1, 4] {
            let encrypted = rsa_encrypt(message, public_pem.as_bytes(), padding);
            assert!(!encrypted.is_empty(), "padding {padding} encrypts");
            assert_ne!(encrypted.as_slice(), message.as_slice());

            let decrypted = rsa_decrypt(&encrypted, private_pem.as_bytes(), padding);
            assert_eq!(
                decrypted.as_slice(),
                message.as_slice(),
                "padding {padding} restores the input"
            );
        }

        // 2 = SSLv23 (dropped by OpenSSL 3.0), 3 = no padding, 5 = X9.31 and 6 = PSS, which only
        // sign: DynXX forwards them to OpenSSL, which refuses, so both sides answer with nothing.
        for padding in [2, 3, 5, 6] {
            assert!(
                rsa_encrypt(message, public_pem.as_bytes(), padding).is_empty(),
                "padding {padding} cannot encrypt"
            );
            assert!(
                rsa_decrypt(&[0x21; 128], private_pem.as_bytes(), padding).is_empty(),
                "padding {padding} cannot decrypt"
            );
        }
    }

    #[test]
    fn a_private_key_with_a_prime_of_one_is_refused_instead_of_panicking() {
        // CVE-2026-21895: before rsa 0.9.10 this divided by zero rather than returning an error,
        // and `rsa_private_key_from_pem` reaches the same constructor with an untrusted PEM key.
        let error = RsaPrivateKey::from_components(
            rsa::BigUint::from(239u64),
            rsa::BigUint::from(185u64),
            rsa::BigUint::from(0u64),
            vec![rsa::BigUint::from(1u64), rsa::BigUint::from(239u64)],
        )
        .unwrap_err();

        assert_eq!(error, rsa::Error::InvalidPrime);
    }
}
