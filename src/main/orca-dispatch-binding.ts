// Binds the main-process napi orcaDispatch into the shared dispatch seam, so
// src/shared modules cut over to Rust reach the core without importing napi.
// Imported for its side effect once from src/main/index.ts. The closure defers
// requireRustGitBinding() to dispatch time, so evaluating this at module load is
// safe (the addon need not be resolved yet); by the time any shared module
// dispatches, the addon — a hard dependency in main — is available.
import { setOrcaDispatchBinding } from '../shared/orca-dispatch-seam'
import { requireRustGitBinding } from './daemon/rust-git-addon'

setOrcaDispatchBinding((module, fn, inputJson) =>
  requireRustGitBinding().orcaDispatch(module, fn, inputJson)
)
