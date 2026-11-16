//! jrnl v1/v2/v3 encryption. v3 = JRNLv3 magic + u16 header len + base64 JSON
//! header (random salt) + Fernet token. Fernet implemented per spec.
//!
//! Layering: `encrypt_v3_with_password` / `decrypt_with` are pure (no prompts,
//! no keyring) and are what the unit tests exercise. `encrypt_v3` / `decrypt`
//! add the interactive password + keyring handling on top.

use aes::cipher::{
    BlockDecryptMut, BlockEncryptMut, KeyIvInit,
    block_padding::{NoPadding, Pkcs7},
};
use aes::{Aes128, Aes256};
use anyhow::{Result, anyhow, bail};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE},
};
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::{Digest, Sha256};

use crate::ui;

type HmacSha256 = Hmac<Sha256>;
const V3_MAGIC: &[u8] = b"JRNLv3";
const V2_SALT: &[u8] = b"\xf2\xd5q\x0e\xc1\x8d.\xde\xdc\x8e6t\x89\x04\xce\xf8";
const ITERATIONS: u32 = 100_000;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Version {
    V1,
    V2,
    V3,
}
impl Version {
    pub fn label(&self) -> &'static str {
        match self {
            Self::V1 => "v1",
            Self::V2 => "v2",
            Self::V3 => "v3",
        }
    }
}

pub fn detect(data: &[u8], v1_hint: bool) -> Version {
    if data.starts_with(V3_MAGIC) {
        return Version::V3;
    }
    if v1_hint {
        return Version::V1;
    }
    if data.starts_with(b"gAAAAA") {
        return Version::V2;
    }
    Version::V1
}

// ---------- key derivation ----------
fn pbkdf2_sha256(pw: &[u8], salt: &[u8], iters: u32, out: &mut [u8]) {
    for (i, block) in out.chunks_mut(32).enumerate() {
        let mut mac = HmacSha256::new_from_slice(pw).unwrap();
        mac.update(salt);
        mac.update(&((i as u32 + 1).to_be_bytes()));
        let mut u = mac.finalize().into_bytes();
        let mut t = u;
        for _ in 1..iters {
            let mut mac = HmacSha256::new_from_slice(pw).unwrap();
            mac.update(&u);
            u = mac.finalize().into_bytes();
            for (a, b) in t.iter_mut().zip(u.iter()) {
                *a ^= *b;
            }
        }
        block.copy_from_slice(&t[..block.len()]);
    }
}

fn fernet_key(password: &str, salt: &[u8]) -> [u8; 32] {
    let mut key = [0u8; 32];
    pbkdf2_sha256(password.as_bytes(), salt, ITERATIONS, &mut key);
    key
}

// ---------- Fernet (pure) ----------
fn fernet_encrypt(key: &[u8; 32], plaintext: &[u8]) -> Result<String> {
    let mut iv = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut iv);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    // PKCS7 always adds 1..=16 bytes of padding, hence the +1 block.
    let mut buf = vec![0u8; (plaintext.len() / 16 + 1) * 16];
    buf[..plaintext.len()].copy_from_slice(plaintext);
    // Fernet: AES-128-CBC with the *second* 16 bytes of the key.
    let ct = cbc::Encryptor::<Aes128>::new_from_slices(&key[16..], &iv)
        .map_err(|_| anyhow!("bad key"))?
        .encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext.len())
        .map_err(|_| anyhow!("padding error"))?;
    let mut body = vec![0x80u8];
    body.extend_from_slice(&ts.to_be_bytes());
    body.extend_from_slice(&iv);
    body.extend_from_slice(ct);
    // ...and HMAC-SHA256 with the *first* 16 bytes.
    let mut mac = HmacSha256::new_from_slice(&key[..16]).unwrap();
    mac.update(&body);
    body.extend_from_slice(&mac.finalize().into_bytes());
    Ok(URL_SAFE.encode(body))
}

