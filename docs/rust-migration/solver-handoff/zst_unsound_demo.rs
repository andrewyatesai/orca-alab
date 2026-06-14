#![crate_type = "lib"]
// SOUNDNESS DEMONSTRATION for origin/main's `conjoin_slice_len_bounds` (generate.rs:3107).
// `&[()]` has a ZERO-SIZED element, so size_of::<()>() * len = 0 <= isize::MAX for
// ANY len — the slice length is NOT bounded by isize::MAX and can reach usize::MAX.
// Therefore `s.len() + 2` CAN overflow (at len in {usize::MAX-1, usize::MAX}) and a
// SOUND verifier must REFUTE it. The unconditional `__slice_len <= isize::MAX` bound
// excludes those models, so the owner's version FALSE-PROVES this. <-- the bug.
pub fn zst_slice_overflow(s: &[()]) -> usize { s.len() + 2 }
// Control: non-ZST element — len IS bounded by isize::MAX, so this genuinely cannot
// overflow and is SOUND to prove.
pub fn nonzst_slice_safe(s: &[u8]) -> usize { s.len() + 2 }
