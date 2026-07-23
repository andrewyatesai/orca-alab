import type { Plugin } from 'vite'

// Why: the packaged renderer runs a privileged preload bridge (window.api.shell/fs/git/
// notebook.runPythonCell) over large volumes of attacker-influenceable content
// (react-markdown + rehypeRaw over PR/issue/MR bodies, commit messages, READMEs,
// agent/terminal output). contextIsolation holds today, but with no CSP there is zero
// second line of defense: one sanitizer regression turns `<img src=x onerror=...>` into
// renderer-privileged code. This injects a strict enforcing policy into the shipped HTML.
//
// script-src deliberately omits 'unsafe-inline' and 'unsafe-eval' so injected inline
// handlers, inline <script>, and string-to-code (eval / new Function) cannot execute.
// blob:/'wasm-unsafe-eval' cover Monaco/Vite workers and wasm without opening JS eval.
// style-src keeps 'unsafe-inline' because Tailwind/Monaco/TipTap inject inline styles.
// Dev is intentionally untouched (plugin is build-only) so Vite HMR keeps its relaxed policy.
export const RENDERER_CONTENT_SECURITY_POLICY = [
  "default-src 'self'",
  "script-src 'self' blob: 'wasm-unsafe-eval'",
  "worker-src 'self' blob:",
  "style-src 'self' 'unsafe-inline'",
  "img-src 'self' data: blob: https:",
  "font-src 'self' data:",
  "media-src 'self' data: blob:",
  "connect-src 'self' https: wss: data:",
  "frame-src 'self' https:",
  "child-src 'self' blob: https:",
  "object-src 'none'",
  "base-uri 'none'",
  "form-action 'none'"
].join('; ')

/**
 * Inserts the Content-Security-Policy `<meta>` at the top of `<head>` if not already
 * present (idempotent). A meta tag is honored regardless of load protocol, which matters
 * because the packaged renderer is served over `file://` where response-header CSP is
 * unreliable.
 */
export function injectRendererContentSecurityPolicy(html: string): string {
  if (/http-equiv=["']Content-Security-Policy["']/i.test(html)) {
    return html
  }
  const meta = `<meta http-equiv="Content-Security-Policy" content="${RENDERER_CONTENT_SECURITY_POLICY}" />`
  return html.replace(/<head>/i, `<head>\n    ${meta}`)
}

/**
 * Build-only Vite plugin. `apply: 'build'` leaves `electron-vite dev` on its relaxed HMR
 * policy (Vite's dev bootstrap needs inline script + ws), while every packaged renderer
 * HTML gets the strict enforcing policy above.
 */
export function createRendererContentSecurityPolicyPlugin(): Plugin {
  return {
    name: 'orca-renderer-content-security-policy',
    apply: 'build',
    transformIndexHtml: {
      order: 'post',
      handler(html: string): string {
        return injectRendererContentSecurityPolicy(html)
      }
    }
  }
}
