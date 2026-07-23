import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import {
  RENDERER_CONTENT_SECURITY_POLICY,
  createRendererContentSecurityPolicyPlugin,
  injectRendererContentSecurityPolicy
} from '../../build-plugins/renderer-content-security-policy'

const repoPath = (rel) => fileURLToPath(new URL(`../../${rel}`, import.meta.url))

/** Pull one directive's source list out of the policy string. */
function directive(name) {
  const found = RENDERER_CONTENT_SECURITY_POLICY.split(';')
    .map((part) => part.trim())
    .find((part) => part === name || part.startsWith(`${name} `))
  return found ?? ''
}

describe('renderer Content-Security-Policy', () => {
  it('defines a script-src that blocks inline handlers and string-to-code', () => {
    const scriptSrc = directive('script-src')
    expect(scriptSrc).not.toBe('')
    expect(scriptSrc).toContain("'self'")
    // The whole point of the policy: an injected `<img onerror=...>` or `<script>...`
    // or eval()/new Function() must not execute even if a sanitizer regresses.
    expect(scriptSrc).not.toContain("'unsafe-inline'")
    expect(scriptSrc).not.toContain("'unsafe-eval'")
  })

  it('locks down the escalation-relevant directives', () => {
    expect(directive('default-src')).toBe("default-src 'self'")
    expect(directive('object-src')).toBe("object-src 'none'")
    expect(directive('base-uri')).toBe("base-uri 'none'")
  })

  it('injects the policy as a meta tag into both packaged renderer entries', () => {
    for (const rel of ['src/renderer/index.html', 'src/renderer/coordinator.html']) {
      const source = readFileSync(repoPath(rel), 'utf8')
      // The shipped source must NOT already carry a CSP meta (dev keeps the relaxed policy).
      expect(source).not.toMatch(/http-equiv=["']Content-Security-Policy["']/i)
      const built = injectRendererContentSecurityPolicy(source)
      expect(built).toMatch(
        /<meta http-equiv="Content-Security-Policy" content="[^"]*script-src 'self'/i
      )
      expect(built).toContain(RENDERER_CONTENT_SECURITY_POLICY)
    }
  })

  it('is idempotent so a second transform pass does not duplicate the meta', () => {
    const source = readFileSync(repoPath('src/renderer/index.html'), 'utf8')
    const once = injectRendererContentSecurityPolicy(source)
    const twice = injectRendererContentSecurityPolicy(once)
    expect(twice).toBe(once)
  })

  it('only applies to production builds so dev HMR keeps its relaxed policy', () => {
    const plugin = createRendererContentSecurityPolicyPlugin()
    expect(plugin.apply).toBe('build')
    expect(plugin.name).toBe('orca-renderer-content-security-policy')
  })

  it('no longer falsely claims electron-vite injects the production CSP', () => {
    for (const rel of ['src/renderer/index.html', 'src/renderer/coordinator.html']) {
      const source = readFileSync(repoPath(rel), 'utf8')
      expect(source).not.toMatch(/electron-vite injects a stricter policy/i)
    }
  })
})
