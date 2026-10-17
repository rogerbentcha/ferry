// @vitest-environment jsdom
import * as React from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { renderToStaticMarkup } from 'react-dom/server'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { LogRow } from '@/components/patterns/LogViewer'
import { TooltipProvider } from '@/components/ui/tooltip'
import { streamPaths } from '@/lib/api/endpoints'
import { useLogStream } from '@/lib/api/useLogStream'
import type { LogViewerLine } from '@/lib/logs'
import { parseServerLogEvent } from '@/lib/server-logs'
import { useAuth } from '@/stores/auth'

import { ServerLogsSection } from './ServerLogsSection'

const line = (level: 'error' | 'warn' | 'info' | 'debug', message: string, fields: Record<string, string> = {}) =>
  `event: log\ndata: ${JSON.stringify({ ts: '2026-10-17T12:00:00.250Z', level, target: 'ferry_engine', message, fields })}\n\n`

const SERVER_LOG =
  line('info', 'engine started') +
  line('warn', 'certificate request failed', { host: 'app.example.com' }) +
  'event: skipped\ndata: {"count":1500}\n\n' +
  line('error', 'push from github.com/acme/web: deploy failed') +
  line('debug', 'connected to Docker')

/** A response whose SSE body sends `text`, then stays open (a follower) until the request is aborted. */
function sse(text: string, signal: AbortSignal | null | undefined): Response {
  const body = new ReadableStream<Uint8Array>({
    start(controller) {
      controller.enqueue(new TextEncoder().encode(text))
      signal?.addEventListener('abort', () => controller.error(new DOMException('Aborted', 'AbortError')))
    },
  })
  return new Response(body, { status: 200, headers: { 'content-type': 'text/event-stream' } })
}

let requests: string[] = []
let root: Root | null = null
const node = () => document.body.firstElementChild as HTMLElement

/** Mount `ui` and let the stream deliver what it has. */
async function mount(ui: React.ReactNode) {
  const el = document.createElement('div')
  document.body.replaceChildren(el)
  root = createRoot(el)
  await React.act(async () => root?.render(<TooltipProvider>{ui}</TooltipProvider>))
  await settle()
}

/** Wait for the fetch, the stream and the animation frame that flushes the lines. */
async function settle() {
  for (let i = 0; i < 5; i++) {
    await React.act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 20))
    })
  }
}

function heights(height: number) {
  // jsdom lays nothing out: give the log region a size, so the virtual list renders rows.
  vi.spyOn(HTMLElement.prototype, 'offsetHeight', 'get').mockReturnValue(height)
  vi.spyOn(HTMLElement.prototype, 'offsetWidth', 'get').mockReturnValue(800)
}

beforeEach(() => {
  ;(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true
  useAuth.setState({ phase: 'signed-in', user: { id: 'usr-1', email: 'ops@example.com' } as never })
  requests = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, init?: RequestInit) => {
      requests.push(String(url))
      return sse(SERVER_LOG, init?.signal)
    }),
  )
  heights(600)
})

afterEach(() => {
  React.act(() => root?.unmount())
  root = null
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
})

