// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! Kani-friendly replacements for `HashMap`, `HashSet`, `VecDeque`, and `Instant`.

mod instant;

pub use instant::VerifyInstant;

use std::borrow::Borrow;
use std::collections::BTreeMap;
use std::ops::RangeBounds;

/// `HashSet` replacement using `Vec`; requires `Ord` instead of `Hash + Eq`.
#[derive(Debug, Clone)]
pub struct VerifySet<T: Ord> {
    inner: Vec<T>,
}

impl<T: Ord> VerifySet<T> {
    /// Creates an empty set.
    pub fn new() -> Self {
        Self { inner: Vec::new() }
    }

    /// Creates an empty set with space for at least `capacity` elements.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Vec::with_capacity(capacity),
        }
    }
    /// Inserts a value into the set.
    pub fn insert(&mut self, value: T) -> bool {
        if self.contains(&value) {
            return false;
        }
        self.inner.push(value);
        true
    }
    /// Removes a value from the set.
    pub fn remove(&mut self, value: &T) -> bool {
        if let Some(idx) = self.inner.iter().position(|v| v == value) {
            self.inner.swap_remove(idx);
            return true;
        }
        false
    }
    /// Returns true if the set contains the value.
    pub fn contains(&self, value: &T) -> bool {
        self.inner.iter().any(|v| v == value)
    }
    /// Returns true if the set contains an element matching the borrowed value.
    /// Mirrors `HashSet::contains` which accepts borrowed keys via `Borrow`.
    pub fn contains_ref<Q>(&self, value: &Q) -> bool
    where
        T: Borrow<Q>,
        Q: Eq + ?Sized,
    {
        self.inner.iter().any(|v| v.borrow() == value)
    }
    /// Returns the number of elements in the set.
    pub fn len(&self) -> usize {
        self.inner.len()
    }
    /// Returns true if the set is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
    /// Clears the set.
    pub fn clear(&mut self) {
        self.inner.clear();
    }
    /// Returns true if self is a subset of other.
    pub fn is_subset(&self, other: &VerifySet<T>) -> bool {
        self.inner.iter().all(|v| other.contains(v))
    }
    /// Returns true if self is a superset of other.
    pub fn is_superset(&self, other: &VerifySet<T>) -> bool {
        other.is_subset(self)
    }
    /// Returns an iterator over the set.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.inner.iter()
    }
    /// Removes and returns elements in the requested range.
    pub fn drain<R>(&mut self, range: R) -> std::vec::Drain<'_, T>
    where
        R: RangeBounds<usize>,
    {
        self.inner.drain(range)
    }

    /// Removes and drops the first `count` elements, preserving insertion order
    /// of the remaining entries. Mirrors `OrderedSet::drain_front` used by
    /// `aterm-memory`'s eviction path. Saturates at `len()` so callers can
    /// pass counts larger than the current size without panicking.
    pub fn drain_front(&mut self, count: usize) {
        let n = count.min(self.inner.len());
        self.inner.drain(..n);
    }
}

impl<T: Ord> Default for VerifySet<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Ord> PartialEq for VerifySet<T> {
    fn eq(&self, other: &Self) -> bool {
        if self.len() != other.len() {
            return false;
        }
        // Sets are equal if they contain the same elements regardless of order.
        // Sort references to avoid cloning and compare.
        let mut self_sorted: Vec<_> = self.inner.iter().collect();
        let mut other_sorted: Vec<_> = other.inner.iter().collect();
        self_sorted.sort();
        other_sorted.sort();
        self_sorted == other_sorted
    }
}

impl<T: Ord> Eq for VerifySet<T> {}

impl<T: Ord> FromIterator<T> for VerifySet<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        // Build incrementally to avoid sort/balancing paths that can explode Kani.
        let mut set = Self::new();
        for item in iter {
            set.insert(item);
        }
        set
    }
}

impl<T: Ord> IntoIterator for VerifySet<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.into_iter()
    }
}

impl<'a, T: Ord> IntoIterator for &'a VerifySet<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter()
    }
}

/// A verification-friendly map that uses `BTreeMap` internally.
///
/// This type provides a `HashMap`-like API but uses a tree structure
/// that Kani can verify efficiently without loop explosion.
#[derive(Debug, Clone)]
pub struct VerifyMap<K: Ord, V> {
    inner: BTreeMap<K, V>,
}

impl<K: Ord, V> VerifyMap<K, V> {
    /// Creates an empty map.
    pub fn new() -> Self {
        Self {
            inner: BTreeMap::new(),
        }
    }

