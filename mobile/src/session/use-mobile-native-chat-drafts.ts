import { useCallback, useEffect, useRef, useState, type Dispatch, type SetStateAction } from 'react'
import type { NativeChatMessage } from '../../../src/shared/native-chat-types'

export type MobileNativeChatPendingMessage = {
  id: string
  text: string
  normalizedText: string
  // The transcript tail at send time; only echoes after it can clear this bubble.
  baselineTailMessageId: string | null
}
export type MobileNativeChatSendOrigin = {
  draftKey: string
  pendingKey: string | null
  normalizedText: string
  baselineTailMessageId: string | null
}

const NO_PENDING_MESSAGES: MobileNativeChatPendingMessage[] = []

// How long an ack-lost send waits for its transcript echo before the UI surfaces
// that delivery remains unconfirmed.
const UNCONFIRMED_SEND_DEADLINE_MS = 20_000

type UnconfirmedSend = {
  draftKey: string
  pendingKey: string | null
  text: string
  normalizedText: string
  baselineTailMessageId: string | null
  deadline: ReturnType<typeof setTimeout> | null
}

function normalizedUserText(message: NativeChatMessage): string | null {
  if (message.role !== 'user') {
    return null
  }
  const text = message.blocks
    .filter((block) => block.type === 'text')
    .map((block) => (block.type === 'text' ? block.text : ''))
    .join('')
    .trim()
  return text || null
}

// Match each entry to a NEW transcript echo: one whose index is after the entry's
// captured tail (prepended history has index <= tail) and not already claimed by
// another entry. Shared by the unconfirmed-send hold and the optimistic-pending
// clear so paged-in identical turns can never satisfy either path.
function findLandedByBaselineTail<
  T extends { normalizedText: string; baselineTailMessageId: string | null }
>(messages: readonly NativeChatMessage[], entries: readonly T[]): T[] {
  const messageIndexById = new Map<string, number>()
  const userMessagesByText = new Map<string, Array<{ id: string; index: number }>>()
  for (const [index, message] of messages.entries()) {
    messageIndexById.set(message.id, index)
    const text = normalizedUserText(message)
    if (text) {
      const current = userMessagesByText.get(text) ?? []
      current.push({ id: message.id, index })
      userMessagesByText.set(text, current)
    }
  }

  const claimedMessageIds = new Set<string>()
  const landed: T[] = []
  for (const entry of entries) {
    const tailIndex = entry.baselineTailMessageId
      ? messageIndexById.get(entry.baselineTailMessageId)
      : -1
    if (tailIndex === undefined) {
      continue
    }
    const echo = userMessagesByText
      .get(entry.normalizedText)
      ?.find((message) => message.index > tailIndex && !claimedMessageIds.has(message.id))
    if (echo) {
      claimedMessageIds.add(echo.id)
      landed.push(entry)
    }
  }
  return landed
}

