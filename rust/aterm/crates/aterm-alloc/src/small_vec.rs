// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! `SmallVec<T, N>`: inline storage with heap fallback.

use std::fmt;
use std::mem::MaybeUninit;
use std::ops::{Deref, DerefMut};

/// A vector that stores up to `N` elements inline before spilling to the heap.
///
/// This avoids heap allocation for the common case where collections are small.
/// When the inline capacity is exceeded, all elements move to a heap-allocated `Vec<T>`.
pub struct SmallVec<T, const N: usize> {
    data: SmallVecData<T, N>,
}

enum SmallVecData<T, const N: usize> {
    Inline {
        buf: [MaybeUninit<T>; N],
        len: usize,
    },
    Heap(Vec<T>),
}

impl<T, const N: usize> SmallVec<T, N> {
    /// Create a new, empty `SmallVec`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            data: SmallVecData::Inline {
                // SAFETY: MaybeUninit array does not require initialization
                buf: unsafe { MaybeUninit::uninit().assume_init() },
                len: 0,
            },
        }
    }

    /// Create a new, empty `SmallVec` (const-compatible alias for `new`).
    #[must_use]
    pub const fn new_const() -> Self {
        Self::new()
    }

    /// Create a `SmallVec` with the given capacity pre-allocated.
    ///
    /// If `capacity <= N`, uses inline storage. Otherwise, allocates on the heap.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        if capacity <= N {
            Self::new()
        } else {
            Self {
                data: SmallVecData::Heap(Vec::with_capacity(capacity)),
            }
        }
    }

    /// The number of elements.
    #[must_use]
    pub fn len(&self) -> usize {
        match &self.data {
            SmallVecData::Inline { len, .. } => *len,
            SmallVecData::Heap(vec) => vec.len(),
        }
    }

    /// Whether the collection is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The current capacity (inline or heap).
    #[must_use]
    pub fn capacity(&self) -> usize {
        match &self.data {
            SmallVecData::Inline { .. } => N,
            SmallVecData::Heap(vec) => vec.capacity(),
        }
    }

    /// Whether the data is currently stored inline.
    #[must_use]
    pub fn is_inline(&self) -> bool {
        matches!(&self.data, SmallVecData::Inline { .. })
    }

    /// Whether the data has spilled to the heap.
    ///
    /// This is the inverse of [`is_inline`](Self::is_inline).
    /// Provided for API compatibility with `smallvec::SmallVec::spilled()`.
    #[must_use]
    pub fn spilled(&self) -> bool {
        !self.is_inline()
    }

    /// Push an element. Spills to heap if inline capacity is exceeded.
    pub fn push(&mut self, value: T) {
        match &mut self.data {
            SmallVecData::Inline { buf, len } => {
                if *len < N {
                    buf[*len] = MaybeUninit::new(value);
                    *len += 1;
                } else {
                    // Spill to heap
                    self.spill_and_push(value);
                }
            }
            SmallVecData::Heap(vec) => {
                vec.push(value);
            }
        }
    }

    /// Pop the last element.
    pub fn pop(&mut self) -> Option<T> {
        match &mut self.data {
            SmallVecData::Inline { buf, len } => {
                if *len == 0 {
                    None
                } else {
                    *len -= 1;
                    // SAFETY: buf[*len] was initialized when it was pushed
                    Some(unsafe { buf[*len].assume_init_read() })
                }
            }
            SmallVecData::Heap(vec) => vec.pop(),
        }
    }

    /// Clear all elements.
    pub fn clear(&mut self) {
        match &mut self.data {
            SmallVecData::Inline { buf, len } => {
                // SAFETY: elements 0..*len are initialized
                for elem in &mut buf[..*len] {
                    unsafe {
                        elem.assume_init_drop();
                    }
                }
                *len = 0;
            }
            SmallVecData::Heap(vec) => vec.clear(),
        }
    }

    /// Insert an element at the given index.
    ///
    /// # Panics
    ///
    /// Panics if `index > len`.
    pub fn insert(&mut self, index: usize, value: T) {
        let len = self.len();
        assert!(index <= len, "index out of bounds: {index} > {len}");

        match &mut self.data {
            SmallVecData::Inline {
                buf,
                len: inline_len,
            } if *inline_len < N => {
                // Shift elements right
                // SAFETY: we have room and all elements in 0..inline_len are init
                unsafe {
                    let ptr = buf.as_mut_ptr().cast::<T>();
                    std::ptr::copy(ptr.add(index), ptr.add(index + 1), *inline_len - index);
                    std::ptr::write(ptr.add(index), value);
                }
                *inline_len += 1;
            }
            _ => {
                // Either inline-full or already on heap: ensure heap
                self.ensure_heap();
                if let SmallVecData::Heap(vec) = &mut self.data {
                    vec.insert(index, value);
                }
            }
        }
    }

    /// Remove and return the element at the given index.
    ///
    /// # Panics
    ///
    /// Panics if `index >= len`.
    pub fn remove(&mut self, index: usize) -> T {
        let len = self.len();
        assert!(index < len, "index out of bounds: {index} >= {len}");

        match &mut self.data {
            SmallVecData::Inline {
                buf,
                len: inline_len,
            } => {
                // SAFETY: element at index is initialized, and we shift remaining left
                unsafe {
                    let ptr = buf.as_mut_ptr().cast::<T>();
                    let value = std::ptr::read(ptr.add(index));
                    std::ptr::copy(ptr.add(index + 1), ptr.add(index), *inline_len - index - 1);
                    *inline_len -= 1;
                    value
                }
            }
            SmallVecData::Heap(vec) => vec.remove(index),
        }
    }

    /// Remove the element at `index` by swapping it with the last element.
    ///
    /// This is O(1) but does not preserve ordering.
    ///
    /// # Panics
    ///
    /// Panics if `index >= len`.
    pub fn swap_remove(&mut self, index: usize) -> T {
        let len = self.len();
        assert!(index < len, "index out of bounds: {index} >= {len}");
        let last = len - 1;
        self.as_mut_slice().swap(index, last);
        self.pop().expect("invariant: len > 0 after swap")
    }

    /// Truncate to the given length, dropping excess elements.
    pub fn truncate(&mut self, new_len: usize) {
        match &mut self.data {
            SmallVecData::Inline { buf, len } => {
                if new_len < *len {
                    // SAFETY: elements new_len..*len are initialized
                    for elem in &mut buf[new_len..*len] {
                        unsafe {
                            elem.assume_init_drop();
                        }
                    }
                    *len = new_len;
                }
            }
            SmallVecData::Heap(vec) => vec.truncate(new_len),
        }
    }

    /// Extend from a slice (requires `T: Clone`).
    pub fn extend_from_slice(&mut self, slice: &[T])
    where
        T: Clone,
    {
        for item in slice {
            self.push(item.clone());
        }
    }

    /// View as a slice.
    #[must_use]
    pub fn as_slice(&self) -> &[T] {
        match &self.data {
            SmallVecData::Inline { buf, len } => {
                // SAFETY: elements 0..*len are initialized
                unsafe { std::slice::from_raw_parts(buf.as_ptr().cast::<T>(), *len) }
            }
            SmallVecData::Heap(vec) => vec.as_slice(),
        }
    }

    /// View as a mutable slice.
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        match &mut self.data {
            SmallVecData::Inline { buf, len } => {
                // SAFETY: elements 0..*len are initialized
                unsafe { std::slice::from_raw_parts_mut(buf.as_mut_ptr().cast::<T>(), *len) }
            }
            SmallVecData::Heap(vec) => vec.as_mut_slice(),
        }
    }

    /// Create a `SmallVec` from a slice (requires `T: Clone`).
    #[must_use]
    pub fn from_slice(slice: &[T]) -> Self
    where
        T: Clone,
    {
        let mut sv = Self::with_capacity(slice.len());
        sv.extend_from_slice(slice);
        sv
    }

    /// Create from a `Vec<T>`.
    #[must_use]
    pub fn from_vec(vec: Vec<T>) -> Self {
        if vec.len() <= N {
            let mut sv = Self::new();
            for item in vec {
                sv.push(item);
            }
            sv
        } else {
            Self {
                data: SmallVecData::Heap(vec),
            }
        }
    }

    /// Convert into a `Vec<T>`.
    pub fn into_vec(mut self) -> Vec<T> {
        match &mut self.data {
            SmallVecData::Inline { buf, len } => {
                let current_len = *len;
                let mut vec = Vec::with_capacity(current_len);
                // SAFETY: elements 0..current_len are initialized
                for elem in &buf[..current_len] {
                    vec.push(unsafe { elem.assume_init_read() });
                }
                // Prevent double-drop: zero the length so Drop does nothing
                *len = 0;
                vec
            }
            SmallVecData::Heap(vec) => {
                // Take the vec out, leave an empty one in its place
                std::mem::take(vec)
            }
        }
    }

    /// Create from a single element repeated `n` times.
    pub fn from_elem(value: T, n: usize) -> Self
    where
        T: Clone,
    {
        let mut sv = Self::with_capacity(n);
        for _ in 0..n.saturating_sub(1) {
            sv.push(value.clone());
        }
        if n > 0 {
            sv.push(value);
        }
        sv
    }

    /// An iterator over references to elements.
    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.as_slice().iter()
    }

    /// An iterator over mutable references to elements.
    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, T> {
        self.as_mut_slice().iter_mut()
    }

    /// Retain only elements where the predicate returns true.
    ///
    /// Panic-safe: if the predicate panics, all elements are in a valid state.
    /// Works in both inline and heap modes.
    pub fn retain<F: FnMut(&T) -> bool>(&mut self, f: F) {
        match &mut self.data {
            SmallVecData::Heap(vec) => vec.retain(f),
            SmallVecData::Inline { buf, len } => {
                retain_inline(buf, len, f);
            }
        }
    }

    // ── Internal helpers ────────────────────────────────────────────────

    fn spill_and_push(&mut self, value: T) {
        self.ensure_heap();
        if let SmallVecData::Heap(vec) = &mut self.data {
            vec.push(value);
        }
    }

    fn ensure_heap(&mut self) {
        if let SmallVecData::Inline { buf, len } = &mut self.data {
            let current_len = *len;
            let mut vec = Vec::with_capacity(current_len.max(N) * 2);
            // SAFETY: elements 0..current_len are initialized
            for elem in &buf[..current_len] {
                vec.push(unsafe { elem.assume_init_read() });
            }
            // Prevent double-drop of inline elements
            *len = 0;
            self.data = SmallVecData::Heap(vec);
        }
    }
}

