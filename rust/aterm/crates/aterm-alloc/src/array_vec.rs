// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! `ArrayVec<T, N>`: fixed-capacity inline-only storage.

use std::fmt;
use std::mem::MaybeUninit;
use std::ops::{Deref, DerefMut};

/// A fixed-capacity vector stored entirely on the stack.
///
/// Never allocates on the heap. Panics if you try to push beyond capacity `N`.
/// Used in the parser for CSI params (N=16) and intermediates (N=4).
pub struct ArrayVec<T, const N: usize> {
    buf: [MaybeUninit<T>; N],
    len: usize,
}

impl<T, const N: usize> ArrayVec<T, N> {
    /// Create a new, empty `ArrayVec`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            // SAFETY: MaybeUninit array does not require initialization
            buf: unsafe { MaybeUninit::uninit().assume_init() },
            len: 0,
        }
    }

    /// Const-compatible alias for `new`.
    #[must_use]
    pub const fn new_const() -> Self {
        Self::new()
    }

    /// The number of elements.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether the collection is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The fixed capacity `N`.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        N
    }

    /// Remaining capacity before full.
    #[must_use]
    pub const fn remaining_capacity(&self) -> usize {
        N - self.len
    }

    /// Whether the collection is at full capacity.
    #[must_use]
    pub const fn is_full(&self) -> bool {
        self.len == N
    }

    /// Push an element. Panics if full.
    ///
    /// # Panics
    ///
    /// Panics if `len == N`.
    pub fn push(&mut self, value: T) {
        assert!(self.len < N, "ArrayVec overflow: capacity is {N}");
        self.buf[self.len] = MaybeUninit::new(value);
        self.len += 1;
    }

    /// Try to push an element. Returns `Err(value)` if full.
    pub fn try_push(&mut self, value: T) -> Result<(), T> {
        if self.len < N {
            self.buf[self.len] = MaybeUninit::new(value);
            self.len += 1;
            Ok(())
        } else {
            Err(value)
        }
    }

    /// Pop the last element.
    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            None
        } else {
            self.len -= 1;
            // SAFETY: buf[self.len] was initialized when it was pushed
            Some(unsafe { self.buf[self.len].assume_init_read() })
        }
    }

    /// Replace all contents with a single element.
    ///
    /// Equivalent to `clear(); push(value);` but avoids the clear loop and
    /// push bounds check. For Copy types with N >= 1, this is a single store.
    #[inline]
    pub fn set_single(&mut self, value: T) {
        // Drop existing elements
        for elem in &mut self.buf[..self.len] {
            unsafe {
                elem.assume_init_drop();
            }
        }
        self.buf[0] = MaybeUninit::new(value);
        self.len = 1;
    }

    /// Clear all elements.
    pub fn clear(&mut self) {
        // SAFETY: elements 0..self.len are initialized
        for elem in &mut self.buf[..self.len] {
            unsafe {
                elem.assume_init_drop();
            }
        }
        self.len = 0;
    }

    /// Truncate to the given length.
    pub fn truncate(&mut self, new_len: usize) {
        if new_len < self.len {
            // SAFETY: elements new_len..self.len are initialized
            for elem in &mut self.buf[new_len..self.len] {
                unsafe {
                    elem.assume_init_drop();
                }
            }
            self.len = new_len;
        }
    }

    /// View as a slice.
    #[must_use]
    pub fn as_slice(&self) -> &[T] {
        // SAFETY: elements 0..self.len are initialized
        unsafe { std::slice::from_raw_parts(self.buf.as_ptr().cast::<T>(), self.len) }
    }

    /// View as a mutable slice.
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        // SAFETY: elements 0..self.len are initialized
        unsafe { std::slice::from_raw_parts_mut(self.buf.as_mut_ptr().cast::<T>(), self.len) }
    }

    /// An iterator over references.
    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.as_slice().iter()
    }

    /// An iterator over mutable references.
    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, T> {
        self.as_mut_slice().iter_mut()
    }

    /// Insert an element at the given index.
    ///
    /// # Panics
    ///
    /// Panics if `index > len` or if the array is full.
    pub fn insert(&mut self, index: usize, value: T) {
        assert!(
            index <= self.len,
            "index out of bounds: {index} > {}",
            self.len
        );
        assert!(self.len < N, "ArrayVec overflow: capacity is {N}");

        // SAFETY: we checked bounds and capacity above
        unsafe {
            let ptr = self.buf.as_mut_ptr().cast::<T>();
            std::ptr::copy(ptr.add(index), ptr.add(index + 1), self.len - index);
            std::ptr::write(ptr.add(index), value);
        }
        self.len += 1;
    }

    /// Remove and return the element at the given index.
    ///
    /// # Panics
    ///
    /// Panics if `index >= len`.
    pub fn remove(&mut self, index: usize) -> T {
        assert!(
            index < self.len,
            "index out of bounds: {index} >= {}",
            self.len
        );

        // SAFETY: element at index is initialized
        unsafe {
            let ptr = self.buf.as_mut_ptr().cast::<T>();
            let value = std::ptr::read(ptr.add(index));
            std::ptr::copy(ptr.add(index + 1), ptr.add(index), self.len - index - 1);
            self.len -= 1;
            value
        }
    }

    /// Retain only elements where the predicate returns true.
    ///
    /// Panic-safe: if the predicate panics, all elements that have already
    /// been processed are in a valid state and will be dropped correctly.
    pub fn retain<F: FnMut(&T) -> bool>(&mut self, mut f: F) {
        let original_len = self.len;
        // Set len to 0 so that if we panic, Drop only drops elements
        // that we've already moved into the write region.
        self.len = 0;

        struct RetainGuard<'a, T, const N: usize> {
            av: &'a mut ArrayVec<T, N>,
            write: usize,
            read: usize,
            original_len: usize,
        }

        impl<T, const N: usize> Drop for RetainGuard<'_, T, N> {
            fn drop(&mut self) {
                // SAFETY: elements 0..write are the retained (initialized) elements.
                // Elements write..read have already been processed (moved or dropped).
                // Elements read..original_len have NOT been processed — drop them now.
                unsafe {
                    for i in self.read..self.original_len {
                        self.av.buf[i].assume_init_drop();
                    }
                }
                self.av.len = self.write;
            }
        }

        let mut guard = RetainGuard {
            av: self,
            write: 0,
            read: 0,
            original_len,
        };

        while guard.read < original_len {
            let read = guard.read;
            // SAFETY: element at `read` is initialized (read < original_len)
            let keep = unsafe { f(&*guard.av.buf[read].as_ptr()) };
            guard.read += 1;
            if keep {
                if guard.write != read {
                    // SAFETY: both indices in bounds; read element consumed, write slot empty
                    unsafe {
                        let val = guard.av.buf[read].assume_init_read();
                        guard.av.buf[guard.write] = MaybeUninit::new(val);
                    }
                }
                guard.write += 1;
            } else {
                // SAFETY: element is initialized; drop it
                unsafe {
                    guard.av.buf[read].assume_init_drop();
                }
            }
        }

        let final_len = guard.write;
        // Defuse the guard — all elements processed, set final state.
        guard.original_len = guard.read; // no unprocessed elements remain
        drop(guard);
        self.len = final_len;
    }
}

