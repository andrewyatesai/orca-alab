; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 The aterm Authors
;
; A6 documented-assumption record — atlas WIDTH (1024) <= device limit (SAFE under
; the stated precondition). By `ay`.
; Expected: unsat  (under assume(max >= 2048), the constant width fits).
;
; FAITHFUL SOURCE (crates/aterm-gpu/src/renderer.rs):
;   :47    const ATLAS_WIDTH: u32 = 1024;
;   :408   width: ATLAS_WIDTH,                 (atlas.width is this constant)
;   :1408  size: Extent3d { width: atlas.width, height: tex_h, .. }
;   The clamps at :1404-1405 are HEIGHT-ONLY. The texture WIDTH is the fixed
;   constant 1024 — there is NO runtime clamp on width. So whether width fits the
;   device is NOT a proof about the code; it is a PRECONDITION on the device.
;
; STATED PRECONDITION (NOT proved here, recorded as an assumption):
;   max_texture_dimension_2d >= 2048.  This is wgpu's downlevel minimum
;   (WebGL2 / DOWNLEVEL_WEBGL2_LIMITS guarantee 2048); every backend aterm targets
;   meets it. We assume max >= 2048 and confirm ATLAS_WIDTH (1024) <= max.
;
; THEOREM:  under assume(max >= 2048),  ATLAS_WIDTH (1024) <= max.
;   This is a TAUTOLOGY-UNDER-ASSUMPTION (1024 <= 2048 <= max). Its job is to put
;   the width assumption ON THE RECORD as the explicit side-condition of the
;   device-abort-impossibility claim — see README "Honest scope". It is deliberately
;   NOT phrased as `for all max, 1024 <= max` (which is FALSE for max < 1024).
(set-logic QF_BV)
(declare-const max (_ BitVec 32))               ; max_texture_dimension_2d
(define-fun ATLAS_WIDTH () (_ BitVec 32) (_ bv1024 32))
(define-fun WGPU_DOWNLEVEL_MIN () (_ BitVec 32) (_ bv2048 32))
(assert (bvuge max WGPU_DOWNLEVEL_MIN))                  ; stated precondition
(assert (bvugt ATLAS_WIDTH max))                         ; negation: width EXCEEDS limit
(check-sat)
