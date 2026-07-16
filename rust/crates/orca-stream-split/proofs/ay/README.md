# orca-stream-split — ay proof certificate

Machine-checked safety certificate for the surrogate-safe split-index primitives,
discharged by [`ay`](../../../../../..) (SAT/SMT/CHC solver) on hand-encoded
SMT-LIB2. This is the "machine-checked safety certificates on the emitted code" half
of the moonshot **E1** claim; the differential parity corpus
([`../../parity-corpus.txt`](../../parity-corpus.txt), run by BOTH the Rust core and
the TS `clampToSafeSplitIndex`/`nextSafeSplitIndex`) is the "regression-gated
behavioral parity corpora" half.

Run: `bash verify.sh` — exits 0 iff every obligation gets its expected verdict.

## Obligations

| file | kind | verdict | property |
|------|------|---------|----------|
| `cs1_never_splits_target_pair` | theorem | unsat | clamp never returns `end` when (end-1, end) is a high/low pair |
| `cs2_in_range` | theorem | unsat | start < end < len ⇒ start ≤ result ≤ end (non-empty, no bigger than asked) |
| `ns1_progress` | theorem | unsat | start < len ⇒ next index > start (the chunk loop never stalls) |
| `ns2_skips_full_pair` | theorem | unsat | a pair beginning at start ⇒ result = start+2 (both halves stay together) |
| `cs_c1_clamp_active_sat` | control | sat | the surrogate clamp actually fires (returns end-1) |
| `ns_c1_skip_active_sat` | control | sat | the whole-pair skip actually fires (returns start+2) |

A theorem file asserts the **negation** of its property; `unsat` means no
counterexample exists over the whole input domain, i.e. proved ∀.

## Model ↔ Rust fidelity

UTF-16 code units are modelled as free integers in `[0, 65535]`, and the surrogate
predicates become linear bounds — high `0xd800..0xdbff` = `55296..56319`, low
`0xdc00..0xdfff` = `56320..57343` — so the whole decision lives in QF_LIA with no
strings or bit-vectors. The SMT encodes each function's branch structure exactly
(`src/lib.rs`): `clamp` as `ite(H(units[end-1]) & L(units[end]), end-1, end)` under
its `end<=start || end>=len` guard, and `next` as
`ite(H(units[start]) & L(units[start+1]) & start+1<len, start+2, min(len,start+1))`.
The safety properties (never split a pair, forward progress, keep a pair whole) hold
for every code-unit assignment. Fidelity to the *running* code is grounded by the
parity corpus, whose cases are replayed by both implementations over real surrogate
pairs (😀 = `d83d de00`) — so a drift surfaces as a corpus failure.
