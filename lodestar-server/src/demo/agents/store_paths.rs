//! Fake but well-formed Nix store paths.

use std::hash::{DefaultHasher, Hash, Hasher};

/// Nix's base32 alphabet, for store path hashes.
const NIX_BASE32: &[u8] = b"0123456789abcdfghijklmnpqrsvwxyz";

/// A fake but well-formed store path hash (32 chars of Nix base32), stable for `seed`.
pub(super) fn store_hash(seed: &str) -> String {
    let mut out = String::with_capacity(32);
    let mut round = 0u64;
    while out.len() < 32 {
        let mut hasher = DefaultHasher::new();
        (seed, round).hash(&mut hasher);
        let mut value = hasher.finish();
        for _ in 0..12 {
            if out.len() == 32 {
                break;
            }
            out.push(NIX_BASE32[(value % 32) as usize] as char);
            value /= 32;
        }
        round += 1;
    }
    out
}

/// `/nix/store/<hash>-<name>` of the system built from `commit`.
pub(super) fn system_path(host: &str, os: &str, commit: &str) -> String {
    let short = &commit[..commit.len().min(7)];
    let name = if os == "darwin" {
        format!("darwin-system-25.11.{short}")
    } else {
        format!("nixos-system-{host}-25.11.20261010.{short}")
    };
    format!(
        "/nix/store/{}-{name}",
        store_hash(&format!("{host}{commit}"))
    )
}
