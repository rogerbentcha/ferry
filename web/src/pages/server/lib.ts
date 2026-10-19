/**
 * Pure helpers for the Server page: where the sections that left it now live.
 */

/**
 * Sections that became pages of their own, in the icon rail: their old links
 * (`/server?section=account` in bookmarks, older docs) still lead there.
 */
const MOVED_SECTIONS = new Map([
  ['monitoring', '/monitoring'],
  ['domains', '/domains'],
  ['account', '/account'],
])

/** The page a `?section=` of the Server page moved to; `null` when it is still a section. */
export function movedSectionPath(section: string | null): string | null {
  return section ? (MOVED_SECTIONS.get(section) ?? null) : null
}