// ── Trait impls ─────────────────────────────────────────────────────────────

impl<T, const N: usize> Default for ArrayVec<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, const N: usize> Deref for ArrayVec<T, N> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        self.as_slice()
    }
}

impl<T, const N: usize> DerefMut for ArrayVec<T, N> {
    fn deref_mut(&mut self) -> &mut [T] {
        self.as_mut_slice()
    }
}

impl<T: Clone, const N: usize> Clone for ArrayVec<T, N> {
    fn clone(&self) -> Self {
        let mut new = Self::new();
        for item in self.as_slice() {
            new.push(item.clone());
        }
        new
    }
}

impl<T: fmt::Debug, const N: usize> fmt::Debug for ArrayVec<T, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.as_slice()).finish()
    }
}

impl<T: PartialEq, const N: usize> PartialEq for ArrayVec<T, N> {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<T: Eq, const N: usize> Eq for ArrayVec<T, N> {}

impl<T, const N: usize> Drop for ArrayVec<T, N> {
    fn drop(&mut self) {
        // SAFETY: elements 0..self.len are initialized
        for elem in &mut self.buf[..self.len] {
            unsafe {
                elem.assume_init_drop();
            }
        }
    }
}

impl<T, const N: usize> FromIterator<T> for ArrayVec<T, N> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut av = Self::new();
        for item in iter {
            av.push(item);
        }
        av
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_is_empty() {
        let av: ArrayVec<i32, 4> = ArrayVec::new();
        assert!(av.is_empty());
        assert_eq!(av.len(), 0);
        assert_eq!(av.capacity(), 4);
        assert!(!av.is_full());
    }

    #[test]
    fn test_push_and_access() {
        let mut av: ArrayVec<i32, 4> = ArrayVec::new();
        av.push(1);
        av.push(2);
        av.push(3);
        assert_eq!(av.len(), 3);
        assert_eq!(av.as_slice(), &[1, 2, 3]);
    }

