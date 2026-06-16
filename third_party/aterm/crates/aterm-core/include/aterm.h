// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
//
// Author: The aterm Authors
//
// Aggregate umbrella for the aterm C headers.
//
// NOTE (2026-06-15): these per-subsystem headers are LEGACY artifacts from the
// original C-ABI carve and describe a much larger `AtermTerminal`/`AtermGrid` ABI
// than the shipped cdylib actually exports. The REAL, currently-exported C ABI is the 7
// `aterm_engine_*` / `aterm_string_free` functions in `crates/aterm-ffi` (opaque
// `AtermEngine` handle). A C consumer should bind those; the headers below remain
// for reference and are slated for regeneration against the real exports.
//
// Three previously-#included headers (aterm_keyboard.h / aterm_media.h /
// aterm_voice.h) never existed in-tree, so this umbrella did not compile; those
// dead includes have been removed.

#ifndef ATERM_H
#define ATERM_H

#include "aterm_base.h"
#include "aterm_terminal.h"
#include "aterm_config.h"
#include "aterm_memory.h"
#include "aterm_grid.h"
#include "aterm_gpu.h"
#include "aterm_editor.h"
#include "aterm_security.h"

#endif /* ATERM_H */