// ── Inline retain with drop guard ──────────────────────────────────────────

fn retain_inline<T, const N: usize>(
    buf: &mut [MaybeUninit<T>; N],
    len: &mut usize,
    mut f: impl FnMut(&T) -> bool,
) {
    let original_len = *len;
    *len = 0;

    struct RetainGuard<'a, T, const N: usize> {
        buf: &'a mut [MaybeUninit<T>; N],
        len: &'a mut usize,
        write: usize,
        read: usize,
        original_len: usize,
    }

    impl<T, const N: usize> Drop for RetainGuard<'_, T, N> {
        fn drop(&mut self) {
            // SAFETY: elements read..original_len have NOT been processed — drop them.
            unsafe {
                for i in self.read..self.original_len {
                    self.buf[i].assume_init_drop();
                }
            }
            *self.len = self.write;
        }
    }

    let mut guard = RetainGuard {
        buf,
        len,
        write: 0,
        read: 0,
        original_len,
    };

    while guard.read < original_len {
        let read = guard.read;
        // SAFETY: element at `read` is initialized (read < original_len)
        let keep = unsafe { f(&*guard.buf[read].as_ptr()) };
        guard.read += 1;
        if keep {
            if guard.write != read {
                // SAFETY: both indices in bounds; read element consumed, write slot empty
                unsafe {
                    let val = guard.buf[read].assume_init_read();
                    guard.buf[guard.write] = MaybeUninit::new(val);
                }
            }
            guard.write += 1;
        } else {
            // SAFETY: element is initialized; drop it
            unsafe {
                guard.buf[read].assume_init_drop();
            }
        }
    }

    guard.original_len = guard.read;
    drop(guard);
}

