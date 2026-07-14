import os from 'node:os'
import { app, ipcMain, net } from 'electron'
import { resolveDiagnosticBuildIdentity } from '../observability/diagnostic-upload-endpoint'

// Why: the production Mac build loads the renderer from a file:// origin, so a
// cross-origin POST from fetch() triggers a CORS preflight that the feedback
// endpoint rejects. Electron's net module runs in the main process and is not
// subject to CORS, so we proxy the submission through IPC. This mirrors the
// same pattern used by updater-changelog.ts and updater-nudge.ts.

// Why fail-closed: this is a fork of public Orca. The vendor's endpoints
// (onorca.dev) must never receive fork feedback or crash reports — they would
// deliver fork-internal diagnostics to an external party's inbox. The endpoint
// is a compile-time build constant (electron-vite `define`, like
// ORCA_POSTHOG_WRITE_KEY); builds without one return a typed
// 'endpoint-not-configured' result instead of falling back to any hardcoded
// host. Module-local ambient declaration because the constant is only read here.
declare const ORCA_FEEDBACK_ENDPOINT: string | null

export const FEEDBACK_ENDPOINT_NOT_CONFIGURED = 'endpoint-not-configured'

const FEEDBACK_REQUEST_TIMEOUT_MS = 10_000
const FEEDBACK_ATTACHMENT_REQUEST_TIMEOUT_MS = 60_000
const DIAGNOSTIC_BUNDLE_CONTENT_TYPE = 'application/x-ndjson'
// Why: corporate filters can reject multipart with 403 while allowing the
// small JSON report, so content-shaped failures should shed the attachment.
const DIAGNOSTIC_BUNDLE_JSON_RETRY_STATUSES = new Set([400, 403, 408, 413, 415, 422])

function resolveBuildFeedbackEndpoint(): string | null {
  // The `globalThis` dance mirrors telemetry/client.ts: compile-time
  // substitution in production, safe undefined in vitest (which lets tests
  // inject an endpoint via `globalThis`).
  const endpoint =
    typeof ORCA_FEEDBACK_ENDPOINT !== 'undefined'
      ? ORCA_FEEDBACK_ENDPOINT
      : ((globalThis as { ORCA_FEEDBACK_ENDPOINT?: string | null }).ORCA_FEEDBACK_ENDPOINT ?? null)
  return typeof endpoint === 'string' && endpoint.length > 0 ? endpoint : null
}

export function resolveFeedbackEndpoint(): string | null {
  const buildEndpoint = resolveBuildFeedbackEndpoint()
  // Why: official builds stay pinned to the CI-substituted endpoint; user env
  // cannot redirect reports the UI labels as going to the Orca fork team.
  // Dev/contributor builds may point at a scratch server via env — the same
  // rule diagnostic-upload-endpoint.ts applies to ORCA_DIAGNOSTICS_TOKEN_URL.
  if (resolveDiagnosticBuildIdentity()) {
    return buildEndpoint
  }
  const fromEnv = process.env.ORCA_FEEDBACK_ENDPOINT
  if (fromEnv && fromEnv.length > 0) {
    return fromEnv
  }
  return buildEndpoint
}

export type FeedbackSubmissionType = 'feedback' | 'crash'

export type FeedbackSubmitArgs = {
  feedback: string
  submitAnonymously?: boolean
  githubLogin: string | null
  githubEmail: string | null
}

export type FeedbackDiagnosticBundleAttachment = {
  bundleSubmissionId: string
  content: string
  bytes: number
  spanCount: number
}

type FeedbackSubmitBody = {
  feedback: string
  submissionType: FeedbackSubmissionType
  githubLogin: string | null
  githubEmail: string | null
  appVersion: string
  platform: NodeJS.Platform
  osRelease: string
  arch: string
  diagnosticBundle?: FeedbackDiagnosticBundleAttachment
}

export type FeedbackRequestFailure = {
  status: number | null
  error: string
}

export type FeedbackSubmitResult =
  | { ok: true; diagnosticBundleFailure?: FeedbackRequestFailure }
  | ({ ok: false } & FeedbackRequestFailure & {
        diagnosticBundleFailure?: FeedbackRequestFailure
      })

