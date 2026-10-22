//! Who a session is: the key the server hands each pane (SAAIO_KEY) and its proof

use std::fmt::Write;
use std::io;

use sha2::{Digest, Sha256};

use crate::store::{Store, Waiter};

/// How many random bytes a key holds.
const KEY_BYTES: usize = 32;

/// A fresh key: 32 random bytes as 64 hex digits.
pub fn new_key() -> io::Result<String> {
    let mut bytes = [0u8; KEY_BYTES];
    getrandom::fill(&mut bytes).map_err(io::Error::other)?;
    let mut key = String::with_capacity(KEY_BYTES * 2);
    for byte in bytes {
        write!(key, "{byte:02x}").expect("writing to a String never fails");
    }
    Ok(key)
}

/// SHA-256 of `key` as hex: what the store keeps, so the key itself lives only in the pane.
pub fn digest(key: &str) -> String {
    format!("{:x}", Sha256::digest(key.as_bytes()))
}

/// Whether `key` is the one the server handed `waiter`'s pane; false without a key or a record.
pub fn proves(store: &Store, waiter: &Waiter, key: Option<&str>) -> bool {
    let Some(key) = key.filter(|key| !key.is_empty()) else {
        return false;
    };
    store
        .key(waiter)
        .is_some_and(|kept| kept.trim() == digest(key))
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn a_new_key_is_64_hex_digits_and_never_the_same() {
        let key = new_key().unwrap();

        assert_eq!(key.len(), 64);
        assert!(key.chars().all(|digit| digit.is_ascii_hexdigit()), "{key}");
        assert_ne!(key, new_key().unwrap());
    }

    #[test]
    fn digests_a_key_with_sha256() {
        assert_eq!(
            digest("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn only_the_key_handed_to_the_pane_proves_its_session() {
        let dir = TempDir::new().unwrap();
        let store = Store::at(dir.path().to_path_buf());
        let worker = Waiter::Flow("7f3c".into());
        let key = new_key().unwrap();

        assert!(!proves(&store, &worker, Some(&key)), "no record yet");
        store.set_key(&worker, &digest(&key)).unwrap();

        assert!(proves(&store, &worker, Some(&key)));
        assert!(!proves(&store, &worker, Some(&new_key().unwrap())));
        assert!(!proves(&store, &worker, Some("")));
        assert!(!proves(&store, &worker, None));
        assert!(!proves(&store, &Waiter::Master, Some(&key)));
    }
}
