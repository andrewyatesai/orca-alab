/* @ts-self-types="./orca_git_wasm.d.ts" */

/**
 * Approximate added/removed line counts for a diff section; returns the
 * line-stats JSON, or `undefined` for the large-input guard (>500k combined
 * chars — splitting that in a React render would block the UI). This one is
 * consumed by the RENDERER (not the relay): the renderer has no napi access,
 * so it loads this same wasm.
 * @param {string} original
 * @param {string} modified
 * @param {string} status
 * @returns {string | undefined}
 */
export function computeLineStats(original, modified, status) {
  try {
    const retptr = wasm.__wbindgen_add_to_stack_pointer(-16)
    const ptr0 = passStringToWasm0(original, wasm.__wbindgen_export, wasm.__wbindgen_export2)
    const len0 = WASM_VECTOR_LEN
    const ptr1 = passStringToWasm0(modified, wasm.__wbindgen_export, wasm.__wbindgen_export2)
    const len1 = WASM_VECTOR_LEN
    const ptr2 = passStringToWasm0(status, wasm.__wbindgen_export, wasm.__wbindgen_export2)
    const len2 = WASM_VECTOR_LEN
    wasm.computeLineStats(retptr, ptr0, len0, ptr1, len1, ptr2, len2)
    var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true)
    var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true)
    let v4
    if (r0 !== 0) {
      v4 = getStringFromWasm0(r0, r1).slice()
      wasm.__wbindgen_export3(r0, r1 * 1, 1)
    }
    return v4
  } finally {
    wasm.__wbindgen_add_to_stack_pointer(16)
  }
}

/**
 * Count additions for an untracked file's contents: `undefined` for binary, 0 for
 * empty, else the trailing-newline-aware line count.
 * @param {Uint8Array} bytes
 * @returns {number | undefined}
 */
export function countAdditionsInBuffer(bytes) {
  const ptr0 = passArray8ToWasm0(bytes, wasm.__wbindgen_export)
  const len0 = WASM_VECTOR_LEN
  const ret = wasm.countAdditionsInBuffer(ptr0, len0)
  return ret === 0x100000001 ? undefined : ret
}

/**
 * Decode a git C-quoted (octal-escaped) path. Raw (unquoted) input passes through.
 * @param {string} value
 * @returns {string}
 */
export function decodeGitCQuotedPath(value) {
  let deferred2_0
  let deferred2_1
  try {
    const retptr = wasm.__wbindgen_add_to_stack_pointer(-16)
    const ptr0 = passStringToWasm0(value, wasm.__wbindgen_export, wasm.__wbindgen_export2)
    const len0 = WASM_VECTOR_LEN
    wasm.decodeGitCQuotedPath(retptr, ptr0, len0)
    var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true)
    var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true)
    deferred2_0 = r0
    deferred2_1 = r1
    return getStringFromWasm0(r0, r1)
  } finally {
    wasm.__wbindgen_add_to_stack_pointer(16)
    wasm.__wbindgen_export3(deferred2_0, deferred2_1, 1)
  }
}

/**
 * Which Pi-compatible agent a launch command starts: `"omp"` for OMP
 * (`omp` / `omp.sh`), else `"pi"`. The relay uses this to target the managed
 * extension dir for the actual agent being launched.
 * @param {string | null} [command]
 * @returns {string}
 */
export function detectPiAgentKindFromCommand(command) {
  let deferred2_0
  let deferred2_1
  try {
    const retptr = wasm.__wbindgen_add_to_stack_pointer(-16)
    var ptr0 = isLikeNone(command)
      ? 0
      : passStringToWasm0(command, wasm.__wbindgen_export, wasm.__wbindgen_export2)
    var len0 = WASM_VECTOR_LEN
    wasm.detectPiAgentKindFromCommand(retptr, ptr0, len0)
    var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true)
    var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true)
    deferred2_0 = r0
    deferred2_1 = r1
    return getStringFromWasm0(r0, r1)
  } finally {
    wasm.__wbindgen_add_to_stack_pointer(16)
    wasm.__wbindgen_export3(deferred2_0, deferred2_1, 1)
  }
}