describe('ServerLogsSection', () => {
  it('follows the server log: each level in its color, the skipped lines, a copy button per line', async () => {
    await mount(<ServerLogsSection />)
    expect(requests).toEqual([`${streamPaths.serverLogs}?level=debug&follow=true`])

    const out = node().textContent ?? ''
    expect(out).toContain('Server logs')
    expect(out).toContain('Live')
    expect(out).toContain('engine started')
    expect(out).toContain('certificate request failed host=app.example.com')
    expect(out).toContain('1,500 lines skipped: this page fell behind the server')

    const level = (l: string) => node().querySelector(`[data-level="${l}"] span[title="ferry_engine"]`)
    expect(level('error')?.textContent).toBe('ERROR')
    expect(level('error')?.className).toContain('text-destructive')
    expect(level('warn')?.className).toContain('text-warning')
    expect(level('info')?.className).toContain('text-info')
    expect(level('debug')?.className).toContain('text-foreground-lighter')
    expect(node().querySelectorAll('button[aria-label="Copy line"]')).toHaveLength(4)
    // Ferry's own lines: nothing to download (out of scope), no instance column.
    expect(node().querySelector('button[aria-label="Download logs"]')).toBeNull()
  })

  it('keeps the lines that mention the search, and the markers', async () => {
    await mount(<ServerLogsSection />)
    const search = node().querySelector<HTMLInputElement>('input[placeholder="Search logs…"]')
    expect(search).not.toBeNull()
    const setValue = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set
    await React.act(async () => {
      setValue?.call(search, 'GitHub')
      search?.dispatchEvent(new Event('input', { bubbles: true }))
    })
    const rows = [...node().querySelectorAll('[data-index]')].map((r) => r.textContent ?? '')
    expect(rows).toHaveLength(2)
    expect(rows[0]).toContain('1,500 lines skipped')
    expect(rows[1]).toContain('push from github.com/acme/web: deploy failed')
  })

  it('stops following when the reader scrolls up, until "Jump to latest"', async () => {
    await mount(<ServerLogsSection />)
    const log = node().querySelector<HTMLElement>('[role="log"]')
    expect(log).not.toBeNull()
    expect(node().textContent).not.toContain('Jump to latest')
    if (!log) return
    Object.defineProperty(log, 'scrollHeight', { configurable: true, value: 2000 })
    Object.defineProperty(log, 'clientHeight', { configurable: true, value: 600 })
    await React.act(async () => {
      log.scrollTop = 100
      log.dispatchEvent(new Event('scroll'))
    })
    expect(node().textContent).toContain('Jump to latest')
  })
})

describe('useLogStream with the server log', () => {
  function Probe({ level }: { level: string }) {
    const stream = useLogStream(streamPaths.serverLogs, { query: { level }, parse: parseServerLogEvent })
    return <p>{stream.lines.map((l) => l.text).join(' | ')}</p>
  }

  it('asks again when the level changes, and only then', async () => {
    await mount(<Probe level="debug" />)
    expect(node().textContent).toContain('engine started | certificate request failed')
    // Rendering again with the same level keeps the stream open.
    await React.act(async () => root?.render(<TooltipProvider><Probe level="debug" /></TooltipProvider>))
    await settle()
    expect(requests).toHaveLength(1)
    await React.act(async () => root?.render(<TooltipProvider><Probe level="warn" /></TooltipProvider>))
    await settle()
    expect(requests).toEqual([
      `${streamPaths.serverLogs}?level=debug&follow=true`,
      `${streamPaths.serverLogs}?level=warn&follow=true`,
    ])
  })
})

describe('LogRow', () => {
  const html = (line: LogViewerLine, copy = true) =>
    renderToStaticMarkup(
      <TooltipProvider>
        <LogRow line={line} needle="" wrap timestamps={false} instance={false} level copy={copy} />
      </TooltipProvider>,
    )

  it('marks a warning in the gutter and an error with a tinted row', () => {
    const warn = html({ seq: 1, ts: 't', text: 'no certificate', level: 'warn', target: 'ferry_tls' })
    expect(warn).toContain('shadow-[inset_2px_0_0_var(--warning)]')
    expect(warn).toContain('title="ferry_tls"')
    const error = html({ seq: 2, ts: 't', text: 'proxy stopped', level: 'error' })
    expect(error).toContain('bg-destructive/[0.06]')
    expect(error).toContain('text-destructive')
  })

  it('has a copy button only when asked', () => {
    expect(html({ seq: 1, ts: 't', text: 'x', level: 'info' })).toContain('aria-label="Copy line"')
    expect(html({ seq: 1, ts: 't', text: 'x', level: 'info' }, false)).not.toContain('Copy line')
  })

  it('draws a marker without a level or a copy button', () => {
    const marker = html({ seq: 3, ts: 't', text: '20 lines skipped: this page fell behind the server', skipped: 20 })
    expect(marker).toContain('data-skipped="20"')
    expect(marker).toContain('20 lines skipped')
    expect(marker).not.toContain('Copy line')
  })
})
