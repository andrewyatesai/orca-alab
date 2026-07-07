// TS dispatch for the workspace-name parity module. The shared TS derivation
// was DELETED (the Rust orca-text core is the sole impl — the renderer drives
// it via wasm), so this adapter drives the same wasm: the vectors' recorded
// goldens now pin that surface absolutely, and the harness's TS-vs-Rust diff
// degenerates to wasm-vs-binary (drift between the two Rust entry points would
// still surface here).
import { gitWasmOracle } from './orca-git-wasm-oracle'

export function dispatch(fn: string, input: unknown): unknown {
  const wasm = gitWasmOracle()
  switch (fn) {
    case 'slugifyForWorkspaceName':
      return wasm.slugifyForWorkspaceName(input as string)
    case 'getLinkedWorkItemSuggestedName':
      return wasm.getLinkedWorkItemSuggestedName((input as { title: string }).title)
    case 'getWorkspaceIntentName': {
      // Struct in/out crosses the wasm boundary as JSON; null models "no seed".
      const json = wasm.getWorkspaceIntentName(JSON.stringify(input))
      return json === undefined ? null : JSON.parse(json)
    }
    case 'getLinkedWorkItemWorkspaceName': {
      const json = wasm.getLinkedWorkItemWorkspaceName(JSON.stringify(input))
      return json === undefined ? null : JSON.parse(json)
    }
    case 'getLinearIssueWorkspaceName': {
      const { identifier, title } = input as { identifier: string; title: string }
      return wasm.getLinearIssueWorkspaceName(identifier, title)
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