/**
 * The actionable nested-submodule rejection hidden behind a recursive-push
 * failure, or `undefined`. Consumed by the RENDERER (push-failure toasts) via
 * this same wasm.
 * @param {string} message
 * @returns {string | undefined}
 */
export function formatSubmodulePushFailureDetail(message) {
  try {
    const retptr = wasm.__wbindgen_add_to_stack_pointer(-16)
    const ptr0 = passStringToWasm0(message, wasm.__wbindgen_export, wasm.__wbindgen_export2)
    const len0 = WASM_VECTOR_LEN
    wasm.formatSubmodulePushFailureDetail(retptr, ptr0, len0)
    var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true)
    var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true)
    let v2
    if (r0 !== 0) {
      v2 = getStringFromWasm0(r0, r1).slice()
      wasm.__wbindgen_export3(r0, r1 * 1, 1)
    }
    return v2
  } finally {
    wasm.__wbindgen_add_to_stack_pointer(16)
  }
}

/**
 * True only for clearly-no-upstream signals (an expected state, gated on a
 * `fatal:` prefix). `undefined` message -> false (a non-Error throw in TS).
 * @param {string | null} [message]
 * @returns {boolean}
 */
export function isNoUpstreamError(message) {
  var ptr0 = isLikeNone(message)
    ? 0
    : passStringToWasm0(message, wasm.__wbindgen_export, wasm.__wbindgen_export2)
  var len0 = WASM_VECTOR_LEN
  const ret = wasm.isNoUpstreamError(ptr0, len0)
  return ret !== 0
}

/**
 * Normalise a git remote-operation error into a user-facing message. `message`
 * is `undefined` for a non-Error throw (returns the fixed fallback). `operation`
 * is `"push" | "pull" | "fetch" | "upstream"` (or `undefined`); an unrecognised
 * value maps to `None`, matching the TS default-parameter behaviour.
 * @param {string | null} [message]
 * @param {string | null} [operation]
 * @returns {string}
 */
export function normalizeGitErrorMessage(message, operation) {
  let deferred3_0
  let deferred3_1
  try {
    const retptr = wasm.__wbindgen_add_to_stack_pointer(-16)
    var ptr0 = isLikeNone(message)
      ? 0
      : passStringToWasm0(message, wasm.__wbindgen_export, wasm.__wbindgen_export2)
    var len0 = WASM_VECTOR_LEN
    var ptr1 = isLikeNone(operation)
      ? 0
      : passStringToWasm0(operation, wasm.__wbindgen_export, wasm.__wbindgen_export2)
    var len1 = WASM_VECTOR_LEN
    wasm.normalizeGitErrorMessage(retptr, ptr0, len0, ptr1, len1)
    var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true)
    var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true)
    deferred3_0 = r0
    deferred3_1 = r1
    return getStringFromWasm0(r0, r1)
  } finally {
    wasm.__wbindgen_add_to_stack_pointer(16)
    wasm.__wbindgen_export3(deferred3_0, deferred3_1, 1)
  }
}

/**
 * NUL-delimited `git log` (in `GIT_HISTORY_COMMIT_FORMAT`) parsed to the
 * `GitHistoryItem[]` JSON the TS `parseGitHistoryLog` produced.
 * @param {string} stdout
 * @returns {string}
 */
export function parseGitHistoryLog(stdout) {
  let deferred2_0
  let deferred2_1
  try {
    const retptr = wasm.__wbindgen_add_to_stack_pointer(-16)
    const ptr0 = passStringToWasm0(stdout, wasm.__wbindgen_export, wasm.__wbindgen_export2)
    const len0 = WASM_VECTOR_LEN
    wasm.parseGitHistoryLog(retptr, ptr0, len0)
    var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true)
    var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true)
    deferred2_0 = r0
    deferred2_1 = r1
    return getStringFromWasm0(r0, r1)
  } finally {
    wasm.__wbindgen_add_to_stack_pointer(16)
    wasm.__wbindgen_export3(deferred2_0, deferred2_1, 1)
  }
}

/**
 * `git diff --numstat` (text or `-z`) parsed to `{path: {added?, removed?}}` JSON.
 * @param {Uint8Array} stdout
 * @returns {string}
 */
