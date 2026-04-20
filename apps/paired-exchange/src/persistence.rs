//! On-disk cache of peers we have previously paired with.
//!
//! The file is a plain JSON array of base58-encoded `PeerId` strings,
//! role-scoped (`<role>_trusted_peers.json`) so two roles sharing a
//! working directory don't clobber each other. Unknown / malformed
//! file contents are treated as "no persisted peers" rather than a
//! startup error — a corrupt cache should never prevent the app from
//! running, just force a fresh handshake.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use swarm_nl::PeerId;

#[derive(Serialize, Deserialize, Default)]
struct OnDisk {
    peers: Vec<String>,
}

/// Default filename for a given role tag. Callers usually pass a
/// full path rather than relying on this, but the resolver keeps
/// the binary's two call-sites consistent.
pub fn default_path(role_label: &str) -> PathBuf {
    PathBuf::from(format!("{}_trusted_peers.json", role_label.to_lowercase()))
}

/// Load previously trusted peers. Missing or corrupt files return an
/// empty vector. Any I/O error that is *not* "file not found" is
/// surfaced via the returned error so ops can see it, but the caller
/// is free to ignore it.
pub fn load_trusted_peers(path: &Path) -> Vec<PeerId> {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "persistence: read failed");
            return Vec::new();
        }
    };
    let disk: OnDisk = match serde_json::from_slice(&bytes) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "persistence: corrupt JSON; ignoring");
            return Vec::new();
        }
    };
    disk.peers
        .into_iter()
        .filter_map(|s| match s.parse() {
            Ok(p) => Some(p),
            Err(e) => {
                tracing::warn!(peer = %s, error = %e, "persistence: unparseable peer id");
                None
            }
        })
        .collect()
}

/// Append `peer` to the trusted-peers file if it isn't already there.
/// Best-effort: any write failure is logged and swallowed — a failed
/// save means the peer will re-handshake next time, which is safe.
pub fn save_trusted_peer(path: &Path, peer: &PeerId) {
    let mut peers: Vec<String> = load_trusted_peers(path)
        .into_iter()
        .map(|p| p.to_string())
        .collect();
    let key = peer.to_string();
    if peers.contains(&key) {
        return;
    }
    peers.push(key);
    let disk = OnDisk { peers };
    match serde_json::to_vec_pretty(&disk) {
        Ok(bytes) => {
            if let Err(e) = fs::write(path, bytes) {
                tracing::warn!(path = %path.display(), error = %e, "persistence: write failed");
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "persistence: JSON encode failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("paired-exchange-persistence-{}", name));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.join("peers.json")
    }

    #[test]
    fn missing_file_returns_empty() {
        let path = tmp_path("missing");
        assert!(load_trusted_peers(&path).is_empty());
    }

    #[test]
    fn roundtrip_one_peer() {
        let path = tmp_path("one");
        let peer = PeerId::random();
        save_trusted_peer(&path, &peer);
        let loaded = load_trusted_peers(&path);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0], peer);
    }

    #[test]
    fn roundtrip_multiple_peers_deduplicated() {
        let path = tmp_path("multi");
        let p1 = PeerId::random();
        let p2 = PeerId::random();
        save_trusted_peer(&path, &p1);
        save_trusted_peer(&path, &p2);
        save_trusted_peer(&path, &p1); // duplicate — ignored
        let loaded = load_trusted_peers(&path);
        assert_eq!(loaded.len(), 2);
        assert!(loaded.contains(&p1));
        assert!(loaded.contains(&p2));
    }

    #[test]
    fn corrupt_file_returns_empty_without_error() {
        let path = tmp_path("corrupt");
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(b"this is not json").unwrap();
        assert!(load_trusted_peers(&path).is_empty());
    }

    #[test]
    fn unparseable_peer_ids_are_dropped() {
        let path = tmp_path("bad_ids");
        let good = PeerId::random();
        let disk = OnDisk {
            peers: vec![good.to_string(), "not-a-peer-id".to_string()],
        };
        fs::write(&path, serde_json::to_vec(&disk).unwrap()).unwrap();
        let loaded = load_trusted_peers(&path);
        assert_eq!(loaded, vec![good]);
    }
}
