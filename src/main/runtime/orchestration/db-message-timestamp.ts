import type { MessageRow } from './types'

// Why: SQLite stores UTC as timezone-less space format for SQL ordering, but
// RPC/CLI consumers need an explicit offset (#9167). The Rust store returns the
// rows as written; this module owns the RFC3339 exposure at the JSON boundary.
const SQLITE_UTC_TIMESTAMP_RE = /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}(?:\.\d+)?$/

function exposeUtcTimestamp(timestamp: string | null): string | null {
  if (!timestamp || !SQLITE_UTC_TIMESTAMP_RE.test(timestamp)) {
    return timestamp
  }
  return `${timestamp.replace(' ', 'T')}Z`
}

export function exposeMessageTimestamps(message: MessageRow): MessageRow {
  return {
    ...message,
    created_at: exposeUtcTimestamp(message.created_at) ?? message.created_at,
    delivered_at: exposeUtcTimestamp(message.delivered_at)
  }
}

export function messageRowFromJson(json: string): MessageRow {
  return exposeMessageTimestamps(JSON.parse(json) as MessageRow)
}

export function optionalMessageRowFromJson(json: string | null): MessageRow | undefined {
  return json === null ? undefined : messageRowFromJson(json)
}

export function messageListFromJson(json: string): MessageRow[] {
  return (JSON.parse(json) as MessageRow[]).map(exposeMessageTimestamps)
}
