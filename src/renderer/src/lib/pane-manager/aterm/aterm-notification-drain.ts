// aterm queues OSC 9/99/777 desktop notifications behind a host-authorized,
// fail-closed engine gate (authorize_notifications) and exposes them as a JSON
// drain (take_notifications) mirroring take_osc_events. This module owns the
// JSON → typed-event decode the facade dispatches to orc's notification path.

export type AtermAppNotificationUrgency = 'low' | 'normal' | 'critical'

/** One drained OSC 9/99/777 desktop notification (engine-decoded fields). */
export type AtermAppNotification = {
  /** Notification id (OSC 99), or null — OSC 9/777 carry none. */
  id: string | null
  /** Payload title; null for OSC 9's body-only form (host supplies a fallback). */
  title: string | null
  body: string | null
  urgency: AtermAppNotificationUrgency
}

function normalizeUrgency(value: unknown): AtermAppNotificationUrgency {
  return value === 'low' || value === 'critical' ? value : 'normal'
}

function normalizeText(value: unknown): string | null {
  return typeof value === 'string' && value.length > 0 ? value : null
}

/** Decode one take_notifications() drain. Tolerant like the other side-channel
 *  decodes: a malformed payload yields [] rather than throwing into the
 *  per-chunk process path. */
export function parseAtermNotifications(json: string | undefined): AtermAppNotification[] {
  if (!json) {
    return []
  }
  let raw: unknown
  try {
    raw = JSON.parse(json)
  } catch {
    return []
  }
  if (!Array.isArray(raw)) {
    return []
  }
  const notifications: AtermAppNotification[] = []
  for (const entry of raw) {
    if (!entry || typeof entry !== 'object') {
      continue
    }
    const fields = entry as Record<string, unknown>
    const title = normalizeText(fields.title)
    const body = normalizeText(fields.body)
    // A notification with no text at all renders nothing OS-side — drop it here.
    if (!title && !body) {
      continue
    }
    notifications.push({
      id: normalizeText(fields.id),
      title,
      body,
      urgency: normalizeUrgency(fields.urgency)
    })
  }
  return notifications
}
