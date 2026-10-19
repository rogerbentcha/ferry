import { useTheme as useSiteTheme } from 'next-themes'
import { useSyncExternalStore } from 'react'

export type Theme = 'light' | 'dark'

const noSubscribe = () => () => {}

/**
 * Light or dark: the theme of the whole site (next-themes, from the docs' provider), so the home
 * page and the docs share one choice. It follows the system until the visitor picks one. Before
 * hydration it reads `light`: the server cannot know the visitor's theme.
 */
export function useTheme() {
  const { resolvedTheme, setTheme } = useSiteTheme()
  const mounted = useSyncExternalStore(noSubscribe, () => true, () => false)
  const theme: Theme = mounted && resolvedTheme === 'dark' ? 'dark' : 'light'
  return { theme, toggle: () => setTheme(theme === 'dark' ? 'light' : 'dark') }
}
