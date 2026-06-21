; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 The aterm Authors
;
; A2 — LOAD-BEARING LEMMA: every byte the base64 encoder emits is ASCII (< 128),
;      which is exactly what makes the encoder's unsafe from_utf8_unchecked sound.
;      Discharged by `ay`.
; Expected: unsat  (the negation — "some emitted byte is >= 128" — is unsatisfiable;
;                   so EVERY emitted byte is ASCII, for every reachable index).
;
; FAITHFUL SOURCE (crates/aterm-codec/src/base64.rs):
;   :9-10  const STANDARD_ALPHABET: &[u8; 64] =
;            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
;   :117-120,128-139  out.push(alphabet[((n >> k) & 0x3F) as usize]);  // emitted byte
;   :131-141          out.push(b'=');                                   // pad byte
;   :148  unsafe { String::from_utf8_unchecked(out) }
;         // SAFETY: all output bytes are ASCII from the alphabet or '='
;
;   Every emitted byte is EITHER alphabet[idx] for some idx in 0..=63 (the masked
;   index, proved in-bounds by encoder_alphabet_index_inbounds.smt2) OR the pad
;   byte b'=' (61). This file models the CONCRETE 64-byte STANDARD_ALPHABET as a
;   pure-BV ite-chain over the index (no array theory needed: stays in QF_BV) and
;   proves every entry — and '=' — is < 128.
;
; THEOREM:  for all idx with 0 <= idx <= 63,  STANDARD_ALPHABET[idx] < 128,
;           AND  b'=' (61) < 128.
;   The alphabet is A-Z(65..90) a-z(97..122) 0-9(48..57) +(43) /(47); max byte is
;   122 < 128, so all 64 entries are ASCII. The actual emitted index is the masked
;   value (n>>k)&0x3F, which the companion lemma proves is always in 0..=63, so the
;   "0 <= idx <= 63" antecedent here exactly covers the reachable emission domain.
;
; NOTE on the URL-safe alphabet (:13-14): identical except idx 62='-'(45), 63='_'(95);
;   both < 128, so the same ASCII conclusion holds (the union of both alphabets is
;   ASCII). This file models the STANDARD alphabet, which encode()/encode_no_pad use.
(set-logic QF_BV)

; idx is the masked emission index; the companion lemma bounds it to 0..=63.
(declare-const idx (_ BitVec 32))
(assert (bvule idx (_ bv63 32)))