fn fernet_decrypt(key: &[u8; 32], token: &str) -> Option<Vec<u8>> {
    let data = URL_SAFE.decode(token.trim()).ok()?;
    if data.len() < 1 + 8 + 16 + 32 || data[0] != 0x80 {
        return None;
    }
    let (body, tag) = data.split_at(data.len() - 32);
    let mut mac = HmacSha256::new_from_slice(&key[..16]).ok()?;
    mac.update(body);
    mac.verify_slice(tag).ok()?;
    let iv = &body[9..25];
    let mut buf = body[25..].to_vec();
    if buf.is_empty() || buf.len() % 16 != 0 {
        return None;
    }
    let pt = cbc::Decryptor::<Aes128>::new_from_slices(&key[16..], iv)
        .ok()?
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .ok()?;
    Some(pt.to_vec())
}

// ---------- v1 (pure, read-only legacy format) ----------
fn v1_decrypt(data: &[u8], password: &str) -> Option<String> {
    if data.len() < 32 || (data.len() - 16) % 16 != 0 {
        return None;
    }
    let key: [u8; 32] = Sha256::digest(password.as_bytes()).into();
    let (iv, ct) = data.split_at(16);
    let mut buf = ct.to_vec();
    let plain = cbc::Decryptor::<Aes256>::new_from_slices(&key, iv)
        .ok()?
        .decrypt_padded_mut::<NoPadding>(&mut buf)
        .ok()?;
    let last = *plain.last()?;
    let bytes = if last == b' ' {
        // Ancient space-padded journals.
        let end = plain
            .iter()
            .rposition(|b| *b != b' ')
            .map(|i| i + 1)
            .unwrap_or(0);
        &plain[..end]
    } else {
        let n = last as usize;
        if n == 0
            || n > 16
            || n > plain.len()
            || plain[plain.len() - n..].iter().any(|b| *b != last)
        {
            return None;
        }
        &plain[..plain.len() - n]
    };
    String::from_utf8(bytes.to_vec()).ok()
}

// ---------- v3 header (pure) ----------
fn v3_parse(data: &[u8]) -> Option<(Vec<u8>, String)> {
    if !data.starts_with(V3_MAGIC) || data.len() < 8 {
        return None;
    }
    let len = u16::from_be_bytes([data[6], data[7]]) as usize;
    let field = data.get(8..8 + len)?;
    let json_bytes = match STANDARD.decode(field) {
        Ok(b) => b,
        Err(_) => field.to_vec(), // legacy raw-JSON header
    };
    let header: serde_json::Value = serde_json::from_slice(&json_bytes).ok()?;
    let salt = URL_SAFE.decode(header.get("salt")?.as_str()?).ok()?;
    let token = String::from_utf8(data[8 + len..].to_vec()).ok()?;
    Some((salt, token))
}

fn decrypt_with(data: &[u8], ver: Version, password: &str) -> Option<String> {
    match ver {
        Version::V1 => v1_decrypt(data, password),
        Version::V2 => {
            let key = fernet_key(password, V2_SALT);
            let token = String::from_utf8(data.to_vec()).ok()?;
            String::from_utf8(fernet_decrypt(&key, &token)?).ok()
        }
        Version::V3 => {
            let (salt, token) = v3_parse(data)?;
            let key = fernet_key(password, &salt);
            String::from_utf8(fernet_decrypt(&key, &token)?).ok()
        }
    }
}

/// Pure v3 encryption: no prompts, no keyring. Safe for tests and scripting.
pub fn encrypt_v3_with_password(text: &str, password: &str) -> Result<Vec<u8>> {
    let mut salt = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut salt);
    let header = serde_json::json!({ "salt": URL_SAFE.encode(salt) });
    let header_json = serde_json::to_vec(&header)?;
    let header_b64 = STANDARD.encode(&header_json);
    if header_b64.len() > u16::MAX as usize {
        bail!("Encryption header too large");
    }
    let key = fernet_key(password, &salt);
    let token = fernet_encrypt(&key, text.as_bytes())?;

    let mut out = V3_MAGIC.to_vec();
    out.extend_from_slice(&(header_b64.len() as u16).to_be_bytes());
    out.extend_from_slice(header_b64.as_bytes());
    out.extend_from_slice(token.as_bytes());
    Ok(out)
}