// ── Trait impls ─────────────────────────────────────────────────────────────

impl<T, const N: usize> Default for SmallVec<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, const N: usize> Deref for SmallVec<T, N> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        self.as_slice()
    }
}

impl<T, const N: usize> DerefMut for SmallVec<T, N> {
    fn deref_mut(&mut self) -> &mut [T] {
        self.as_mut_slice()
    }
}

impl<T: Clone, const N: usize> Clone for SmallVec<T, N> {
    fn clone(&self) -> Self {
        let mut new = Self::with_capacity(self.len());
        for item in self.as_slice() {
            new.push(item.clone());
        }
        new
    }
}

impl<T: fmt::Debug, const N: usize> fmt::Debug for SmallVec<T, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.as_slice()).finish()
    }
}

impl<T: PartialEq, const N: usize> PartialEq for SmallVec<T, N> {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<T: Eq, const N: usize> Eq for SmallVec<T, N> {}

impl<T, const N: usize> Drop for SmallVec<T, N> {
    fn drop(&mut self) {
        if let SmallVecData::Inline { buf, len } = &mut self.data {
            // SAFETY: elements 0..*len are initialized
            for elem in &mut buf[..*len] {
                unsafe {
                    elem.assume_init_drop();
                }
            }
        }
        // Heap variant: Vec drop handles it
    }
}

impl<T, const N: usize> FromIterator<T> for SmallVec<T, N> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let iter = iter.into_iter();
        let (lower, _) = iter.size_hint();
        let mut sv = Self::with_capacity(lower);
        for item in iter {
            sv.push(item);
        }
        sv
    }
}