; STANDARD_ALPHABET as a pure-BV ite-chain (idx -> byte). Default arm is idx 63 ('/')
; since idx is constrained to 0..=63 above, so every reachable idx hits a real entry.
(define-fun ALPHABET () (_ BitVec 8)
  (ite (= idx (_ bv0 32)) (_ bv65 8)  ; 'A'
  (ite (= idx (_ bv1 32)) (_ bv66 8)  ; 'B'
  (ite (= idx (_ bv2 32)) (_ bv67 8)  ; 'C'
  (ite (= idx (_ bv3 32)) (_ bv68 8)  ; 'D'
  (ite (= idx (_ bv4 32)) (_ bv69 8)  ; 'E'
  (ite (= idx (_ bv5 32)) (_ bv70 8)  ; 'F'
  (ite (= idx (_ bv6 32)) (_ bv71 8)  ; 'G'
  (ite (= idx (_ bv7 32)) (_ bv72 8)  ; 'H'
  (ite (= idx (_ bv8 32)) (_ bv73 8)  ; 'I'
  (ite (= idx (_ bv9 32)) (_ bv74 8)  ; 'J'
  (ite (= idx (_ bv10 32)) (_ bv75 8)  ; 'K'
  (ite (= idx (_ bv11 32)) (_ bv76 8)  ; 'L'
  (ite (= idx (_ bv12 32)) (_ bv77 8)  ; 'M'
  (ite (= idx (_ bv13 32)) (_ bv78 8)  ; 'N'
  (ite (= idx (_ bv14 32)) (_ bv79 8)  ; 'O'
  (ite (= idx (_ bv15 32)) (_ bv80 8)  ; 'P'
  (ite (= idx (_ bv16 32)) (_ bv81 8)  ; 'Q'
  (ite (= idx (_ bv17 32)) (_ bv82 8)  ; 'R'
  (ite (= idx (_ bv18 32)) (_ bv83 8)  ; 'S'
  (ite (= idx (_ bv19 32)) (_ bv84 8)  ; 'T'
  (ite (= idx (_ bv20 32)) (_ bv85 8)  ; 'U'
  (ite (= idx (_ bv21 32)) (_ bv86 8)  ; 'V'
  (ite (= idx (_ bv22 32)) (_ bv87 8)  ; 'W'
  (ite (= idx (_ bv23 32)) (_ bv88 8)  ; 'X'
  (ite (= idx (_ bv24 32)) (_ bv89 8)  ; 'Y'
  (ite (= idx (_ bv25 32)) (_ bv90 8)  ; 'Z'
  (ite (= idx (_ bv26 32)) (_ bv97 8)  ; 'a'
  (ite (= idx (_ bv27 32)) (_ bv98 8)  ; 'b'
  (ite (= idx (_ bv28 32)) (_ bv99 8)  ; 'c'
  (ite (= idx (_ bv29 32)) (_ bv100 8)  ; 'd'
  (ite (= idx (_ bv30 32)) (_ bv101 8)  ; 'e'
  (ite (= idx (_ bv31 32)) (_ bv102 8)  ; 'f'
  (ite (= idx (_ bv32 32)) (_ bv103 8)  ; 'g'
  (ite (= idx (_ bv33 32)) (_ bv104 8)  ; 'h'
  (ite (= idx (_ bv34 32)) (_ bv105 8)  ; 'i'
  (ite (= idx (_ bv35 32)) (_ bv106 8)  ; 'j'
  (ite (= idx (_ bv36 32)) (_ bv107 8)  ; 'k'
  (ite (= idx (_ bv37 32)) (_ bv108 8)  ; 'l'
  (ite (= idx (_ bv38 32)) (_ bv109 8)  ; 'm'
  (ite (= idx (_ bv39 32)) (_ bv110 8)  ; 'n'
  (ite (= idx (_ bv40 32)) (_ bv111 8)  ; 'o'
  (ite (= idx (_ bv41 32)) (_ bv112 8)  ; 'p'
  (ite (= idx (_ bv42 32)) (_ bv113 8)  ; 'q'
  (ite (= idx (_ bv43 32)) (_ bv114 8)  ; 'r'
  (ite (= idx (_ bv44 32)) (_ bv115 8)  ; 's'
  (ite (= idx (_ bv45 32)) (_ bv116 8)  ; 't'
  (ite (= idx (_ bv46 32)) (_ bv117 8)  ; 'u'
  (ite (= idx (_ bv47 32)) (_ bv118 8)  ; 'v'
  (ite (= idx (_ bv48 32)) (_ bv119 8)  ; 'w'
  (ite (= idx (_ bv49 32)) (_ bv120 8)  ; 'x'
  (ite (= idx (_ bv50 32)) (_ bv121 8)  ; 'y'
  (ite (= idx (_ bv51 32)) (_ bv122 8)  ; 'z'
  (ite (= idx (_ bv52 32)) (_ bv48 8)  ; '0'
  (ite (= idx (_ bv53 32)) (_ bv49 8)  ; '1'
  (ite (= idx (_ bv54 32)) (_ bv50 8)  ; '2'
  (ite (= idx (_ bv55 32)) (_ bv51 8)  ; '3'
  (ite (= idx (_ bv56 32)) (_ bv52 8)  ; '4'
  (ite (= idx (_ bv57 32)) (_ bv53 8)  ; '5'
  (ite (= idx (_ bv58 32)) (_ bv54 8)  ; '6'
  (ite (= idx (_ bv59 32)) (_ bv55 8)  ; '7'
  (ite (= idx (_ bv60 32)) (_ bv56 8)  ; '8'
  (ite (= idx (_ bv61 32)) (_ bv57 8)  ; '9'
  (ite (= idx (_ bv62 32)) (_ bv43 8)  ; '+'
  (_ bv47 8)  ; idx 63 -> '/'
  ))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))

(define-fun emitted () (_ BitVec 8) ALPHABET)
; negation: an emitted alphabet byte is NOT ASCII (>= 128), OR pad '=' (61) is not ASCII.
; The second disjunct (61 >= 128) is trivially false, so the model can only be
; satisfied by a genuinely non-ASCII alphabet byte — there is none => unsat.
(assert (or (bvuge emitted (_ bv128 8))
            (bvuge (_ bv61 8) (_ bv128 8))))   ; (_ bv61 8) = b'='
(check-sat)
