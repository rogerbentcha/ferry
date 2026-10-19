import type { RouteObject } from 'react-router'
import { describe, expect, it } from 'vitest'

import { routes } from '@/routes'

import { NAV_ITEMS } from './nav'

/** Every `path` of the route tree, nested ones included. */
function paths(tree: RouteObject[]): string[] {
  return tree.flatMap((r) => [...(r.path ? [r.path] : []), ...paths(r.children ?? [])])
}

describe('NAV_ITEMS', () => {
  it('has Monitoring, Domains and Account in the icon rail, each a page of its own', () => {
    const items = NAV_ITEMS.flat().map((i) => [i.label, i.to])
    expect(items).toContainEqual(['Monitoring', '/monitoring'])
    expect(items).toContainEqual(['Domains', '/domains'])
    expect(items).toContainEqual(['Account', '/account'])
  })

  it('links each item to a route of the dashboard, once', () => {
    const known = new Set(paths(routes))
    const targets = NAV_ITEMS.flat().map((i) => i.to)
    for (const to of targets) expect(known, to).toContain(to.slice(1))
    expect(new Set(targets).size).toBe(targets.length)
  })
})