export function parseNumstat(stdout) {
  let deferred2_0
  let deferred2_1
  try {
    const retptr = wasm.__wbindgen_add_to_stack_pointer(-16)
    const ptr0 = passArray8ToWasm0(stdout, wasm.__wbindgen_export)
    const len0 = WASM_VECTOR_LEN
    wasm.parseNumstat(retptr, ptr0, len0)
    var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true)
    var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true)
    deferred2_0 = r0
    deferred2_1 = r1
    return getStringFromWasm0(r0, r1)
  } finally {
    wasm.__wbindgen_add_to_stack_pointer(16)
    wasm.__wbindgen_export3(deferred2_0, deferred2_1, 1)
  }
}

/**
 * One-shot status scan (the relay's `parseStatusOutput` replacement): the cap is
 * applied DURING the scan, so `entries` is bounded by `limit`. Returns the
 * status-parse-result JSON.
 * @param {Uint8Array} stdout
 * @param {number} limit
 * @returns {string}
 */
export function parseStatusPorcelain(stdout, limit) {
  let deferred2_0
  let deferred2_1
  try {
    const retptr = wasm.__wbindgen_add_to_stack_pointer(-16)
    const ptr0 = passArray8ToWasm0(stdout, wasm.__wbindgen_export)
    const len0 = WASM_VECTOR_LEN
    wasm.parseStatusPorcelain(retptr, ptr0, len0, limit)
    var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true)
    var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true)
    deferred2_0 = r0
    deferred2_1 = r1
    return getStringFromWasm0(r0, r1)
  } finally {
    wasm.__wbindgen_add_to_stack_pointer(16)
    wasm.__wbindgen_export3(deferred2_0, deferred2_1, 1)
  }
}

/**
 * `git worktree list --porcelain` (or the `-z` NUL form) parsed to the
 * `GitWorktreeInfo[]` JSON the TS `parseWorktreeList` produced.
 * @param {string} output
 * @param {boolean} nul_delimited
 * @returns {string}
 */
export function parseWorktreeList(output, nul_delimited) {
  let deferred2_0
  let deferred2_1
  try {
    const retptr = wasm.__wbindgen_add_to_stack_pointer(-16)
    const ptr0 = passStringToWasm0(output, wasm.__wbindgen_export, wasm.__wbindgen_export2)
    const len0 = WASM_VECTOR_LEN
    wasm.parseWorktreeList(retptr, ptr0, len0, nul_delimited)
    var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true)
    var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true)
    deferred2_0 = r0
    deferred2_1 = r1
    return getStringFromWasm0(r0, r1)
  } finally {
    wasm.__wbindgen_add_to_stack_pointer(16)
    wasm.__wbindgen_export3(deferred2_0, deferred2_1, 1)
  }
}

/**
 * Scrub credentials embedded in a git URL within `message` (keeps SSH user-info;
 * strips `user:password@` on any scheme + HTTP(S) token-only `user@`).
 * @param {string} message
 * @returns {string}
 */
export function stripCredentialsFromMessage(message) {
  let deferred2_0
  let deferred2_1
  try {
    const retptr = wasm.__wbindgen_add_to_stack_pointer(-16)
    const ptr0 = passStringToWasm0(message, wasm.__wbindgen_export, wasm.__wbindgen_export2)
    const len0 = WASM_VECTOR_LEN
    wasm.stripCredentialsFromMessage(retptr, ptr0, len0)
    var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true)
    var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true)
    deferred2_0 = r0
    deferred2_1 = r1
    return getStringFromWasm0(r0, r1)
  } finally {
    wasm.__wbindgen_add_to_stack_pointer(16)
    wasm.__wbindgen_export3(deferred2_0, deferred2_1, 1)
  }
}

/**
 * True when `git cherry <upstream> HEAD`-style mark output shows at least one
 * commit and every commit is patch-equivalent (`=`). The relay's
 * behind-commits-are-patch-equivalent probe.
 * @param {string} cherry_mark_output
 * @returns {boolean}
 */
