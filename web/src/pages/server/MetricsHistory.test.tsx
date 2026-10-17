import * as React from 'react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { renderToStaticMarkup } from 'react-dom/server'
import { MemoryRouter } from 'react-router'
import { describe, expect, it } from 'vitest'

import { TooltipProvider } from '@/components/ui/tooltip'
import { keys } from '@/lib/api/keys'
import type {
  ContainerSeries,
  HostPoint,
  MetricsHistory,
  RequestMetrics,
  RequestPoint,
  ServiceView,
} from '@/lib/api/types'

import {
  COLLECTING,
  ContainerCharts,
  HostCharts,
  MetricsHistoryView,
  NO_REQUESTS,
  NO_SERVICE_REQUESTS,
  RequestsView,
} from './MetricsHistory'

/** The services the shell knows: `srv-1` was renamed `site` since its requests. */
const SERVICES = [{ id: 'srv-1', name: 'site' }] as ServiceView[]

/** The HTML of what `node` renders (inside a router and with the service list: the tables link to pages). */
function html(node: React.ReactNode): string {
  const queryClient = new QueryClient()
  queryClient.setQueryData(keys.serviceList(), SERVICES)
  return renderToStaticMarkup(
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>
        <MemoryRouter>{node}</MemoryRouter>
      </TooltipProvider>
    </QueryClientProvider>,
  )
}

