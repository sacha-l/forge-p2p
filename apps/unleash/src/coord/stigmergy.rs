//! Lamport-clock-versioned key/value store, gossiped for eventual
//! consistency. Higher Lamport wins; ties broken by `robot_id`.

use std::collections::HashMap;
use std::sync::RwLock;

use crate::keyspace::StigmergyUpdate;

pub struct Stigmergy {
    self_id: String,
    inner: RwLock<Inner>,
}

struct Inner {
    kv: HashMap<String, (String, u64, String)>, // key -> (value, lamport, robot_id)
    local_lamport: u64,
}

impl Stigmergy {
    pub fn new(self_id: String) -> Self {
        Self {
            self_id,
            inner: RwLock::new(Inner {
                kv: HashMap::new(),
                local_lamport: 0,
            }),
        }
    }

    pub fn set(&self, key: &str, value: &str) -> StigmergyUpdate {
        let mut inner = self.inner.write().expect("stigmergy poisoned");
        inner.local_lamport += 1;
        let lamport = inner.local_lamport;
        inner.kv.insert(
            key.to_string(),
            (value.to_string(), lamport, self.self_id.clone()),
        );
        StigmergyUpdate {
            key: key.to_string(),
            value: value.to_string(),
            lamport,
            robot_id: self.self_id.clone(),
        }
    }

    /// Returns `true` iff the local state changed.
    pub fn apply(&self, u: StigmergyUpdate) -> bool {
        let mut inner = self.inner.write().expect("stigmergy poisoned");
        inner.local_lamport = inner.local_lamport.max(u.lamport);
        let entry = inner
            .kv
            .entry(u.key.clone())
            .or_insert((String::new(), 0, String::new()));
        if u.lamport > entry.1 || (u.lamport == entry.1 && u.robot_id > entry.2) {
            *entry = (u.value, u.lamport, u.robot_id);
            true
        } else {
            false
        }
    }

    pub fn get(&self, key: &str) -> Option<(String, u64, String)> {
        self.inner
            .read()
            .expect("stigmergy poisoned")
            .kv
            .get(key)
            .cloned()
    }

    pub fn snapshot(&self) -> HashMap<String, (String, u64, String)> {
        self.inner
            .read()
            .expect("stigmergy poisoned")
            .kv
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn later_lamport_wins() {
        let a = Stigmergy::new("a".into());
        let b = Stigmergy::new("b".into());
        let ua = a.set("k", "v1");
        let ub = b.set("k", "v2");
        a.apply(ub);
        b.apply(ua);
        // Both a and b wrote lamport=1; tie-break by robot_id = b wins.
        assert_eq!(a.get("k").unwrap().0, "v2");
        assert_eq!(b.get("k").unwrap().0, "v2");
    }

    #[test]
    fn later_update_overwrites() {
        let a = Stigmergy::new("a".into());
        let _u1 = a.set("k", "v1");
        let u2 = a.set("k", "v2");
        assert_eq!(a.get("k").unwrap().0, "v2");
        // Re-applying old lamport should not regress.
        let regress = StigmergyUpdate {
            key: "k".into(),
            value: "stale".into(),
            lamport: 1,
            robot_id: "a".into(),
        };
        assert!(!a.apply(regress));
        assert_eq!(a.get("k").unwrap().0, "v2");
        let _ = u2;
    }

    #[test]
    fn partition_heals_on_apply() {
        let a = Stigmergy::new("a".into());
        let b = Stigmergy::new("b".into());
        // Partition: a writes twice, b once
        let _u1 = a.set("k", "a1");
        let u2 = a.set("k", "a2");
        let ub = b.set("k", "b1");
        // Heal
        b.apply(u2.clone());
        a.apply(ub);
        // Both converge to a's second write (higher lamport = 2 vs b's 1)
        assert_eq!(a.get("k").unwrap().0, "a2");
        assert_eq!(b.get("k").unwrap().0, "a2");
    }
}
