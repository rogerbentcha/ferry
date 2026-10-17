import * as React from 'react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { renderToStaticMarkup } from 'react-dom/server'
import { MemoryRouter } from 'react-router'
import { describe, expect, it } from 'vitest'

import { TooltipProvider } from '@/components/ui/tooltip'
import type { ContainerMetrics, HostMetrics, ServerMetrics } from '@/lib/api/types'

import { ContainersTable, HostCards, MonitoringView, NOT_AVAILABLE } from './MonitoringSection'

function html(node: React.ReactNode): string {
  return renderToStaticMarkup(
    <QueryClientProvider client={new QueryClient()}>
      <MemoryRouter>
        <TooltipProvider>{node}</TooltipProvider>
      </MemoryRouter>
    </QueryClientProvider>,
  )
}

/** Visible text of what `node` renders (tags stripped, whitespace collapsed). */
function text(node: React.ReactNode): string {
  return html(node)
    .replace(/<[^>]+>/g, ' ')
    .replace(/&#x27;/g, "'")
    .replace(/&amp;/g, '&')
    .replace(/\s+/g, ' ')
    .trim()
}

const GIB = 1024 ** 3
const MIB = 1024 ** 2

/** A Linux server: every value is known. */
const LINUX: HostMetrics = {
  cpu_percent: 37.5,
  cpu_count: 4,
  memory_used_bytes: 3 * GIB,
  memory_total_bytes: 8 * GIB,
  disks: [
    { mount_point: '/', used_bytes: 40 * GIB, total_bytes: 100 * GIB, read_bytes_per_sec: MIB, write_bytes_per_sec: 512 * 1024 },
    { mount_point: '/var/lib/docker', used_bytes: 10 * GIB, total_bytes: 50 * GIB, read_bytes_per_sec: 0, write_bytes_per_sec: 0 },
  ],
  network_rx_bytes_per_sec: 2 * MIB,
  network_tx_bytes_per_sec: 300 * 1024,
}

/** A Mac used for development: no /proc. */
const MAC: HostMetrics = {
  cpu_percent: null,
  cpu_count: 10,
  memory_used_bytes: null,
  memory_total_bytes: null,
  disks: [{ mount_point: '/System/Volumes/Data', used_bytes: 200 * GIB, total_bytes: 500 * GIB, read_bytes_per_sec: null, write_bytes_per_sec: null }],
  network_rx_bytes_per_sec: null,
  network_tx_bytes_per_sec: null,
}

const WEB: ContainerMetrics = {
  kind: 'service',
  id: 'srv-1',
  name: 'web',
  instance: '9f8e7d',
  cpu_percent: 120,
  memory_bytes: 300 * MIB,
  memory_limit_bytes: 512 * MIB,
  cpu_limit: 2,
}

const DB: ContainerMetrics = {
  kind: 'datastore',
  id: 'ds-1',
  name: 'db',
  instance: null,
  cpu_percent: 0.5,
  memory_bytes: 64 * MIB,
  memory_limit_bytes: null,
  cpu_limit: null,
}

/** A container that did not answer Docker in time. */
const STUCK: ContainerMetrics = { ...WEB, id: 'srv-2', name: 'worker', instance: 'a1b2c3', cpu_percent: null, memory_bytes: null }

describe('HostCards', () => {
  it('shows the CPU, memory, disks and network of the machine', () => {
    const out = text(<HostCards host={LINUX} />)
    expect(out).toContain('CPU 37.5% of 4 CPUs')
    expect(out).toContain('Memory 3.0 GiB / 8.0 GiB')
    expect(out).toContain('Disk / 40.0 GiB / 100 GiB')
    expect(out).toContain('Read 1.0 MiB/s Write 512 KiB/s')
    expect(out).toContain('Disk /var/lib/docker 10.0 GiB / 50.0 GiB')
    expect(out).toContain('Received 2.0 MiB/s Sent 300 KiB/s')
    expect(out).not.toContain(NOT_AVAILABLE)
    // usage bars: 37.5% CPU, 37.5% memory, 40% and 20% disk
    const meters = [...html(<HostCards host={LINUX} />).matchAll(/aria-valuenow="(\d+)"/g)].map((m) => m[1])
    expect(meters).toEqual(['38', '38', '40', '20'])
  })

  it('says "Not available" for what the machine does not tell', () => {
    const out = text(<HostCards host={MAC} />)
    expect(out).toContain(`CPU ${NOT_AVAILABLE} 10 CPUs`)
    expect(out).toContain(`Memory ${NOT_AVAILABLE}`)
    expect(out).toContain(`Network ${NOT_AVAILABLE}`)
    // the space of the disk is known, its speed is not
    expect(out).toContain('Disk /System/Volumes/Data 200 GiB / 500 GiB')
    expect(out).toContain(`Read and write speed: ${NOT_AVAILABLE}`)
    expect(out).not.toContain('NaN')

    const noDisk = text(<HostCards host={{ ...MAC, disks: [] }} />)
    expect(noDisk).toContain(`Disk ${NOT_AVAILABLE}`)
  })
})

describe('ContainersTable', () => {
  it('lists each container with its CPU and memory, in the order given, linked to its page', () => {
    const page = html(<ContainersTable containers={[WEB, DB]} />)
    const out = text(<ContainersTable containers={[WEB, DB]} />)
    expect(out).toContain('web 9f8e7d 120.0% limit 2 CPUs 300 MiB limit 512 MiB')
    expect(out).toContain('db datastore 0.5% no limit 64.0 MiB no limit')
    expect(out.indexOf('web 9f8e7d')).toBeLessThan(out.indexOf('db datastore'))
    expect(page).toContain('href="/services/web"')
    expect(page).toContain('href="/datastores/db"')
  })

  it('shows a dash for a container that did not answer', () => {
    const out = text(<ContainersTable containers={[STUCK]} />)
    expect(out).toContain('worker a1b2c3 — limit 2 CPUs — limit 512 MiB')
  })

  it('says when nothing runs, and when Docker did not answer', () => {
    const empty = text(<ContainersTable containers={[]} />)
    expect(empty).toContain('Nothing is running')
    expect(html(<ContainersTable containers={[]} />)).not.toContain('<table')

    const noDocker = text(<ContainersTable containers={null} />)
    expect(noDocker).toContain(NOT_AVAILABLE)
    expect(noDocker).toContain('Docker did not answer')
  })
})

describe('MonitoringView', () => {
  it('shows the machine, then its containers', () => {
    const metrics: ServerMetrics = { at: '2026-10-17T12:00:00Z', host: LINUX, containers: [WEB] }
    const out = text(<MonitoringView metrics={metrics} />)
    expect(out.indexOf('This server')).toBeLessThan(out.indexOf('Services and datastores'))
    expect(out).toContain('web 9f8e7d')
  })
})
