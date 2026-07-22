import {
  EMPTY_HISTORY,
  type HistoryState
} from '../native-chat/native-chat-composer-state'
import { setBoundedScopeCacheEntry } from '../native-chat/native-chat-composer-scope-cache'

export type ComposeBoxDraftEntry = {
  draft: string
  history: HistoryState
}

// Session-scoped only — drafts may contain secrets, so nothing is persisted to disk.
const draftCache = new Map<string, ComposeBoxDraftEntry>()

export function getComposeBoxDraftEntry(paneKey: string): ComposeBoxDraftEntry {
  return draftCache.get(paneKey) ?? { draft: '', history: EMPTY_HISTORY }
}

/** Why: unlike the chat draft cache, empty-draft entries are kept — history must survive a send, which empties the draft. */
export function setComposeBoxDraftEntry(paneKey: string, entry: ComposeBoxDraftEntry): void {
  setBoundedScopeCacheEntry(draftCache, paneKey, entry)
}

export function resetComposeBoxDraftCacheForTests(): void {
  draftCache.clear()
}