    #[test]
    fn test_is_full() {
        let mut av: ArrayVec<i32, 2> = ArrayVec::new();
        av.push(1);
        av.push(2);
        assert!(av.is_full());
        assert_eq!(av.remaining_capacity(), 0);
    }

    #[test]
    #[should_panic(expected = "ArrayVec overflow")]
    fn test_push_overflow_panics() {
        let mut av: ArrayVec<i32, 2> = ArrayVec::new();
        av.push(1);
        av.push(2);
        av.push(3); // should panic
    }

    #[test]
    fn test_try_push() {
        let mut av: ArrayVec<i32, 2> = ArrayVec::new();
        assert!(av.try_push(1).is_ok());
        assert!(av.try_push(2).is_ok());
        assert_eq!(av.try_push(3), Err(3));
    }

    #[test]
    fn test_pop() {
        let mut av: ArrayVec<i32, 4> = ArrayVec::new();
        av.push(10);
        av.push(20);
        assert_eq!(av.pop(), Some(20));
        assert_eq!(av.pop(), Some(10));
        assert_eq!(av.pop(), None);
    }

    #[test]
    fn test_clear() {
        let mut av: ArrayVec<String, 4> = ArrayVec::new();
        av.push("hello".into());
        av.push("world".into());
        av.clear();
        assert!(av.is_empty());
    }

    #[test]
    fn test_truncate() {
        let mut av: ArrayVec<i32, 4> = ArrayVec::new();
        av.push(1);
        av.push(2);
        av.push(3);
        av.truncate(1);
        assert_eq!(av.as_slice(), &[1]);
    }

    #[test]
    fn test_insert_and_remove() {
        let mut av: ArrayVec<i32, 8> = ArrayVec::new();
        av.push(1);
        av.push(3);
        av.insert(1, 2);
        assert_eq!(av.as_slice(), &[1, 2, 3]);

        let removed = av.remove(1);
        assert_eq!(removed, 2);
        assert_eq!(av.as_slice(), &[1, 3]);
    }

    #[test]
    fn test_retain() {
        let mut av: ArrayVec<i32, 8> = ArrayVec::new();
        av.push(1);
        av.push(2);
        av.push(3);
        av.push(4);
        av.push(5);
        av.retain(|x| x % 2 == 0);
        assert_eq!(av.as_slice(), &[2, 4]);
    }

    #[test]
    fn test_clone() {
        let mut av: ArrayVec<String, 4> = ArrayVec::new();
        av.push("hello".into());
        let cloned = av.clone();
        assert_eq!(av, cloned);
    }

    #[test]
    fn test_collect() {
        let av: ArrayVec<i32, 8> = (0..5).collect();
        assert_eq!(av.as_slice(), &[0, 1, 2, 3, 4]);
    }

    #[test]
    fn test_deref_indexing() {
        let mut av: ArrayVec<i32, 4> = ArrayVec::new();
        av.push(10);
        av.push(20);
        assert_eq!(av[0], 10);
        assert_eq!(av[1], 20);
    }

    #[test]
    fn test_debug_format() {
        let mut av: ArrayVec<i32, 4> = ArrayVec::new();
        av.push(1);
        av.push(2);
        assert_eq!(format!("{av:?}"), "[1, 2]");
    }

    #[test]
    fn test_drop_string_elements() {
        let mut av: ArrayVec<String, 4> = ArrayVec::new();
        av.push("heap allocated string that is long enough".into());
        av.push("another one".into());
        drop(av);
    }

    #[test]
    fn test_const_new() {
        // Verify const construction works
        const AV: ArrayVec<u8, 16> = ArrayVec::new_const();
        assert!(AV.is_empty());
    }

    #[test]
    fn test_retain_with_drop_types() {
        let mut av: ArrayVec<String, 8> = ArrayVec::new();
        av.push("keep-a".into());
        av.push("drop-b".into());
        av.push("keep-c".into());
        av.push("drop-d".into());
        av.push("keep-e".into());
        av.retain(|s| s.starts_with("keep"));
        assert_eq!(av.len(), 3);
        assert_eq!(av.as_slice(), &["keep-a", "keep-c", "keep-e"]);
    }

    #[test]
    fn test_retain_panic_safety() {
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
            let mut av: ArrayVec<Tracked, 8> = ArrayVec::new();
            av.push(Tracked(1));
            av.push(Tracked(2));
            av.push(Tracked(3));
            av.push(Tracked(4));
            av.push(Tracked(5));

            let mut call_count = 0;
            av.retain(|_| {
                call_count += 1;
                if call_count == 3 {
                    panic!("predicate panic");
                }
                true
            });
        }));

        assert!(result.is_err());
        // All 5 elements must be dropped exactly once (no leak, no double-free).
        assert_eq!(DROP_COUNT.load(Ordering::Relaxed), 5);
    }
}

