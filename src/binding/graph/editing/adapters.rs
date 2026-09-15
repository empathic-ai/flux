//! Collection identity and presentation adapters.
use super::*;
use bevy::reflect::{GetTypeRegistration, Typed};
use std::{hash::Hash, marker::PhantomData};

/// Implement this trait for custom projections retaining original keys. Replacement
/// must address the original collection, preserve membership, and reject missing keys.
/// Validation should be pure; no I/O belongs in a collection adapter.
pub trait EditCollection: Send + Sync + 'static {
    type Collection: Reflect + FromReflect + Clone + PartialEq;
    type Item: Reflect + FromReflect + Clone + PartialEq;
    type Key: Clone + Eq + Hash + Send + Sync + 'static;
    fn entries(&self, collection: &Self::Collection) -> Vec<(Self::Key, Self::Item)>;
    fn replace(
        &self,
        collection: &mut Self::Collection,
        key: &Self::Key,
        value: Self::Item,
    ) -> std::result::Result<(), EditError>;
    /// Resolve an original item, even when a presentation filter hides its row.
    fn get(&self, collection: &Self::Collection, key: &Self::Key) -> Option<Self::Item> {
        self.entries(collection)
            .into_iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
    }
    fn filter<F: Fn(&Self::Item) -> bool + Send + Sync + 'static>(
        self,
        predicate: F,
    ) -> FilteredEntries<Self, F>
    where
        Self: Sized,
    {
        FilteredEntries {
            inner: self,
            predicate,
        }
    }
    fn sort_by<F: Fn(&Self::Item, &Self::Item) -> std::cmp::Ordering + Send + Sync + 'static>(
        self,
        compare: F,
    ) -> SortedEntries<Self, F>
    where
        Self: Sized,
    {
        SortedEntries {
            inner: self,
            compare,
        }
    }
    fn guard_collection(&self) -> bool {
        false
    }
    /// Unordered collections retain existing row order and append new keys.
    fn ordered(&self) -> bool {
        true
    }
}

/// Presentation filtering retains source keys and original lookup for active drafts.
pub struct FilteredEntries<A, F> {
    inner: A,
    predicate: F,
}
impl<A: EditCollection, F: Fn(&A::Item) -> bool + Send + Sync + 'static> EditCollection
    for FilteredEntries<A, F>
{
    type Collection = A::Collection;
    type Item = A::Item;
    type Key = A::Key;
    fn entries(&self, c: &Self::Collection) -> Vec<(Self::Key, Self::Item)> {
        self.inner
            .entries(c)
            .into_iter()
            .filter(|(_, v)| (self.predicate)(v))
            .collect()
    }
    fn get(&self, c: &Self::Collection, k: &Self::Key) -> Option<Self::Item> {
        self.inner.get(c, k)
    }
    fn replace(
        &self,
        c: &mut Self::Collection,
        k: &Self::Key,
        v: Self::Item,
    ) -> std::result::Result<(), EditError> {
        self.inner.replace(c, k, v)
    }
    fn guard_collection(&self) -> bool {
        self.inner.guard_collection()
    }
    fn ordered(&self) -> bool {
        self.inner.ordered()
    }
}
/// Presentation sorting never changes the address used for item write-back.
pub struct SortedEntries<A, F> {
    inner: A,
    compare: F,
}
impl<A: EditCollection, F: Fn(&A::Item, &A::Item) -> std::cmp::Ordering + Send + Sync + 'static>
    EditCollection for SortedEntries<A, F>
{
    type Collection = A::Collection;
    type Item = A::Item;
    type Key = A::Key;
    fn entries(&self, c: &Self::Collection) -> Vec<(Self::Key, Self::Item)> {
        let mut entries = self.inner.entries(c);
        entries.sort_by(|(_, a), (_, b)| (self.compare)(a, b));
        entries
    }
    fn get(&self, c: &Self::Collection, k: &Self::Key) -> Option<Self::Item> {
        self.inner.get(c, k)
    }
    fn replace(
        &self,
        c: &mut Self::Collection,
        k: &Self::Key,
        v: Self::Item,
    ) -> std::result::Result<(), EditError> {
        self.inner.replace(c, k, v)
    }
    fn guard_collection(&self) -> bool {
        self.inner.guard_collection()
    }
}