impl<T, const N: usize> Extend<T> for SmallVec<T, N> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        for item in iter {
            self.push(item);
        }
    }
}

impl<T, const N: usize> IntoIterator for SmallVec<T, N> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.into_vec().into_iter()
    }
}

impl<'a, T, const N: usize> IntoIterator for &'a SmallVec<T, N> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.as_slice().iter()
    }
}

impl<'a, T, const N: usize> IntoIterator for &'a mut SmallVec<T, N> {
    type Item = &'a mut T;
    type IntoIter = std::slice::IterMut<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.as_mut_slice().iter_mut()
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_is_empty() {
        let sv: SmallVec<i32, 4> = SmallVec::new();
        assert!(sv.is_empty());
        assert_eq!(sv.len(), 0);
        assert!(sv.is_inline());
    }

    #[test]
    fn test_push_within_inline_capacity() {
        let mut sv: SmallVec<i32, 4> = SmallVec::new();
        sv.push(1);
        sv.push(2);
        sv.push(3);
        assert_eq!(sv.len(), 3);
        assert!(sv.is_inline());
        assert_eq!(sv.as_slice(), &[1, 2, 3]);
    }

    #[test]
    fn test_push_spills_to_heap() {
        let mut sv: SmallVec<i32, 2> = SmallVec::new();
        sv.push(1);
        sv.push(2);
        assert!(sv.is_inline());
        sv.push(3);
        assert!(!sv.is_inline());
        assert_eq!(sv.as_slice(), &[1, 2, 3]);
    }

    #[test]
    fn test_pop() {
        let mut sv: SmallVec<i32, 4> = SmallVec::new();
        sv.push(10);
        sv.push(20);
        assert_eq!(sv.pop(), Some(20));
        assert_eq!(sv.pop(), Some(10));
        assert_eq!(sv.pop(), None);
    }

    #[test]
    fn test_clear() {
        let mut sv: SmallVec<String, 2> = SmallVec::new();
        sv.push("a".into());
        sv.push("b".into());
        sv.clear();
        assert!(sv.is_empty());
    }