/** Visible text of what `node` renders (tags stripped, whitespace collapsed). */
function text(node: React.ReactNode): string {
  return html(node)
    .replace(/<style[^]*?<\/style>/g, ' ')
    .replace(/<[^>]+>/g, ' ')
    .replace(/&#x27;/g, "'")
    .replace(/&lt;/g, '<')
    .replace(/&amp;/g, '&')
    .replace(/\s+/g, ' ')
    .trim()
}

const T0 = Date.parse('2026-10-17T03:00:00Z')
const at = (seconds: number) => new Date(T0 + seconds * 1000).toISOString()

function history(
  host: HostPoint[],
  containers: ContainerSeries[] = [],
  requests: RequestPoint[] = [],
  request_targets: RequestMetrics[] = [],
): MetricsHistory {
  return { range: '1h', step_seconds: 10, from: at(0), to: at(3600), host, containers, requests, request_targets }
}

/** A Linux server: every value is known. */
const LINUX: HostPoint = {
  at: at(0),
  cpu_percent: 40,
  memory_used_bytes: 1024 ** 3,
  memory_total_bytes: 4 * 1024 ** 3,
  disk_read_bytes_per_sec: 1024,
  disk_write_bytes_per_sec: 2048,
  network_rx_bytes_per_sec: 4096,
  network_tx_bytes_per_sec: 512,
}

/** A Mac used for development: no /proc. */
const MAC: HostPoint = {
  at: at(0),
  cpu_percent: null,
  memory_used_bytes: null,
  memory_total_bytes: null,
  disk_read_bytes_per_sec: null,
  disk_write_bytes_per_sec: null,
  network_rx_bytes_per_sec: null,
  network_tx_bytes_per_sec: null,
}

const WEB: ContainerSeries = {
  kind: 'service',
  id: 'srv-1',
  name: 'web',
  instance: '9f8e7d',
  points: [{ at: at(0), cpu_percent: 150, memory_bytes: 300 * 1024 ** 2 }],
}

const DB: ContainerSeries = {
  kind: 'datastore',
  id: 'ds-1',
  name: 'db',
  instance: null,
  points: [{ at: at(0), cpu_percent: 2, memory_bytes: 50 * 1024 ** 2 }],
}

describe('MetricsHistoryView', () => {
  it('says data is being collected until the first reading', () => {
    const t = text(<MetricsHistoryView history={history([])} />)
    expect(t).toContain(COLLECTING)
    expect(t).not.toContain('CPU')
  })

  it('draws the host and the busiest containers', () => {
    const t = text(<MetricsHistoryView history={history([LINUX, { ...LINUX, at: at(10) }], [WEB, DB])} />)
    expect(t).not.toContain(COLLECTING)
    for (const title of ['CPU', 'Memory', 'Disk', 'Network', 'Busiest services and datastores']) {
      expect(t).toContain(title)
    }
    expect(t).not.toContain('Not available')
  })
})

describe('HostCharts', () => {
  it('shows "Not available" for what a Mac cannot read', () => {
    const t = text(<HostCharts history={history([MAC, { ...MAC, at: at(10) }])} />)
    expect(t.match(/Not available/g)).toHaveLength(4)
  })

  it('names the lines of a card with two', () => {
    const t = text(<HostCharts history={history([LINUX, { ...LINUX, at: at(10) }])} />)
    for (const label of ['Read', 'Write', 'Received', 'Sent']) {
      expect(t).toContain(label)
    }
  })
})

describe('ContainerCharts', () => {
  it('has a legend with each instance and datastore', () => {
    const t = text(<ContainerCharts history={history([LINUX], [WEB, DB])} />)
    expect(t).toContain('web · 9f8e7d')
    expect(t).toContain('db')
  })

  it('says when nothing ran in the range', () => {
    expect(text(<ContainerCharts history={history([LINUX])} />)).toContain('Nothing ran in this range')
  })
})

function point(seconds: number, ok: number, errors: number): RequestPoint {
  return {
    at: at(seconds),
    seconds: 10,
    requests_per_sec: (ok + errors) / 10,
    status_2xx: ok,
    status_3xx: 0,
    status_4xx: 0,
    status_5xx: errors,
    p50_ms: ok + errors ? 40 : null,
    p95_ms: ok + errors ? 180 : null,
  }
}

function target(target: string, name: string, over: Partial<RequestMetrics> = {}): RequestMetrics {
  return {
    target,
    name,
    requests_per_sec: 2.5,
    status_2xx: 8_100,
    status_3xx: 0,
    status_4xx: 0,
    status_5xx: 900,
    p50_ms: 40,
    p95_ms: 180,
    ...over,
  }
}

describe('RequestsView', () => {
  it('says "No requests yet" before the proxy answered any', () => {
    const t = text(<RequestsView history={history([LINUX], [], [point(0, 0, 0)])} />)
    expect(t).toContain(NO_REQUESTS)
    expect(t).not.toContain('Latency')
  })

  it('draws the requests and their latency, then a row per service, the dashboard and unrouted', () => {
    const targets = [
      target('srv-1', 'web'),
      target('srv-gone', 'old-api', { requests_per_sec: 0.5, status_2xx: 10, status_5xx: 0, p95_ms: 30 }),
      target('dashboard', 'dashboard', { requests_per_sec: 0.2, status_5xx: 0, p95_ms: 4.2 }),
      target('unrouted', 'unrouted', { requests_per_sec: 0.004, status_2xx: 0, status_4xx: 14, status_5xx: 0, p95_ms: null }),
    ]
    const view = <RequestsView history={history([LINUX], [], [point(0, 40, 10), point(10, 30, 0)], targets)} />
    const t = text(view)
    for (const title of ['Requests', 'Latency', '2xx', '3xx', '4xx', '5xx', 'p50', 'p95']) {
      expect(t).toContain(title)
    }
    expect(t).not.toContain(NO_SERVICE_REQUESTS)
    expect(t).toContain('site 2.50 req/s 9,000 requests 10.0% 900 5xx 180 ms')
    expect(t).toContain('old-api 0.50 req/s 10 requests 0.0% 0 5xx 30 ms')
    expect(t).toContain("Dashboard Ferry's own host 0.20 req/s 8,100 requests 0.0% 0 5xx 4.2 ms")
    expect(t).toContain('Unrouted Hosts no service has <0.01 req/s 14 requests 0.0% 0 5xx —')
    // The current name of a service that still exists; no link to a deleted one.
    const links = html(view).match(/href="[^"]*"/g)
    expect(links).toEqual(['href="/services/site"'])
  })

  it('says when only the dashboard or unrouted hosts got requests', () => {
    const t = text(<RequestsView history={history([LINUX], [], [point(0, 0, 0)], [target('unrouted', 'unrouted')])} />)
    expect(t.match(new RegExp(NO_SERVICE_REQUESTS, 'g'))).toHaveLength(2)
    expect(t).toContain('Unrouted')
  })
})