/// Positional identity with a conservative whole-collection snapshot guard.
/// Does not claim to detect remove/reinsert histories with identical final values.
pub struct IndexedVec<T>(PhantomData<T>);
impl<T> Default for IndexedVec<T> {
    fn default() -> Self {
        Self(PhantomData)
    }
}
impl<T: Reflect + FromReflect + Typed + GetTypeRegistration + Clone + PartialEq> EditCollection
    for IndexedVec<T>
{
    type Collection = Vec<T>;
    type Item = T;
    type Key = usize;
    fn entries(&self, c: &Vec<T>) -> Vec<(usize, T)> {
        c.iter().cloned().enumerate().collect()
    }
    fn replace(&self, c: &mut Vec<T>, k: &usize, v: T) -> std::result::Result<(), EditError> {
        *c.get_mut(*k).ok_or(EditError::Missing)? = v;
        Ok(())
    }
    fn guard_collection(&self) -> bool {
        true
    }
}

pub struct KeyedVec<T, K, F> {
    key: F,
    marker: PhantomData<(T, K)>,
}
impl<T, K, F> KeyedVec<T, K, F> {
    pub fn new(key: F) -> Self {
        Self {
            key,
            marker: PhantomData,
        }
    }
}
impl<T, K, F> EditCollection for KeyedVec<T, K, F>
where
    T: Reflect + FromReflect + Typed + GetTypeRegistration + Clone + PartialEq,
    K: Clone + Eq + Hash + Send + Sync + 'static,
    F: Fn(&T) -> K + Send + Sync + 'static,
{
    type Collection = Vec<T>;
    type Item = T;
    type Key = K;
    fn entries(&self, c: &Vec<T>) -> Vec<(K, T)> {
        c.iter().map(|v| ((self.key)(v), v.clone())).collect()
    }
    fn replace(&self, c: &mut Vec<T>, k: &K, v: T) -> std::result::Result<(), EditError> {
        if (self.key)(&v) != *k {
            return Err(EditError::Invalid);
        }
        let mut items = c.iter_mut().filter(|v| (self.key)(v) == *k);
        let item = items.next().ok_or(EditError::Missing)?;
        if items.next().is_some() {
            return Err(EditError::DuplicateKey);
        }
        *item = v;
        Ok(())
    }
}

/// Map keys are occurrence identities; the item editor receives only the value.
pub struct MapEntries<K, V>(PhantomData<(K, V)>);
impl<K, V> Default for MapEntries<K, V> {
    fn default() -> Self {
        Self(PhantomData)
    }
}
impl<K, V> EditCollection for MapEntries<K, V>
where
    K: Reflect + FromReflect + Typed + GetTypeRegistration + Clone + Eq + Hash,
    V: Reflect + FromReflect + Typed + GetTypeRegistration + Clone + PartialEq,
{
    type Collection = HashMap<K, V>;
    type Item = V;
    type Key = K;
    fn ordered(&self) -> bool {
        false
    }
    fn entries(&self, c: &HashMap<K, V>) -> Vec<(K, V)> {
        c.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }
    fn replace(&self, c: &mut HashMap<K, V>, k: &K, v: V) -> std::result::Result<(), EditError> {
        *c.get_mut(k).ok_or(EditError::Missing)? = v;
        Ok(())
    }
}

/// The same lifecycle for a scalar, struct, or complete form at a single path.
pub struct EditValue<T>(PhantomData<T>);
impl<T> Default for EditValue<T> {
    fn default() -> Self {
        Self(PhantomData)
    }
}
impl<T: Reflect + FromReflect + Clone + PartialEq> EditCollection for EditValue<T> {
    type Collection = T;
    type Item = T;
    type Key = ();
    fn entries(&self, c: &T) -> Vec<((), T)> {
        vec![((), c.clone())]
    }
    fn replace(&self, c: &mut T, _: &(), v: T) -> std::result::Result<(), EditError> {
        *c = v;
        Ok(())
    }
}