type InternalFeedbackSubmitArgs = FeedbackSubmitArgs & {
  submissionType?: FeedbackSubmissionType
  diagnosticBundle?: FeedbackDiagnosticBundleAttachment
  feedbackWithoutDiagnosticBundle?: string
}

// Why: the notification and any follow-up investigation need to know which
// Orca build and which OS the feedback came from. The main process is the
// only place with trusted access to these values (app.getVersion and the
// node os module), so we enrich the payload here rather than trusting the
// renderer.
function buildSubmitBody(args: InternalFeedbackSubmitArgs): FeedbackSubmitBody {
  const identity = args.submitAnonymously
    ? { githubLogin: null, githubEmail: null }
    : { githubLogin: args.githubLogin, githubEmail: args.githubEmail }

  // Why: anonymity is an IPC-only privacy decision. Allow-list fields here so
  // stale renderer state or future identity-shaped fields cannot leak upstream.
  return {
    feedback: args.feedback,
    submissionType: args.submissionType ?? 'feedback',
    ...identity,
    appVersion: app.getVersion(),
    platform: process.platform,
    osRelease: os.release(),
    arch: process.arch,
    ...(args.submissionType === 'crash' && args.diagnosticBundle
      ? { diagnosticBundle: args.diagnosticBundle }
      : {})
  }
}

async function postFeedback(
  url: string,
  body: FeedbackSubmitBody,
  timeoutMs = FEEDBACK_REQUEST_TIMEOUT_MS
): Promise<Response> {
  const controller = new AbortController()
  // Why: a silent feedback endpoint should not leave IPC or crash-report
  // submission flows pending forever. Diagnostic attachments pass a longer
  // budget than the small JSON report path.
  const timeout = setTimeout(() => controller.abort(), timeoutMs)
  try {
    const init: RequestInit = {
      method: 'POST',
      ...feedbackRequestBodyInit(body),
      signal: controller.signal
    }
    return await net.fetch(url, init)
  } finally {
    clearTimeout(timeout)
  }
}

function feedbackRequestBodyInit(body: FeedbackSubmitBody): Pick<RequestInit, 'body' | 'headers'> {
  if (!body.diagnosticBundle) {
    return {
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body)
    }
  }

  const formData = new FormData()
  appendFeedbackFormField(formData, 'feedback', body.feedback)
  appendFeedbackFormField(formData, 'submissionType', body.submissionType)
  appendFeedbackFormField(formData, 'githubLogin', body.githubLogin)
  appendFeedbackFormField(formData, 'githubEmail', body.githubEmail)
  appendFeedbackFormField(formData, 'appVersion', body.appVersion)
  appendFeedbackFormField(formData, 'platform', body.platform)
  appendFeedbackFormField(formData, 'osRelease', body.osRelease)
  appendFeedbackFormField(formData, 'arch', body.arch)
  appendFeedbackFormField(
    formData,
    'diagnosticBundleSubmissionId',
    body.diagnosticBundle.bundleSubmissionId
  )
  appendFeedbackFormField(formData, 'diagnosticBundleBytes', String(body.diagnosticBundle.bytes))
  appendFeedbackFormField(
    formData,
    'diagnosticBundleSpanCount',
    String(body.diagnosticBundle.spanCount)
  )
  formData.append(
    'diagnosticBundleFile',
    new Blob([body.diagnosticBundle.content], { type: DIAGNOSTIC_BUNDLE_CONTENT_TYPE }),
    `orca-diagnostics-${body.diagnosticBundle.bundleSubmissionId}.ndjson`
  )

  // Why: multipart avoids JSON-escaping a near-cap NDJSON bundle over the
  // backend request limit while still submitting one feedback request.
  return { body: formData }
}

function appendFeedbackFormField(formData: FormData, key: string, value: string | null): void {
  if (value !== null) {
    formData.append(key, value)
  }
}