export function upstreamOnlyCommitsArePatchEquivalent(cherry_mark_output) {
  const ptr0 = passStringToWasm0(
    cherry_mark_output,
    wasm.__wbindgen_export,
    wasm.__wbindgen_export2
  )
  const len0 = WASM_VECTOR_LEN
  const ret = wasm.upstreamOnlyCommitsArePatchEquivalent(ptr0, len0)
  return ret !== 0
}

/**
 * Validate a persisted push target's *value* rules (path-traversal safety for a
 * remote name / branch name / optional GitHub URL). Returns the TS-identical
 * error message, or `undefined` when valid. The `unknown`->typed guards (the
 * "Invalid PR push target …" messages) stay in JS.
 * @param {string} remote_name
 * @param {string} branch_name
 * @param {string | null} [remote_url]
 * @returns {string | undefined}
 */
export function validateGitPushTargetRules(remote_name, branch_name, remote_url) {
  try {
    const retptr = wasm.__wbindgen_add_to_stack_pointer(-16)
    const ptr0 = passStringToWasm0(remote_name, wasm.__wbindgen_export, wasm.__wbindgen_export2)
    const len0 = WASM_VECTOR_LEN
    const ptr1 = passStringToWasm0(branch_name, wasm.__wbindgen_export, wasm.__wbindgen_export2)
    const len1 = WASM_VECTOR_LEN
    var ptr2 = isLikeNone(remote_url)
      ? 0
      : passStringToWasm0(remote_url, wasm.__wbindgen_export, wasm.__wbindgen_export2)
    var len2 = WASM_VECTOR_LEN
    wasm.validateGitPushTargetRules(retptr, ptr0, len0, ptr1, len1, ptr2, len2)
    var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true)
    var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true)
    let v4
    if (r0 !== 0) {
      v4 = getStringFromWasm0(r0, r1).slice()
      wasm.__wbindgen_export3(r0, r1 * 1, 1)
    }
    return v4
  } finally {
    wasm.__wbindgen_add_to_stack_pointer(16)
  }
}

function __wbg_get_imports() {
  const import0 = {
    __proto__: null
  }
  return {
    __proto__: null,
    './orca_git_wasm_bg.js': import0
  }
}

let cachedDataViewMemory0 = null
function getDataViewMemory0() {
  if (
    cachedDataViewMemory0 === null ||
    cachedDataViewMemory0.buffer.detached === true ||
    (cachedDataViewMemory0.buffer.detached === undefined &&
      cachedDataViewMemory0.buffer !== wasm.memory.buffer)
  ) {
    cachedDataViewMemory0 = new DataView(wasm.memory.buffer)
  }
  return cachedDataViewMemory0
}

function getStringFromWasm0(ptr, len) {
  ptr = ptr >>> 0
  return decodeText(ptr, len)
}

let cachedUint8ArrayMemory0 = null
function getUint8ArrayMemory0() {
  if (cachedUint8ArrayMemory0 === null || cachedUint8ArrayMemory0.byteLength === 0) {
    cachedUint8ArrayMemory0 = new Uint8Array(wasm.memory.buffer)
  }
  return cachedUint8ArrayMemory0
}

function isLikeNone(x) {
  return x === undefined || x === null
}

function passArray8ToWasm0(arg, malloc) {
  const ptr = malloc(arg.length * 1, 1) >>> 0
  getUint8ArrayMemory0().set(arg, ptr / 1)
  WASM_VECTOR_LEN = arg.length
  return ptr
}

function passStringToWasm0(arg, malloc, realloc) {
  if (realloc === undefined) {
    const buf = cachedTextEncoder.encode(arg)
    const ptr = malloc(buf.length, 1) >>> 0
    getUint8ArrayMemory0()
      .subarray(ptr, ptr + buf.length)
      .set(buf)
    WASM_VECTOR_LEN = buf.length
    return ptr
  }

  let len = arg.length
  let ptr = malloc(len, 1) >>> 0

  const mem = getUint8ArrayMemory0()

  let offset = 0

  for (; offset < len; offset++) {
    const code = arg.charCodeAt(offset)
    if (code > 0x7f) break
    mem[ptr + offset] = code
  }
  if (offset !== len) {
    if (offset !== 0) {
      arg = arg.slice(offset)
    }
    ptr = realloc(ptr, len, (len = offset + arg.length * 3), 1) >>> 0
    const view = getUint8ArrayMemory0().subarray(ptr + offset, ptr + len)
    const ret = cachedTextEncoder.encodeInto(arg, view)

    offset += ret.written
    ptr = realloc(ptr, len, offset, 1) >>> 0
  }

  WASM_VECTOR_LEN = offset
  return ptr
}

let cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true })
cachedTextDecoder.decode()
const MAX_SAFARI_DECODE_BYTES = 2146435072
let numBytesDecoded = 0
function decodeText(ptr, len) {
  numBytesDecoded += len
  if (numBytesDecoded >= MAX_SAFARI_DECODE_BYTES) {
    cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true })
    cachedTextDecoder.decode()
    numBytesDecoded = len
  }
  return cachedTextDecoder.decode(getUint8ArrayMemory0().subarray(ptr, ptr + len))
}

const cachedTextEncoder = new TextEncoder()

if (!('encodeInto' in cachedTextEncoder)) {
  cachedTextEncoder.encodeInto = function (arg, view) {
    const buf = cachedTextEncoder.encode(arg)
    view.set(buf)
    return {
      read: arg.length,
      written: buf.length
    }
  }
}

let WASM_VECTOR_LEN = 0

let wasmModule, wasm
function __wbg_finalize_init(instance, module) {
  wasm = instance.exports
  wasmModule = module
  cachedDataViewMemory0 = null
  cachedUint8ArrayMemory0 = null
  return wasm
}

async function __wbg_load(module, imports) {
  if (typeof Response === 'function' && module instanceof Response) {
    if (typeof WebAssembly.instantiateStreaming === 'function') {
      try {
        return await WebAssembly.instantiateStreaming(module, imports)
      } catch (e) {
        const validResponse = module.ok && expectedResponseType(module.type)

        if (validResponse && module.headers.get('Content-Type') !== 'application/wasm') {
          console.warn(
            '`WebAssembly.instantiateStreaming` failed because your server does not serve Wasm with `application/wasm` MIME type. Falling back to `WebAssembly.instantiate` which is slower. Original error:\n',
            e
          )
        } else {
          throw e
        }
      }
    }

    const bytes = await module.arrayBuffer()
    return await WebAssembly.instantiate(bytes, imports)
  } else {
    const instance = await WebAssembly.instantiate(module, imports)

    if (instance instanceof WebAssembly.Instance) {
      return { instance, module }
    } else {
      return instance
    }
  }

  function expectedResponseType(type) {
    switch (type) {
      case 'basic':
      case 'cors':
      case 'default':
        return true
    }
    return false
  }
}

function initSync(module) {
  if (wasm !== undefined) return wasm

  if (module !== undefined) {
    if (Object.getPrototypeOf(module) === Object.prototype) {
      ;({ module } = module)
    } else {
      console.warn('using deprecated parameters for `initSync()`; pass a single object instead')
    }
  }

  const imports = __wbg_get_imports()
  if (!(module instanceof WebAssembly.Module)) {
    module = new WebAssembly.Module(module)
  }
  const instance = new WebAssembly.Instance(module, imports)
  return __wbg_finalize_init(instance, module)
}

async function __wbg_init(module_or_path) {
  if (wasm !== undefined) return wasm

  if (module_or_path !== undefined) {
    if (Object.getPrototypeOf(module_or_path) === Object.prototype) {
      ;({ module_or_path } = module_or_path)
    } else {
      console.warn(
        'using deprecated parameters for the initialization function; pass a single object instead'
      )
    }
  }

  if (module_or_path === undefined) {
    module_or_path = new URL('orca_git_wasm_bg.wasm', import.meta.url)
  }
  const imports = __wbg_get_imports()

  if (
    typeof module_or_path === 'string' ||
    (typeof Request === 'function' && module_or_path instanceof Request) ||
    (typeof URL === 'function' && module_or_path instanceof URL)
  ) {
    module_or_path = fetch(module_or_path)
  }

  const { instance, module } = await __wbg_load(await module_or_path, imports)

  return __wbg_finalize_init(instance, module)
}

export { initSync, __wbg_init as default }