    #[test]
    fn test_insert_and_remove() {
        let mut sv: SmallVec<i32, 4> = SmallVec::new();
        sv.push(1);
        sv.push(3);
        sv.insert(1, 2);
        assert_eq!(sv.as_slice(), &[1, 2, 3]);

        let removed = sv.remove(1);
        assert_eq!(removed, 2);
        assert_eq!(sv.as_slice(), &[1, 3]);
    }

    #[test]
    fn test_truncate() {
        let mut sv: SmallVec<i32, 4> = SmallVec::new();
        sv.push(1);
        sv.push(2);
        sv.push(3);
        sv.truncate(1);
        assert_eq!(sv.as_slice(), &[1]);
    }

    #[test]
    fn test_from_vec() {
        let sv: SmallVec<i32, 4> = SmallVec::from_vec(vec![1, 2, 3]);
        assert!(sv.is_inline());
        assert_eq!(sv.as_slice(), &[1, 2, 3]);

        let sv: SmallVec<i32, 2> = SmallVec::from_vec(vec![1, 2, 3, 4, 5]);
        assert!(!sv.is_inline());
        assert_eq!(sv.as_slice(), &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_into_vec() {
        let mut sv: SmallVec<i32, 4> = SmallVec::new();
        sv.push(1);
        sv.push(2);
        let vec = sv.into_vec();
        assert_eq!(vec, vec![1, 2]);
    }

    #[test]
    fn test_from_elem() {
        let sv: SmallVec<char, 4> = SmallVec::from_elem('x', 3);
        assert_eq!(sv.as_slice(), &['x', 'x', 'x']);
    }

    #[test]
    fn test_clone() {
        let mut sv: SmallVec<String, 2> = SmallVec::new();
        sv.push("hello".into());
        let cloned = sv.clone();
        assert_eq!(sv.as_slice(), cloned.as_slice());
    }

    #[test]
    fn test_eq() {
        let mut a: SmallVec<i32, 4> = SmallVec::new();
        a.push(1);
        a.push(2);
        let mut b: SmallVec<i32, 4> = SmallVec::new();
        b.push(1);
        b.push(2);
        assert_eq!(a, b);
    }

    #[test]
    fn test_collect() {
        let sv: SmallVec<i32, 4> = (0..3).collect();
        assert_eq!(sv.as_slice(), &[0, 1, 2]);
    }

    #[test]
    fn test_deref_indexing() {
        let mut sv: SmallVec<i32, 4> = SmallVec::new();
        sv.push(10);
        sv.push(20);
        assert_eq!(sv[0], 10);
        assert_eq!(sv[1], 20);
    }

    #[test]
    fn test_extend_from_slice() {
        let mut sv: SmallVec<i32, 4> = SmallVec::new();
        sv.extend_from_slice(&[1, 2, 3]);
        assert_eq!(sv.as_slice(), &[1, 2, 3]);
    }

    #[test]
    fn test_with_capacity() {
        let sv: SmallVec<i32, 4> = SmallVec::with_capacity(2);
        assert!(sv.is_inline());
        assert_eq!(sv.capacity(), 4); // inline capacity is always N

        let sv: SmallVec<i32, 4> = SmallVec::with_capacity(10);
        assert!(!sv.is_inline());
        assert!(sv.capacity() >= 10);
    }

    #[test]
    fn test_debug_format() {
        let mut sv: SmallVec<i32, 4> = SmallVec::new();
        sv.push(1);
        sv.push(2);
        assert_eq!(format!("{sv:?}"), "[1, 2]");
    }

    #[test]
    fn test_into_iter() {
        let mut sv: SmallVec<i32, 4> = SmallVec::new();
        sv.push(10);
        sv.push(20);
        sv.push(30);
        let collected: Vec<i32> = sv.into_iter().collect();
        assert_eq!(collected, vec![10, 20, 30]);
    }

    #[test]
    fn test_drop_string_elements() {
        // Verify that String elements are properly dropped (no leaks).
        let mut sv: SmallVec<String, 2> = SmallVec::new();
        sv.push("hello world this is a longer string to force heap alloc".into());
        sv.push("another string".into());
        drop(sv);
        // If we get here without ASAN/MIRI complaint, drop is correct.
    }

    #[test]
    fn test_insert_at_end() {
        let mut sv: SmallVec<i32, 4> = SmallVec::new();
        sv.push(1);
        sv.insert(1, 2);
        assert_eq!(sv.as_slice(), &[1, 2]);
    }

    #[test]
    fn test_insert_at_beginning() {
        let mut sv: SmallVec<i32, 4> = SmallVec::new();
        sv.push(2);
        sv.push(3);
        sv.insert(0, 1);
        assert_eq!(sv.as_slice(), &[1, 2, 3]);
    }

    #[test]
    #[should_panic(expected = "index out of bounds")]
    fn test_insert_out_of_bounds_panics() {
        let mut sv: SmallVec<i32, 4> = SmallVec::new();
        sv.insert(1, 42);
    }

    #[test]
    #[should_panic(expected = "index out of bounds")]
    fn test_remove_out_of_bounds_panics() {
        let mut sv: SmallVec<i32, 4> = SmallVec::new();
        sv.remove(0);
    }

    #[test]
    fn test_retain_inline() {
        let mut sv: SmallVec<i32, 8> = SmallVec::new();
        sv.push(1);
        sv.push(2);
        sv.push(3);
        sv.push(4);
        sv.push(5);
        assert!(sv.is_inline());
        sv.retain(|x| x % 2 == 0);
        assert_eq!(sv.as_slice(), &[2, 4]);
        assert!(sv.is_inline());
    }

    #[test]
    fn test_retain_heap() {
        let mut sv: SmallVec<i32, 2> = SmallVec::new();
        sv.push(1);
        sv.push(2);
        sv.push(3);
        sv.push(4);
        sv.push(5);
        assert!(!sv.is_inline());
        sv.retain(|x| x % 2 == 0);
        assert_eq!(sv.as_slice(), &[2, 4]);
    }

    #[test]
    fn test_retain_inline_with_drop_types() {
        let mut sv: SmallVec<String, 8> = SmallVec::new();
        sv.push("keep-a".into());
        sv.push("drop-b".into());
        sv.push("keep-c".into());
        sv.push("drop-d".into());
        sv.push("keep-e".into());
        assert!(sv.is_inline());
        sv.retain(|s| s.starts_with("keep"));
        assert_eq!(sv.as_slice(), &["keep-a", "keep-c", "keep-e"]);
    }

    #[test]
    fn test_retain_inline_panic_safety() {
        use std::panic;
        use std::sync::atomic::{AtomicUsize, Ordering};

        static DROP_COUNT: AtomicUsize = AtomicUsize::new(0);

        #[derive(Debug)]
        struct Tracked(#[allow(dead_code)] i32);

        impl Drop for Tracked {
            fn drop(&mut self) {
                DROP_COUNT.fetch_add(1, Ordering::Relaxed);
            }
        }

        DROP_COUNT.store(0, Ordering::Relaxed);

        let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            let mut sv: SmallVec<Tracked, 8> = SmallVec::new();
            sv.push(Tracked(1));
            sv.push(Tracked(2));
            sv.push(Tracked(3));
            sv.push(Tracked(4));
            sv.push(Tracked(5));

            let mut call_count = 0;
            sv.retain(|_| {
                call_count += 1;
                if call_count == 3 {
                    panic!("predicate panic");
                }
                true
            });
        }));

        assert!(result.is_err());
        assert_eq!(DROP_COUNT.load(Ordering::Relaxed), 5);
    }