export function useMobileNativeChatDrafts(args: {
  hostId: string
  worktreeId: string
  tabId: string | null
  sessionId: string | null
  messages: readonly NativeChatMessage[]
}): {
  composerText: string
  setComposerText: Dispatch<SetStateAction<string>>
  pending: MobileNativeChatPendingMessage[]
  captureSendOrigin: (text: string) => MobileNativeChatSendOrigin | null
  acceptSend: (origin: MobileNativeChatSendOrigin, text: string) => void
  holdUnconfirmedSend: (
    origin: MobileNativeChatSendOrigin,
    text: string,
    onUnconfirmed: () => void
  ) => void
} {
  const { hostId, worktreeId, tabId, sessionId, messages } = args
  const draftKey = tabId ? `${hostId}\0${worktreeId}\0${tabId}` : null
  const pendingKey = draftKey && sessionId ? `${draftKey}\0${sessionId}` : null
  const [drafts, setDrafts] = useState<Record<string, string>>({})
  const [pendingBySession, setPendingBySession] = useState<
    Record<string, MobileNativeChatPendingMessage[]>
  >({})
  const pendingCounterRef = useRef(0)
  const messagesRef = useRef(messages)
  messagesRef.current = messages
  const activeDraftKeyRef = useRef(draftKey)
  activeDraftKeyRef.current = draftKey
  const activePendingKeyRef = useRef(pendingKey)
  activePendingKeyRef.current = pendingKey
  const mountedRef = useRef(false)

  const setComposerText: Dispatch<SetStateAction<string>> = useCallback(
    (value) => {
      if (!draftKey) {
        return
      }
      setDrafts((previous) => {
        const current = previous[draftKey] ?? ''
        const next = typeof value === 'function' ? value(current) : value
        return next === current ? previous : { ...previous, [draftKey]: next }
      })
    },
    [draftKey]
  )

  const captureSendOrigin = useCallback(
    (text: string) => {
      if (!draftKey) {
        return null
      }
      const normalizedText = text.trim()
      const currentMessages = messagesRef.current
      return {
        draftKey,
        pendingKey,
        normalizedText,
        baselineTailMessageId: currentMessages[currentMessages.length - 1]?.id ?? null
      }
    },
    [draftKey, pendingKey]
  )

  const acceptSend = useCallback((origin: MobileNativeChatSendOrigin, text: string) => {
    // Why: an RPC may settle after a tab switch; mutate only the tab that
    // originated the send, without erasing edits typed after it began.
    setDrafts((previous) =>
      (previous[origin.draftKey] ?? '').trim() === text.trim()
        ? { ...previous, [origin.draftKey]: '' }
        : previous
    )
    // Why: the first prompt can be sent before the provider reports a session
    // id; clear its draft, but wait for an id before keying an optimistic echo.
    if (!origin.pendingKey) {
      return
    }
    const pendingKey = origin.pendingKey
    pendingCounterRef.current += 1
    const pendingId = `pending-${pendingCounterRef.current}`
    setPendingBySession((previous) => {
      const current = previous[pendingKey] ?? NO_PENDING_MESSAGES
      const pending: MobileNativeChatPendingMessage = {
        id: pendingId,
        text,
        normalizedText: origin.normalizedText,
        baselineTailMessageId: origin.baselineTailMessageId
      }
      return { ...previous, [pendingKey]: [...current, pending] }
    })
  }, [])

  // Why: a relay drop mid-send loses only the ack in the common case — the
  // desktop already delivered the message. Hold the send instead of claiming
  // failure (which baits a duplicate): clear the draft when the transcript echo
  // lands, and surface the uncertainty if the deadline passes without one.
  const unconfirmedRef = useRef<UnconfirmedSend[]>([])
  const holdUnconfirmedSend = useCallback(
    (origin: MobileNativeChatSendOrigin, text: string, onUnconfirmed: () => void) => {
      if (!mountedRef.current) {
        return
      }
      const isActiveTranscript =
        activeDraftKeyRef.current === origin.draftKey &&
        (origin.pendingKey === null || activePendingKeyRef.current === origin.pendingKey)
      const entry: UnconfirmedSend = {
        draftKey: origin.draftKey,
        pendingKey: origin.pendingKey,
        text,
        normalizedText: origin.normalizedText,
        baselineTailMessageId: origin.baselineTailMessageId,
        deadline: null
      }
      // Why: the transcript event can beat the lost RPC acknowledgement.
      if (isActiveTranscript && findLandedByBaselineTail(messagesRef.current, [entry]).length > 0) {
        setDrafts((previous) =>
          (previous[origin.draftKey] ?? '').trim() === text.trim()
            ? { ...previous, [origin.draftKey]: '' }
            : previous
        )
        return
      }
      entry.deadline = setTimeout(() => {
        unconfirmedRef.current = unconfirmedRef.current.filter((held) => held !== entry)
        onUnconfirmed()
      }, UNCONFIRMED_SEND_DEADLINE_MS)
      unconfirmedRef.current = [...unconfirmedRef.current, entry]
    },
    []
  )

  useEffect(() => {
    if (!draftKey || unconfirmedRef.current.length === 0) {
      return
    }
    const relevant = unconfirmedRef.current.filter(
      (entry) =>
        entry.draftKey === draftKey &&
        (entry.pendingKey === null || entry.pendingKey === pendingKey)
    )
    const landed = findLandedByBaselineTail(messages, relevant)
    if (landed.length === 0) {
      return
    }
    const landedSet = new Set(landed)
    unconfirmedRef.current = unconfirmedRef.current.filter((entry) => !landedSet.has(entry))
    for (const entry of landed) {
      if (entry.deadline !== null) {
        clearTimeout(entry.deadline)
      }
      // Same guard as acceptSend: never erase edits typed after the send began.
      setDrafts((previous) =>
        (previous[entry.draftKey] ?? '').trim() === entry.text.trim()
          ? { ...previous, [entry.draftKey]: '' }
          : previous
      )
    }
  }, [messages, draftKey, pendingKey])

  useEffect(() => {
    mountedRef.current = true
    return () => {
      mountedRef.current = false
      for (const entry of unconfirmedRef.current) {
        if (entry.deadline !== null) {
          clearTimeout(entry.deadline)
        }
      }
      unconfirmedRef.current = []
    }
  }, [])

  const pending = pendingKey
    ? (pendingBySession[pendingKey] ?? NO_PENDING_MESSAGES)
    : NO_PENDING_MESSAGES
  // Why: trigger only on a transcript change (like the unconfirmed-send hold), not
  // on `pending`. Reconciling survivors against the same window would let a second
  // effect run re-consume an echo already claimed by a bubble removed in the first.
  useEffect(() => {
    if (!pendingKey) {
      return
    }
    setPendingBySession((previous) => {
      const current = previous[pendingKey] ?? NO_PENDING_MESSAGES
      if (current.length === 0) {
        return previous
      }
      // Clear a bubble only when a NEW echo lands after its captured tail; paged-in
      // older identical turns (index <= tail) must not drop it — the real echo may
      // not have arrived yet. One echo is claimed per bubble (duplicate sends each
      // need their own). Same guard as the unconfirmed-send hold.
      const landed = findLandedByBaselineTail(messages, current)
      if (landed.length === 0) {
        return previous
      }
      const landedSet = new Set(landed)
      const next = current.filter((item) => !landedSet.has(item))
      if (next.length > 0) {
        return { ...previous, [pendingKey]: next }
      }
      const remaining = { ...previous }
      delete remaining[pendingKey]
      return remaining
    })
  }, [messages, pendingKey])

  return {
    composerText: draftKey ? (drafts[draftKey] ?? '') : '',
    setComposerText,
    pending,
    captureSendOrigin,
    acceptSend,
    holdUnconfirmedSend
  }
}
