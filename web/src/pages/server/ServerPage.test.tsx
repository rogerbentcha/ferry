// @vitest-environment jsdom
import * as React from 'react'
import { createRoot } from 'react-dom/client'
import { createMemoryRouter, RouterProvider } from 'react-router'
import { describe, expect, it } from 'vitest'

import { movedSectionPath } from './lib'
import { ServerPage } from './ServerPage'

describe('movedSectionPath', () => {
  it('sends the sections that left the Server page to their own page', () => {
    expect(movedSectionPath('monitoring')).toBe('/monitoring')
    expect(movedSectionPath('domains')).toBe('/domains')
    expect(movedSectionPath('account')).toBe('/account')
  })

  it('keeps the sections still on the Server page, and the first one by default', () => {
    for (const section of ['general', 'logs', 'connections', 'api', null, '']) {
      expect(movedSectionPath(section)).toBeNull()
    }
  })

  it('does not take a property of every object for a section', () => {
    expect(movedSectionPath('constructor')).toBeNull()
    expect(movedSectionPath('toString')).toBeNull()
  })
})

describe('ServerPage', () => {
  it('redirects an old ?section= link to the page that section became', async () => {
    ;(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true
    const router = createMemoryRouter(
      [
        { path: '/server', element: <ServerPage /> },
        { path: '/account', element: <p>Account page</p> },
      ],
      { initialEntries: ['/server?section=account'] },
    )
    const node = document.createElement('div')
    const root = createRoot(node)
    await React.act(async () => {
      root.render(<RouterProvider router={router} />)
    })
    expect(router.state.location.pathname).toBe('/account')
    expect(router.state.location.search).toBe('')
    expect(node.textContent).toBe('Account page')
    React.act(() => root.unmount())
  })
})
