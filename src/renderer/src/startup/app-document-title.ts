import type { AppIdentity } from '../../../shared/app-identity'

type IdentityReader = () => Promise<Pick<AppIdentity, 'name'>>
type TitleTarget = Pick<Document, 'title'>

/** Keep the page/window title aligned with the runtime identity instead of the
 * static renderer HTML fallback. A failed identity read leaves that fallback
 * intact so startup never produces an unhandled rejection or a blank title. */
export async function applyAppDocumentTitle(
  readIdentity: IdentityReader,
  target: TitleTarget
): Promise<boolean> {
  try {
    const identity = await readIdentity()
    const name = identity.name.trim()
    if (!name) {
      return false
    }
    target.title = name
    return true
  } catch {
    return false
  }
}
