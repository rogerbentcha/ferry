import * as React from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'

import { TooltipProvider } from '@/components/ui/tooltip'
import type { ContainerSeries, HostPoint, MetricsHistory } from '@/lib/api/types'

import { COLLECTING, ContainerCharts, HostCharts, MetricsHistoryView } from './MetricsHistory'

/** Visible text of what `node` renders (tags stripped, whitespace collapsed). */
function text(node: React.ReactNode): string {
  return renderToStaticMarkup(<TooltipProvider>{node}</TooltipProvider>)
    .replace(/<style[^]*?<\/style>/g, ' ')
    .replace(/<[^>]+>/g, ' ')
    .replace(/&#x27;/g, "'")
    .replace(/&amp;/g, '&')
    .replace(/\s+/g, ' ')
    .trim()
}

const T0 = Date.parse('2026-10-17T03:00:00Z')
const at = (seconds: number) => new Date(T0 + seconds * 1000).toISOString()

function history(host: HostPoint[], containers: ContainerSeries[] = []): MetricsHistory {
  return { range: '1h', step_seconds: 10, from: at(0), to: at(3600), host, containers }
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