// ---------- keyring ----------
fn keyring_get(name: &str) -> Option<String> {
    match keyring::Entry::new("jrnl", name) {
        Ok(e) => match e.get_password() {
            Ok(p) => Some(p),
            Err(keyring::Error::NoEntry) => None,
            Err(_) => {
                ui::warning("Failed to retrieve keyring");
                None
            }
        },
        Err(_) => None,
    }
}

fn keyring_set(name: &str, password: &str) {
    match keyring::Entry::new("jrnl", name) {
        Ok(e) => {
            if e.set_password(password).is_err() {
                ui::warning("Failed to retrieve keyring");
            }
        }
        Err(_) => ui::warning("Keyring backend not found. See https://pypi.org/project/keyring/"),
    }
}

// ---------- interactive layer ----------
fn create_password(name: &str) -> Result<String> {
    loop {
        // Fail fast instead of looping forever when no TTY is available.
        let pw = rpassword::prompt_password(format!("Enter password for journal '{name}': "))
            .map_err(|_| anyhow!("Cannot read password: no interactive terminal available"))?;
        if pw.is_empty() {
            ui::warning("Password can't be empty!");
            continue;
        }
        let confirm = rpassword::prompt_password("Enter password again: ")
            .map_err(|_| anyhow!("Cannot read password: no interactive terminal available"))?;
        if pw == confirm {
            if ui::yes_no("Do you want to store the password in your keychain?", true) {
                keyring_set(name, &pw);
            }
            return Ok(pw);
        }
        ui::error("Passwords did not match, please try again");
    }
}

/// Decrypt, prompting (keyring first, then up to 3 password attempts).
pub fn decrypt(
    data: &[u8],
    ver: Version,
    name: &str,
    cache: &mut Option<String>,
) -> Result<String> {
    if cache.is_none() {
        *cache = keyring_get(name);
    }
    let mut attempts = 0;
    loop {
        if cache.is_none() {
            if attempts >= 3 {
                bail!("Too many attempts with wrong password");
            }
            if attempts > 0 {
                ui::warning("Wrong password, try again");
            }
            *cache = Some(ui::prompt_password("Password: "));
            attempts += 1;
        }
        let pw = cache.clone().unwrap();
        if let Some(text) = decrypt_with(data, ver, &pw) {
            return Ok(text);
        }
        *cache = None; // wrong password (or corrupt data) — retry
    }
}

/// Encrypt as v3, reusing a known password or creating a new one.
pub fn encrypt_v3(
    text: &str,
    name: &str,
    cache: &mut Option<String>,
    force_new: bool,
) -> Result<Vec<u8>> {
    if force_new {
        *cache = None;
    }
    if cache.is_none() {
        *cache = keyring_get(name);
    }
    if cache.is_none() {
        *cache = Some(create_password(name)?);
    }
    encrypt_v3_with_password(text, cache.as_deref().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v3_roundtrip_and_detect() {
        let ct = encrypt_v3_with_password("hello 世界 🎉", "test password").unwrap();
        assert_eq!(detect(&ct, false), Version::V3);
        assert_eq!(
            decrypt_with(&ct, Version::V3, "test password").unwrap(),
            "hello 世界 🎉"
        );
        assert!(decrypt_with(&ct, Version::V3, "wrong").is_none());
    }

    #[test]
    fn unique_salts() {
        let a = encrypt_v3_with_password("x", "pw").unwrap();
        let b = encrypt_v3_with_password("x", "pw").unwrap();
        assert_ne!(v3_parse(&a).unwrap().0, v3_parse(&b).unwrap().0);
    }

    #[test]
    fn legacy_raw_json_header_still_decrypts() {
        // Pre-base64 v3 journals stored the JSON header directly (jrnl back-compat).
        let salt = [7u8; 16];
        let header = format!(r#"{{"salt":"{}"}}"#, URL_SAFE.encode(salt));
        let key = fernet_key("pw", &salt);
        let token = fernet_encrypt(&key, b"old entry").unwrap();
        let mut blob = V3_MAGIC.to_vec();
        blob.extend_from_slice(&(header.len() as u16).to_be_bytes());
        blob.extend_from_slice(header.as_bytes());
        blob.extend_from_slice(token.as_bytes());
        assert_eq!(decrypt_with(&blob, Version::V3, "pw").unwrap(), "old entry");
    }
}