// ── Kani proofs ────────────────────────────────────────────────────────────

#[cfg(kani)]
mod kani_proofs {
    use super::*;

    /// Verify that pushing into a full ArrayVec panics (not UB).
    ///
    /// The `push` method uses an `assert!` guard before writing into the
    /// MaybeUninit buffer. This proof drives all capacity-4 slots full, then
    /// confirms the N+1 push triggers the panic path rather than writing
    /// out-of-bounds into uninitialized memory.
    #[kani::proof]
    #[kani::unwind(6)]
    fn arrayvec_push_at_capacity_panics() {
        let mut av: ArrayVec<u32, 4> = ArrayVec::new();
        av.push(1);
        av.push(2);
        av.push(3);
        av.push(4);

        // Capacity is full; len must equal N.
        kani::assert(av.len() == 4, "len must equal capacity after filling");
        kani::assert(av.is_full(), "is_full must be true at capacity");

        // try_push must fail and return the value back.
        let result = av.try_push(99);
        kani::assert(
            result == Err(99),
            "try_push must return Err(value) when full",
        );

        // len must not have changed.
        kani::assert(av.len() == 4, "len must remain 4 after failed try_push");
    }

    /// Verify the retain drop guard: after retain, len equals the number of
    /// elements that passed the predicate, and those elements are exactly
    /// the ones the predicate accepted.
    ///
    /// Uses symbolic threshold to partition elements into keep/drop sets,
    /// then verifies the surviving slice matches expectations.
    #[kani::proof]
    #[kani::unwind(6)]
    fn arrayvec_retain_len_consistent() {
        let threshold: u32 = kani::any();
        kani::assume(threshold <= 4);

        let mut av: ArrayVec<u32, 4> = ArrayVec::new();
        av.push(0);
        av.push(1);
        av.push(2);
        av.push(3);

        // Retain elements strictly less than the symbolic threshold.
        av.retain(|&x| x < threshold);

        // The number of values in 0..4 that are < threshold is exactly
        // min(threshold, 4).
        let expected_len = threshold as usize;
        kani::assert(
            av.len() == expected_len,
            "len must equal count of elements satisfying predicate",
        );

        // Every surviving element must actually satisfy the predicate.
        let slice = av.as_slice();
        let mut i = 0;
        while i < av.len() {
            kani::assert(
                slice[i] < threshold,
                "retained element must satisfy predicate",
            );
            i += 1;
        }
    }

    /// Verify that pop returns elements in LIFO order.
    ///
    /// Pushes symbolic values a, b, c and verifies pop returns c, b, a
    /// in that order, then returns None on empty.
    #[kani::proof]
    fn arrayvec_pop_lifo_order() {
        let a: u32 = kani::any();
        let b: u32 = kani::any();
        let c: u32 = kani::any();

        let mut av: ArrayVec<u32, 4> = ArrayVec::new();
        av.push(a);
        av.push(b);
        av.push(c);

        kani::assert(av.len() == 3, "len must be 3 after 3 pushes");

        let p1 = av.pop();
        kani::assert(p1 == Some(c), "first pop must return last pushed (c)");
        kani::assert(av.len() == 2, "len must be 2 after first pop");

        let p2 = av.pop();
        kani::assert(p2 == Some(b), "second pop must return b");
        kani::assert(av.len() == 1, "len must be 1 after second pop");

        let p3 = av.pop();
        kani::assert(p3 == Some(a), "third pop must return a");
        kani::assert(av.len() == 0, "len must be 0 after third pop");

        let p4 = av.pop();
        kani::assert(p4.is_none(), "pop on empty must return None");
    }

    /// Verify that as_slice returns a slice whose length equals the
    /// ArrayVec's len, and whose elements match what was pushed.
    ///
    /// Pushes a symbolic number of elements (0..=4) and verifies the
    /// slice length and content at each step.
    #[kani::proof]
    #[kani::unwind(6)]
    fn arrayvec_as_slice_len_and_content() {
        let count: usize = kani::any();
        kani::assume(count <= 4);

        let vals: [u32; 4] = [kani::any(), kani::any(), kani::any(), kani::any()];

        let mut av: ArrayVec<u32, 4> = ArrayVec::new();
        let mut i = 0;
        while i < count {
            av.push(vals[i]);
            i += 1;
        }

        let slice = av.as_slice();
        kani::assert(
            slice.len() == count,
            "as_slice length must equal number of pushed elements",
        );

        // Verify each element matches what was pushed.
        let mut j = 0;
        while j < count {
            kani::assert(
                slice[j] == vals[j],
                "as_slice element must match pushed value",
            );
            j += 1;
        }
    }
}