    /// Inserts a key-value pair into the map.
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        self.inner.insert(key, value)
    }

    /// Removes a key from the map.
    pub fn remove<Q>(&mut self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.inner.remove(key)
    }

    /// Returns a reference to the value for the key.
    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.inner.get(key)
    }

    /// Returns a mutable reference to the value for the key.
    pub fn get_mut<Q>(&mut self, key: &Q) -> Option<&mut V>
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.inner.get_mut(key)
    }

    /// Returns true if the map contains the key.
    pub fn contains_key<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.inner.contains_key(key)
    }

    /// Returns the number of elements in the map.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Returns true if the map is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Clears the map.
    pub fn clear(&mut self) {
        self.inner.clear();
    }

    /// Returns an iterator over the keys.
    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.inner.keys()
    }

    /// Returns an iterator over the values.
    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.inner.values()
    }

    /// Returns a mutable iterator over the values.
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.inner.values_mut()
    }

    /// Returns an iterator over the key-value pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.inner.iter()
    }

    /// Returns a mutable iterator over the key-value pairs.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&K, &mut V)> {
        self.inner.iter_mut()
    }

    /// Retains only the elements specified by the predicate.
    pub fn retain<F>(&mut self, f: F)
    where
        F: FnMut(&K, &mut V) -> bool,
    {
        self.inner.retain(f);
    }

    /// Gets the given key's corresponding entry in the map for in-place manipulation.
    pub fn entry(&mut self, key: K) -> std::collections::btree_map::Entry<'_, K, V> {
        self.inner.entry(key)
    }
}

impl<K: Ord, V> Default for VerifyMap<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Ord, V, Q: ?Sized> std::ops::Index<&Q> for VerifyMap<K, V>
where
    K: Borrow<Q>,
    Q: Ord,
{
    type Output = V;

    fn index(&self, key: &Q) -> &V {
        self.inner.index(key)
    }
}

impl<K: Ord, V: PartialEq> PartialEq for VerifyMap<K, V> {
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

impl<K: Ord, V: Eq> Eq for VerifyMap<K, V> {}

impl<K: Ord, V> FromIterator<(K, V)> for VerifyMap<K, V> {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        // Avoid BTreeMap's FromIterator fast-path, which sorts a Vec internally.
        // The sort path tends to cause significant state explosion under Kani.
        let mut map = Self::new();
        for (key, value) in iter {
            map.insert(key, value);
        }
        map
    }
}

impl<K: Ord, V> IntoIterator for VerifyMap<K, V> {
    type Item = (K, V);
    type IntoIter = std::collections::btree_map::IntoIter<K, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.into_iter()
    }
}

impl<'a, K: Ord, V> IntoIterator for &'a VerifyMap<K, V> {
    type Item = (&'a K, &'a V);
    type IntoIter = std::collections::btree_map::Iter<'a, K, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter()
    }
}

impl<'a, K: Ord, V> IntoIterator for &'a mut VerifyMap<K, V> {
    type Item = (&'a K, &'a mut V);
    type IntoIter = std::collections::btree_map::IterMut<'a, K, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter_mut()
    }
}

/// A verification-friendly deque for Kani proofs.
///
/// `VecDeque`'s internal ring-buffer logic can cause CBMC/Kani to spend a lot of
/// time on pointer arithmetic and wrap-around cases. For bounded audit logs used
/// in proofs, a simple `Vec` with FIFO semantics is sufficient and tends to
/// verify much faster.
#[derive(Debug, Clone)]
pub struct VerifyDeque<T> {
    inner: Vec<T>,
}

impl<T> VerifyDeque<T> {
    /// Creates an empty deque.
    pub fn new() -> Self {
        Self { inner: Vec::new() }
    }

    /// Appends an element to the back of the deque.
    pub fn push_back(&mut self, value: T) {
        self.inner.push(value);
    }

    /// Removes and returns the element from the front of the deque.
    pub fn pop_front(&mut self) -> Option<T> {
        if self.inner.is_empty() {
            None
        } else {
            Some(self.inner.remove(0))
        }
    }

    /// Returns the number of elements in the deque.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Returns true if the deque contains no elements.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Clears the deque, removing all values.
    pub fn clear(&mut self) {
        self.inner.clear();
    }

    /// Returns an iterator over the deque's contents.
    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.inner.iter()
    }

    /// Returns a reference to the back (last) element, or `None` if empty.
    pub fn back(&self) -> Option<&T> {
        self.inner.last()
    }
}

impl<T> Default for VerifyDeque<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a, T> IntoIterator for &'a VerifyDeque<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter()
    }
}

#[cfg(test)]
#[path = "../stubs_tests.rs"]
mod tests;
