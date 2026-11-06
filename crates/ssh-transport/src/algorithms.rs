//! SSH algorithm policy (Issue #1173, Spec 0005 "Outdated algorithms").
//!
//! The client offers the algorithms of russh's defaults first, so a modern
//! server always negotiates a modern one, and appends a fixed list of legacy
//! algorithms that old servers and devices still depend on. After the key
//! exchange the negotiated names are checked against that list: a hit is a
//! warning, never a refusal.

use std::borrow::Cow;

use russh::keys::Algorithm;
use russh::{cipher, kex, mac, Preferred};
use ssh_manager_core::ssh::STRICT_KEX_MISSING;

/// Legacy key exchange algorithms, offered after all current ones.
pub(crate) const LEGACY_KEX: &[kex::Name] = &[kex::DH_G14_SHA1, kex::DH_GEX_SHA1, kex::DH_G1_SHA1];

/// Legacy ciphers, offered after all current ones.
pub(crate) const LEGACY_CIPHERS: &[cipher::Name] = &[
    cipher::AES_128_CBC,
    cipher::AES_192_CBC,
    cipher::AES_256_CBC,
    cipher::TRIPLE_DES_CBC,
];

/// Legacy MACs, offered after all current ones.
pub(crate) const LEGACY_MACS: &[mac::Name] = &[mac::HMAC_SHA1, mac::HMAC_SHA1_ETM];

/// Host key and user key signature algorithm `ssh-rsa` (RSA with SHA-1).
pub(crate) const LEGACY_HOST_KEY: &str = "ssh-rsa";

/// The preference lists of every connection and jump hop: russh's defaults
/// followed by the legacy algorithms. `ssh-rsa` is already the last entry of
/// the default host key list; it is kept explicitly so the order rule does
/// not depend on russh's defaults.
pub(crate) fn preferred() -> Preferred {
    let default = Preferred::DEFAULT;
    let mut kex: Vec<kex::Name> = default.kex.to_vec();
    kex.extend(LEGACY_KEX.iter().copied());
    let mut key: Vec<Algorithm> = default.key.to_vec();
    let ssh_rsa = Algorithm::Rsa { hash: None };
    if !key.contains(&ssh_rsa) {
        key.push(ssh_rsa);
    }
    let mut ciphers: Vec<cipher::Name> = default.cipher.to_vec();
    ciphers.extend(LEGACY_CIPHERS.iter().copied());
    let mut macs: Vec<mac::Name> = default.mac.to_vec();
    macs.extend(LEGACY_MACS.iter().copied());
    Preferred {
        kex: Cow::Owned(kex),
        key: Cow::Owned(key),
        cipher: Cow::Owned(ciphers),
        mac: Cow::Owned(macs),
        ..default
    }
}

/// The negotiated algorithm names of one hop that are outdated, plus the
/// Terrapin marker when the server lacks strict key exchange. Empty if
/// nothing is outdated.
pub(crate) fn outdated(
    kex_name: &str,
    host_key: &str,
    cipher_name: &str,
    client_mac: &str,
    server_mac: &str,
    strict_kex: bool,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |name: &str| {
        if !out.iter().any(|n| n == name) {
            out.push(name.to_string());
        }
    };
    if LEGACY_KEX.iter().any(|k| k.as_ref() == kex_name) {
        push(kex_name);
    }
    if host_key == LEGACY_HOST_KEY {
        push(host_key);
    }
    if LEGACY_CIPHERS.iter().any(|c| c.as_ref() == cipher_name) {
        push(cipher_name);
    }
    for m in [client_mac, server_mac] {
        if LEGACY_MACS.iter().any(|l| l.as_ref() == m) {
            push(m);
        }
    }
    if !strict_kex {
        push(STRICT_KEX_MISSING);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_algorithms_come_strictly_after_the_defaults() {
        let p = preferred();
        let d = Preferred::DEFAULT;
        assert_eq!(&p.kex[..d.kex.len()], &d.kex[..]);
        assert_eq!(&p.cipher[..d.cipher.len()], &d.cipher[..]);
        assert_eq!(&p.mac[..d.mac.len()], &d.mac[..]);
        assert_eq!(&p.key[..d.key.len()], &d.key[..]);
        for k in LEGACY_KEX {
            assert!(!d.kex.contains(k));
            assert!(p.kex.contains(k));
        }
        for c in LEGACY_CIPHERS {
            assert!(!d.cipher.contains(c));
            assert!(p.cipher.contains(c));
        }
        for m in LEGACY_MACS {
            assert!(!d.mac.contains(m));
            assert!(p.mac.contains(m));
        }
        assert_eq!(p.key.last(), Some(&Algorithm::Rsa { hash: None }));
    }

    #[test]
    fn modern_names_are_not_outdated() {
        assert!(outdated(
            "curve25519-sha256",
            "ssh-ed25519",
            "chacha20-poly1305@openssh.com",
            "none",
            "none",
            true
        )
        .is_empty());
    }

    #[test]
    fn each_legacy_name_is_reported_once() {
        let o = outdated(
            "diffie-hellman-group14-sha1",
            "ssh-rsa",
            "aes128-cbc",
            "hmac-sha1",
            "hmac-sha1",
            false,
        );
        assert_eq!(
            o,
            vec![
                "diffie-hellman-group14-sha1",
                "ssh-rsa",
                "aes128-cbc",
                "hmac-sha1",
                STRICT_KEX_MISSING
            ]
        );
    }
}
