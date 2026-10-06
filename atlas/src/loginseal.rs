//! Sealing a key to your Windows sign-in.
//!
//! The vault opens with your passphrase or your recovery key. Neither works
//! for something scheduled at 6 a.m.: nobody is there to type. The usual
//! answers are to store the passphrase somewhere (a secret you then have to
//! keep safe, which is the design you've ruled out) or to leave the vault
//! open (worse).
//!
//! Windows has a third: the Data Protection API seals bytes so that only the
//! same Windows account, signed in on the same machine, can unseal them. The
//! key to that is your Windows sign-in itself, which you already use every
//! day and which Windows lets you reset. So a copy of the vault's data key is
//! sealed that way (a `How::ThisLogin` wrap), and scheduled work can open
//! the vault while you're signed in — for the kinds of secret that
//! `vault::Kind::usable_unattended` allows, and never the recovery codes or
//! authenticator seeds.
//!
//! Only on Windows. On Linux the equivalent is the desktop keyring (Secret
//! Service over D-Bus), which isn't wired, and this says so.

/// Extra bytes mixed into the seal, so a blob made for Atlas's vault can't be
/// unsealed by a program asking for a different purpose.
#[cfg(windows)]
const PURPOSE: &[u8] = b"atlas vault data key v1";

pub fn available() -> bool {
    #[cfg(windows)]
    {
        true
    }
    #[cfg(not(windows))]
    {
        stand_in()
    }
}

#[cfg(windows)]
pub fn seal(data: &[u8]) -> Result<Vec<u8>, String> {
    use windows::Win32::Security::Cryptography::{CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB};
    let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
    let entropy = CRYPT_INTEGER_BLOB { cbData: PURPOSE.len() as u32, pbData: PURPOSE.as_ptr() as *mut u8 };
    let mut out = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptProtectData(&input, windows::core::w!("Atlas vault"), Some(&entropy), None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
            .map_err(|e| format!("Windows wouldn't seal it: {e}"))?;
        let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        let _ = windows::Win32::Foundation::LocalFree(windows::Win32::Foundation::HLOCAL(out.pbData as *mut _));
        Ok(v)
    }
}

#[cfg(windows)]
pub fn unseal(blob: &[u8]) -> Result<Vec<u8>, String> {
    use windows::Win32::Security::Cryptography::{CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB};
    let input = CRYPT_INTEGER_BLOB { cbData: blob.len() as u32, pbData: blob.as_ptr() as *mut u8 };
    let entropy = CRYPT_INTEGER_BLOB { cbData: PURPOSE.len() as u32, pbData: PURPOSE.as_ptr() as *mut u8 };
    let mut out = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptUnprotectData(&input, None, Some(&entropy), None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
            .map_err(|e| format!("this Windows sign-in can't open it: {e}"))?;
        let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        let _ = windows::Win32::Foundation::LocalFree(windows::Win32::Foundation::HLOCAL(out.pbData as *mut _));
        Ok(v)
    }
}

#[cfg(not(windows))]
pub fn seal(data: &[u8]) -> Result<Vec<u8>, String> {
    if stand_in() {
        return Ok([STAND_IN, data].concat());
    }
    Err(NOT_HERE.into())
}

#[cfg(not(windows))]
pub fn unseal(blob: &[u8]) -> Result<Vec<u8>, String> {
    if stand_in() {
        return blob.strip_prefix(STAND_IN).map(<[u8]>::to_vec).ok_or_else(|| "this sign-in can't open it".to_string());
    }
    Err(NOT_HERE.into())
}

/// Tests off Windows: a seal that is no seal at all, so the paths that use
/// one can be run. Only in a debug build, and only when a test asks for it
/// (`ATLAS_TEST_LOGINSEAL=1`) -- never in anything that ships.
#[cfg(not(windows))]
const STAND_IN: &[u8] = b"TEST-ONLY-NOT-SEALED:";

#[cfg(not(windows))]
fn stand_in() -> bool {
    cfg!(debug_assertions) && std::env::var("ATLAS_TEST_LOGINSEAL").as_deref() == Ok("1")
}

pub const NOT_HERE: &str = "sealing the vault to your sign-in works on Windows only; on this system the \
     vault opens with your passphrase or recovery key, and scheduled work that needs it waits for you";