    #[test]
    fn test_retain_all() {
        let mut sv: SmallVec<i32, 4> = SmallVec::new();
        sv.push(1);
        sv.push(2);
        sv.push(3);
        sv.retain(|_| true);
        assert_eq!(sv.as_slice(), &[1, 2, 3]);
    }

    #[test]
    fn test_retain_none() {
        let mut sv: SmallVec<i32, 4> = SmallVec::new();
        sv.push(1);
        sv.push(2);
        sv.push(3);
        sv.retain(|_| false);
        assert!(sv.is_empty());
    }
}

// ── Kani proofs ────────────────────────────────────────────────────────────

#[cfg(kani)]
mod kani_proofs {
    use super::*;

    /// Verify that pushing past inline capacity spills to heap while
    /// preserving all previously pushed elements.
    ///
    /// Fills inline capacity (4 elements), then pushes one more to trigger
    /// the spill. Verifies that after spill: (1) storage mode changed to
    /// heap, (2) all 5 elements are present and in correct order.
    #[kani::proof]
    #[kani::unwind(7)]
    fn smallvec_push_spill_preserves_elements() {
        let a: u32 = kani::any();
        let b: u32 = kani::any();
        let c: u32 = kani::any();
        let d: u32 = kani::any();
        let e: u32 = kani::any();

        let mut sv: SmallVec<u32, 4> = SmallVec::new();
        sv.push(a);
        sv.push(b);
        sv.push(c);
        sv.push(d);

        // Still inline at capacity.
        kani::assert(sv.is_inline(), "must be inline at capacity N");
        kani::assert(sv.len() == 4, "len must be 4 before spill");

        // Push the 5th element — triggers spill.
        sv.push(e);

        kani::assert(
            !sv.is_inline(),
            "must be on heap after exceeding inline capacity",
        );
        kani::assert(sv.len() == 5, "len must be 5 after spill push");

        // All elements must be preserved in push order.
        let slice = sv.as_slice();
        kani::assert(slice[0] == a, "element 0 must survive spill");
        kani::assert(slice[1] == b, "element 1 must survive spill");
        kani::assert(slice[2] == c, "element 2 must survive spill");
        kani::assert(slice[3] == d, "element 3 must survive spill");
        kani::assert(slice[4] == e, "element 4 (post-spill push) must be correct");
    }

