//! The database key: 32 random bytes, hex-encoded, kept in the OS keychain
//! (Windows Credential Manager, macOS Keychain, Secret Service on Linux).

use std::fmt::Write as _;

use zeroize::Zeroizing;

pub const KEYCHAIN_SERVICE: &str = "private-context-ai";
pub const KEYCHAIN_ACCOUNT: &str = "db-key-v1";

const KEY_BYTES: usize = 32;

/// The keychain cannot be used at all: no store on this system, access
/// denied, or a platform failure. The message never contains the key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeychainUnavailable(pub String);

/// Where the database key lives. A trait so tests can use memory instead of
/// the real keychain.
pub trait KeySource: Send + Sync {
    /// The stored hex key, or `None` when there is no entry.
    fn get(&self) -> Result<Option<Zeroizing<String>>, KeychainUnavailable>;

    fn set(&self, hex: &str) -> Result<(), KeychainUnavailable>;
}

/// The real OS keychain.
pub struct KeychainKeySource;

impl KeychainKeySource {
    fn entry() -> Result<keyring::Entry, KeychainUnavailable> {
        keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT).map_err(unavailable)
    }
}

impl KeySource for KeychainKeySource {
    fn get(&self) -> Result<Option<Zeroizing<String>>, KeychainUnavailable> {
        match Self::entry()?.get_password() {
            Ok(hex) => Ok(Some(Zeroizing::new(hex))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(unavailable(err)),
        }
    }

    fn set(&self, hex: &str) -> Result<(), KeychainUnavailable> {
        Self::entry()?.set_password(hex).map_err(unavailable)
    }
}

/// Fixed messages, so a stored value can never end up in one.
fn unavailable(err: keyring::Error) -> KeychainUnavailable {
    let message = match err {
        keyring::Error::NoDefaultStore => "no keychain is available on this system".to_owned(),
        keyring::Error::NoStorageAccess(_) => "access to the keychain was denied".to_owned(),
        keyring::Error::PlatformFailure(inner) => format!("the keychain failed: {inner}"),
        keyring::Error::BadEncoding(_) | keyring::Error::BadDataFormat(..) => {
            "the stored key is not readable".to_owned()
        }
        _ => "the keychain refused the request".to_owned(),
    };
    KeychainUnavailable(message)
}

/// 32 bytes from the thread-local CSPRNG, as 64 lowercase hex characters.
pub fn new_key() -> Zeroizing<String> {
    let mut bytes = Zeroizing::new([0u8; KEY_BYTES]);
    rand::fill(&mut bytes[..]);
    // Exact capacity, so the string never reallocates and leaves no copy.
    let mut hex = Zeroizing::new(String::with_capacity(KEY_BYTES * 2));
    for byte in bytes.iter() {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

pub fn is_valid_key(hex: &str) -> bool {
    hex.len() == KEY_BYTES * 2 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
pub mod testing {
    use std::sync::Mutex;

    use super::*;

    /// An in-memory keychain that counts writes.
    #[derive(Default)]
    pub struct MemoryKeySource {
        key: Mutex<Option<String>>,
        sets: Mutex<usize>,
        unavailable: bool,
    }

    impl MemoryKeySource {
        pub fn with_key(hex: &str) -> Self {
            Self {
                key: Mutex::new(Some(hex.to_owned())),
                ..Self::default()
            }
        }

        pub fn unavailable() -> Self {
            Self {
                unavailable: true,
                ..Self::default()
            }
        }

        pub fn key(&self) -> Option<String> {
            self.key.lock().unwrap().clone()
        }

        pub fn sets(&self) -> usize {
            *self.sets.lock().unwrap()
        }

        pub fn clear(&self) {
            *self.key.lock().unwrap() = None;
        }
    }

    impl KeySource for MemoryKeySource {
        fn get(&self) -> Result<Option<Zeroizing<String>>, KeychainUnavailable> {
            if self.unavailable {
                return Err(KeychainUnavailable("no keychain".into()));
            }
            Ok(self.key.lock().unwrap().clone().map(Zeroizing::new))
        }

        fn set(&self, hex: &str) -> Result<(), KeychainUnavailable> {
            if self.unavailable {
                return Err(KeychainUnavailable("no keychain".into()));
            }
            *self.sets.lock().unwrap() += 1;
            *self.key.lock().unwrap() = Some(hex.to_owned());
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_key_is_64_hex_chars_and_random() {
        let first = new_key();
        assert!(is_valid_key(&first));
        assert_ne!(*first, *new_key());
    }

    #[test]
    fn malformed_keys_are_invalid() {
        assert!(!is_valid_key(""));
        assert!(!is_valid_key(&"a".repeat(63)));
        assert!(!is_valid_key(&"g".repeat(64)));
        assert!(is_valid_key(&"aB".repeat(32)));
    }
}
