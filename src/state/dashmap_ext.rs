use dashmap::DashMap;
use std::borrow::Borrow;
use std::hash::Hash;

/// Extension helpers for `DashMap` that avoid holding shard locks across `.await`.
///
/// `DashMap::get()` and `DashMap::iter()` return guard types that hold a shard lock.
/// Awaiting while those guards are alive can deadlock or cause severe contention.
///
/// These helpers clone values/entries so the guard drops immediately.
pub trait DashMapExt<K, V> {
    /// Clone the value for `key` (dropping the DashMap guard immediately).
    fn get_cloned<Q>(&self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
        V: Clone;

    /// Collect all `(key, value)` pairs by cloning them (dropping guards immediately).
    #[allow(dead_code)]
    fn iter_cloned(&self) -> Vec<(K, V)>
    where
        K: Clone,
        V: Clone;
}

impl<K, V> DashMapExt<K, V> for DashMap<K, V>
where
    K: Eq + Hash,
{
    fn get_cloned<Q>(&self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
        V: Clone,
    {
        self.get(key).map(|r| r.value().clone())
    }

    fn iter_cloned(&self) -> Vec<(K, V)>
    where
        K: Clone,
        V: Clone,
    {
        self.iter()
            .map(|e| (e.key().clone(), e.value().clone()))
            .collect()
    }
}