function messageFromError(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

function responseFailure(response: Response): FeedbackRequestFailure {
  return { status: response.status, error: `status ${response.status}` }
}

function errorFailure(error: unknown): FeedbackRequestFailure {
  return { status: null, error: messageFromError(error) }
}

function shouldRetryDiagnosticBundleAsJson(status: number): boolean {
  // Why: content-shaped rejections (filtered/oversized multipart) and server
  // errors both point at the attachment, not the report — so shed the bundle
  // and retry the small JSON report. Unlike upstream there is no vendor
  // fallback host; every retry targets the same configured endpoint.
  return DIAGNOSTIC_BUNDLE_JSON_RETRY_STATUSES.has(status) || status === 404 || status >= 500
}

async function submitFeedbackWithoutDiagnosticBundle(
  endpoint: string,
  body: FeedbackSubmitBody,
  diagnosticBundleFailure: FeedbackRequestFailure
): Promise<FeedbackSubmitResult> {
  try {
    const response = await postFeedback(endpoint, body)
    if (response.ok) {
      return { ok: true, diagnosticBundleFailure }
    }
    return { ok: false, ...responseFailure(response), diagnosticBundleFailure }
  } catch (error) {
    return { ok: false, ...errorFailure(error), diagnosticBundleFailure }
  }
}

async function submitFeedbackWithDiagnosticBundle(
  endpoint: string,
  body: FeedbackSubmitBody,
  bodyWithoutDiagnosticBundle: FeedbackSubmitBody | null
): Promise<FeedbackSubmitResult> {
  try {
    // Why: diagnostic bundles can approach 4 MiB and need more upload time than
    // the small JSON report-only path, especially on constrained connections.
    const response = await postFeedback(endpoint, body, FEEDBACK_ATTACHMENT_REQUEST_TIMEOUT_MS)
    if (response.ok) {
      return { ok: true }
    }
    const failure = responseFailure(response)
    if (bodyWithoutDiagnosticBundle && shouldRetryDiagnosticBundleAsJson(response.status)) {
      return submitFeedbackWithoutDiagnosticBundle(endpoint, bodyWithoutDiagnosticBundle, failure)
    }
    return { ok: false, ...failure }
  } catch (error) {
    // Why: an attachment upload can time out or drop on constrained links; retry
    // report-only so the crash text still lands at the same configured endpoint.
    const failure = errorFailure(error)
    return bodyWithoutDiagnosticBundle
      ? submitFeedbackWithoutDiagnosticBundle(endpoint, bodyWithoutDiagnosticBundle, failure)
      : { ok: false, ...failure }
  }
}

export async function submitFeedback(
  args: InternalFeedbackSubmitArgs
): Promise<FeedbackSubmitResult> {
  const endpoint = resolveFeedbackEndpoint()
  if (!endpoint) {
    // Fail closed, typed: the renderer surfaces this as a submission failure
    // and no bytes leave the machine. There is deliberately NO fallback host.
    return { ok: false, status: null, error: FEEDBACK_ENDPOINT_NOT_CONFIGURED }
  }
  const body = buildSubmitBody(args)
  if (body.diagnosticBundle) {
    const bodyWithoutDiagnosticBundle =
      args.feedbackWithoutDiagnosticBundle !== undefined
        ? buildSubmitBody({
            ...args,
            feedback: args.feedbackWithoutDiagnosticBundle,
            diagnosticBundle: undefined
          })
        : null
    return submitFeedbackWithDiagnosticBundle(endpoint, body, bodyWithoutDiagnosticBundle)
  }
  try {
    const res = await postFeedback(endpoint, body)
    if (res.ok) {
      return { ok: true }
    }
    return { ok: false, status: res.status, error: `status ${res.status}` }
  } catch (error) {
    return { ok: false, status: null, error: messageFromError(error) }
  }
}

export function registerFeedbackHandlers(): void {
  ipcMain.removeHandler('feedback:submit')
  ipcMain.handle('feedback:submit', (_event, args: FeedbackSubmitArgs) =>
    // Why: crash submissions are main-only. A compromised renderer can invoke
    // this channel directly, so force the public feedback lane at the boundary.
    submitFeedback({ ...args, submissionType: 'feedback' })
  )
}