    /// Verify that insert and remove preserve element ordering.
    ///
    /// Starts with [a, b, c], inserts d at a symbolic index, verifies the
    /// resulting order, then removes from a symbolic index and verifies
    /// the removed value and remaining order.
    #[kani::proof]
    #[kani::unwind(7)]
    fn smallvec_insert_remove_ordering() {
        let a: u32 = kani::any();
        let b: u32 = kani::any();
        let c: u32 = kani::any();
        let d: u32 = kani::any();

        // Use distinct symbolic values to make ordering verifiable.
        kani::assume(a != b && a != c && a != d);
        kani::assume(b != c && b != d);
        kani::assume(c != d);

        let mut sv: SmallVec<u32, 8> = SmallVec::new();
        sv.push(a);
        sv.push(b);
        sv.push(c);

        // Insert d at symbolic position (0..=3 is valid).
        let ins_idx: usize = kani::any();
        kani::assume(ins_idx <= 3);

        sv.insert(ins_idx, d);
        kani::assert(sv.len() == 4, "len must be 4 after insert");

        // Verify d is at the inserted position.
        kani::assert(
            sv.as_slice()[ins_idx] == d,
            "inserted element must be at the specified index",
        );

        // Verify that elements before the insert point are unchanged.
        let original = [a, b, c];
        let mut orig_i = 0;
        let mut sv_i = 0;
        while sv_i < 4 {
            if sv_i == ins_idx {
                // This is where d was inserted; skip it.
                sv_i += 1;
                continue;
            }
            kani::assert(
                sv.as_slice()[sv_i] == original[orig_i],
                "non-inserted elements must maintain relative order",
            );
            orig_i += 1;
            sv_i += 1;
        }

        // Now remove at the insert index — should get d back.
        let removed = sv.remove(ins_idx);
        kani::assert(removed == d, "remove must return the inserted element");
        kani::assert(sv.len() == 3, "len must be 3 after remove");

        // Original elements restored.
        kani::assert(sv.as_slice()[0] == a, "element 0 must be a after remove");
        kani::assert(sv.as_slice()[1] == b, "element 1 must be b after remove");
        kani::assert(sv.as_slice()[2] == c, "element 2 must be c after remove");
    }

