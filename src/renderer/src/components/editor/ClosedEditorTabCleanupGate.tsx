import { useAppStore } from '@/store'
import { useClosedEditorTabCleanup } from './useClosedEditorTabCleanup'

// Why: tab-close cleanup (Monaco model disposal, scroll/cursor cache eviction)
// must observe closes from an always-mounted host — EditorPanel unmounts when
// its last tab closes, before its own cleanup effect could see the close. A
// leaf gate keeps the openFiles subscription from re-rendering the App tree.
export default function ClosedEditorTabCleanupGate(): null {
  const openFiles = useAppStore((s) => s.openFiles)
  useClosedEditorTabCleanup(openFiles)
  return null
}