    /// Verify the retain drop guard on inline storage: after retain,
    /// len equals the count of elements satisfying the predicate, and
    /// every surviving element actually satisfies it.
    ///
    /// Uses a symbolic threshold to partition [0,1,2,3] into keep/discard
    /// sets, then checks the invariant.
    #[kani::proof]
    #[kani::unwind(6)]
    fn smallvec_retain_len_invariant() {
        let threshold: u32 = kani::any();
        kani::assume(threshold <= 4);

        let mut sv: SmallVec<u32, 4> = SmallVec::new();
        sv.push(0);
        sv.push(1);
        sv.push(2);
        sv.push(3);

        kani::assert(sv.is_inline(), "must be inline for inline retain path");

        sv.retain(|&x| x < threshold);

        // Expected survivors: values in {0..threshold}.
        let expected_len = threshold as usize;
        kani::assert(
            sv.len() == expected_len,
            "len must equal count of elements satisfying predicate",
        );

        // Every retained element must satisfy the predicate.
        let slice = sv.as_slice();
        let mut i = 0;
        while i < sv.len() {
            kani::assert(
                slice[i] < threshold,
                "retained element must satisfy predicate",
            );
            i += 1;
        }
    }

    /// Verify the spill transition: ensure_heap moves all inline elements
    /// to the heap without loss or reordering.
    ///
    /// Pushes symbolic values inline, forces a spill via insert at capacity,
    /// and verifies all original elements are preserved.
    #[kani::proof]
    #[kani::unwind(7)]
    fn smallvec_spill_transition_preserves_all() {
        let a: u32 = kani::any();
        let b: u32 = kani::any();
        let c: u32 = kani::any();
        let d: u32 = kani::any();

        let mut sv: SmallVec<u32, 4> = SmallVec::new();
        sv.push(a);
        sv.push(b);
        sv.push(c);
        sv.push(d);

        kani::assert(sv.is_inline(), "must start inline");

        // Force spill via insert at end (capacity is full).
        let extra: u32 = kani::any();
        sv.insert(4, extra);

        kani::assert(!sv.is_inline(), "must be heap after spill via insert");
        kani::assert(sv.len() == 5, "len must be 5 after insert-spill");

        // All original elements preserved in order.
        let slice = sv.as_slice();
        kani::assert(slice[0] == a, "element 0 preserved after spill");
        kani::assert(slice[1] == b, "element 1 preserved after spill");
        kani::assert(slice[2] == c, "element 2 preserved after spill");
        kani::assert(slice[3] == d, "element 3 preserved after spill");
        kani::assert(slice[4] == extra, "inserted element at correct position");
    }

    /// Verify as_slice length invariant: for a symbolic number of pushes,
    /// as_slice().len() always equals the SmallVec's len(), and the content
    /// matches in both inline and heap modes.
    #[kani::proof]
    #[kani::unwind(8)]
    fn smallvec_as_slice_length_invariant() {
        let count: usize = kani::any();
        // Allow up to 6 elements: 4 inline + 2 heap to exercise both paths.
        kani::assume(count <= 6);

        let vals: [u32; 6] = [
            kani::any(),
            kani::any(),
            kani::any(),
            kani::any(),
            kani::any(),
            kani::any(),
        ];

        let mut sv: SmallVec<u32, 4> = SmallVec::new();
        let mut i = 0;
        while i < count {
            sv.push(vals[i]);
            i += 1;
        }

        let slice = sv.as_slice();
        kani::assert(
            slice.len() == count,
            "as_slice length must equal number of pushed elements",
        );
        kani::assert(slice.len() == sv.len(), "as_slice length must equal len()");

        // Verify each element matches what was pushed.
        let mut j = 0;
        while j < count {
            kani::assert(
                slice[j] == vals[j],
                "as_slice element must match pushed value",
            );
            j += 1;
        }

        // Verify storage mode is consistent with count.
        if count <= 4 {
            kani::assert(sv.is_inline(), "must be inline when count <= N");
        } else {
            kani::assert(!sv.is_inline(), "must be heap when count > N");
        }
    }
}
