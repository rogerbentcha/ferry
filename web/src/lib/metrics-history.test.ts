import { describe, expect, it } from 'vitest'

import type { ContainerSeries, HostPoint, MetricsHistory, RequestMetrics, RequestPoint } from '@/lib/api/types'

import {
  GAP_STEPS,
  containerLabel,
  containerRows,
  errorRate,
  formatLatency,
  formatMetric,
  formatRate,
  formatStep,
  formatTick,
  hasValues,
  hostRows,
  maxStacked,
  maxValue,
  niceTicks,
  quarterTicks,
  requestRows,
  targetLabel,
  timeTicks,
  totalRequests,
  withGaps,
} from './metrics-history'

const T0 = Date.parse('2026-10-17T03:00:00Z')
const at = (seconds: number) => new Date(T0 + seconds * 1000).toISOString()

function host(seconds: number, cpu: number | null, memory: number | null = 1024): HostPoint {
  return {
    at: at(seconds),
    cpu_percent: cpu,
    memory_used_bytes: memory,
    memory_total_bytes: 4096,
    disk_read_bytes_per_sec: null,
    disk_write_bytes_per_sec: null,
    network_rx_bytes_per_sec: 2048,
    network_tx_bytes_per_sec: 0,
  }
}

function series(name: string, instance: string | null, points: [number, number | null][]): ContainerSeries {
  return {
    kind: instance ? 'service' : 'datastore',
    id: `id-${name}`,
    name,
    instance,
    points: points.map(([seconds, cpu]) => ({ at: at(seconds), cpu_percent: cpu, memory_bytes: cpu === null ? null : cpu * 1000 })),
  }
}

function history(
  hostPoints: HostPoint[],
  containers: ContainerSeries[] = [],
  step = 10,
  requests: RequestPoint[] = [],
): MetricsHistory {
  return {
    range: '1h',
    step_seconds: step,
    from: at(0),
    to: at(3600),
    host: hostPoints,
    containers,
    requests,
    request_targets: [],
  }
}

function requests(seconds: number, ok: number, errors: number, p95: number | null, covered = 10): RequestPoint {
  return {
    at: at(seconds),
    seconds: covered,
    requests_per_sec: (ok + errors) / covered,
    status_2xx: ok,
    status_3xx: 0,
    status_4xx: 0,
    status_5xx: errors,
    p50_ms: p95 === null ? null : p95 / 2,
    p95_ms: p95,
  }
}

describe('withGaps', () => {
  const row = (seconds: number, v: number) => ({ t: T0 + seconds * 1000, v })

  it('breaks the line where ferryd read nothing for more than two steps', () => {
    // ferryd stopped for 20 minutes after 20 s.
    const rows = withGaps([row(0, 1), row(10, 2), row(20, 3), row(1220, 4), row(1230, 5)], ['v'], 10)
    expect(rows.map((r) => [(r.t - T0) / 1000, r.v])).toEqual([
      [0, 1],
      [10, 2],
      [20, 3],
      [30, null],
      [1220, 4],
      [1230, 5],
    ])
  })

  it('does not break the line for one missing step', () => {
    const rows = withGaps([row(0, 1), row(20, 2), row(20 + GAP_STEPS * 10, 3)], ['v'], 10)
    expect(rows.map((r) => r.v)).toEqual([1, 2, 3])
  })

  it('counts in steps of the range', () => {
    // 24 h: steps of 4 minutes; 8 minutes apart is still one line, 20 minutes is not.
    const rows = withGaps([row(0, 1), row(480, 2), row(1680, 3)], ['v'], 240)
    expect(rows.map((r) => r.v)).toEqual([1, 2, null, 3])
    expect(rows[2]?.t).toBe(T0 + 720 * 1000)
  })
})

describe('hostRows', () => {
  it('has one row per point, a break for each gap, and nulls for what was not read', () => {
    const rows = hostRows(history([host(0, 10), host(10, null, null), host(600, 30)]))
    expect(rows).toHaveLength(4)
    expect(rows[0]).toMatchObject({ t: T0, cpu: 10, memory: 1024, memoryTotal: 4096, networkRx: 2048, networkTx: 0 })
    expect(rows[1]).toMatchObject({ cpu: null, memory: null })
    expect(rows[2]).toEqual({
      t: T0 + 20_000,
      cpu: null,
      memory: null,
      memoryTotal: null,
      diskRead: null,
      diskWrite: null,
      networkRx: null,
      networkTx: null,
    })
    expect(rows[3]).toMatchObject({ t: T0 + 600_000, cpu: 30 })
    // macOS: no disk speed was ever read.
    expect(hasValues(rows, ['diskRead', 'diskWrite'])).toBe(false)
    expect(hasValues(rows, ['cpu'])).toBe(true)
  })

  it('is empty before the first reading', () => {
    expect(hostRows(history([]))).toEqual([])
  })
})

describe('containerRows', () => {
  const web = series('web', '9f8e7d', [
    [0, 150],
    [10, 120],
    [20, 100],
  ])
  // Started later; not measured at 20 s.
  const db = series('db', null, [
    [10, 5],
    [20, null],
  ])

  it('has one line per container, the busiest first, and nulls where one has no point', () => {
    const rows = containerRows(history([], [web, db]), 'cpu')
    expect(rows.map((r) => [(r.t - T0) / 1000, r.c0, r.c1])).toEqual([
      [0, 150, null],
      [10, 120, 5],
      [20, 100, null],
    ])
    const memory = containerRows(history([], [web, db]), 'memory')
    expect(memory.map((r) => r.c1)).toEqual([null, 5000, null])
  })

  it('breaks every line where ferryd read nothing', () => {
    const later = series('web', '9f8e7d', [
      [0, 1],
      [3000, 2],
    ])
    const rows = containerRows(history([], [later]), 'cpu')
    expect(rows.map((r) => r.c0)).toEqual([1, null, 2])
  })

  it('names a service by its instance and a datastore alone', () => {
    expect(containerLabel(web)).toBe('web · 9f8e7d')
    expect(containerLabel(db)).toBe('db')
  })
})

describe('units', () => {
  it('formats values with their unit', () => {
    expect(formatMetric(37.5, 'percent')).toBe('37.5%')
    expect(formatMetric(150, 'percent')).toBe('150.0%')
    expect(formatMetric(1.5 * 1024 ** 3, 'bytes')).toBe('1.5 GiB')
    expect(formatMetric(2 * 1024 ** 2, 'bytes_per_sec')).toBe('2.0 MiB/s')
    expect(formatMetric(0, 'bytes_per_sec')).toBe('0 B/s')
    for (const unit of ['percent', 'bytes', 'bytes_per_sec'] as const) {
      expect(formatMetric(null, unit)).toBe('—')
    }
  })

  it('keeps the axis short', () => {
    expect(formatTick(50, 'percent')).toBe('50%')
    expect(formatTick(2 * 1024 ** 2, 'bytes_per_sec')).toBe('2 MiB/s')
    expect(formatTick(1.5 * 1024 ** 3, 'bytes')).toBe('1.5 GiB')
  })

  it('says when a point is and what it averages', () => {
    const t = new Date(2026, 9, 17, 3, 4, 50).getTime()
    expect(formatStep(t, 10)).toBe('Oct 17, 03:04:50 · 10 s average')
    expect(formatStep(t, 240)).toBe('Oct 17, 03:04:50 · 4 min average')
  })
})

describe('axes', () => {
  it('puts the time ticks on round local times', () => {
    const to = new Date(2026, 9, 17, 14, 7, 30).getTime()
    const local = (t: number) => {
      const d = new Date(t)
      return `${d.getHours()}:${String(d.getMinutes()).padStart(2, '0')}`
    }
    expect(timeTicks(to - 3_600_000, to, 10).map(local)).toEqual(['13:15', '13:30', '13:45', '14:00'])
    expect(timeTicks(to - 6 * 3_600_000, to, 60).map(local)).toEqual(['9:00', '10:00', '11:00', '12:00', '13:00', '14:00'])
    expect(timeTicks(to - 24 * 3_600_000, to, 240).map(local)).toEqual(['16:00', '20:00', '0:00', '4:00', '8:00', '12:00'])
  })

  it('rounds an open value axis up to round steps', () => {
    expect(niceTicks(195, 'percent')).toEqual([0, 50, 100, 150, 200])
    expect(niceTicks(62.5, 'percent')).toEqual([0, 25, 50, 75])
    expect(niceTicks(0.4, 'percent')).toEqual([0, 1])
    const mib = 1024 ** 2
    expect(niceTicks(4.3 * mib, 'bytes_per_sec')).toEqual([0, 2 * mib, 4 * mib, 6 * mib])
    expect(niceTicks(429 * mib, 'bytes')?.map((v) => formatTick(v, 'bytes'))).toEqual([
      '0 B',
      '128 MiB',
      '256 MiB',
      '384 MiB',
      '512 MiB',
    ])
    expect(niceTicks(0, 'bytes')).toBeUndefined()
    expect(maxValue([{ t: 0, a: 1, b: null }, { t: 1, a: 3, b: 7 }], ['a'])).toBe(3)
    expect(maxValue([{ t: 0, a: null }], ['a'])).toBe(0)
  })

  it('splits a bounded value axis in quarters', () => {
    expect(quarterTicks(100)).toEqual([0, 25, 50, 75, 100])
    expect(quarterTicks(8 * 1024 ** 3).map((v) => formatTick(v, 'bytes'))).toEqual(['0 B', '2 GiB', '4 GiB', '6 GiB', '8 GiB'])
  })
})

describe('requestRows', () => {
  it('has the requests per second of each status class and the percentiles of each step, with the breaks', () => {
    // ferryd stopped for 20 minutes after 10 s.
    const points = [requests(0, 40, 10, 180), requests(10, 0, 0, null), requests(1210, 5, 0, 12)]
    const rows = requestRows(history([], [], 10, points))
    expect(rows.map((r) => [(r.t - T0) / 1000, r.s2xx, r.s5xx, r.p50, r.p95])).toEqual([
      [0, 4, 1, 90, 180],
      [10, 0, 0, null, null],
      [20, null, null, null, null],
      [1210, 0.5, 0, 6, 12],
    ])
    expect(rows[0]?.s3xx).toBe(0)
  })

  it('counts per second over the seconds a step covers', () => {
    // 24 h: steps of 4 minutes; the last one, still filling, covers 30 s so far.
    const rows = requestRows(history([], [], 240, [requests(0, 480, 0, 50, 240), requests(240, 60, 0, 50, 30)]))
    expect(rows.map((r) => r.s2xx)).toEqual([2, 2])
  })

  it('tops a stacked chart with the highest total of a row', () => {
    const rows = requestRows(history([], [], 10, [requests(0, 40, 10, 180), requests(10, 30, 30, 50)]))
    expect(maxStacked(rows, ['s2xx', 's3xx', 's4xx', 's5xx'])).toBe(6)
    expect(maxValue(rows, ['s2xx', 's3xx', 's4xx', 's5xx'])).toBe(4)
    expect(maxStacked([], ['s2xx'])).toBe(0)
  })
})

describe('request numbers', () => {
  const target = (over: Partial<RequestMetrics>): RequestMetrics => ({
    target: 'srv-1',
    name: 'web',
    requests_per_sec: 1,
    status_2xx: 0,
    status_3xx: 0,
    status_4xx: 0,
    status_5xx: 0,
    p50_ms: null,
    p95_ms: null,
    ...over,
  })

  it('formats rates and latencies', () => {
    expect([0, 0.004, 0.25, 2.5, 12.34, 345.6].map(formatRate)).toEqual([
      '0 req/s',
      '<0.01 req/s',
      '0.25 req/s',
      '2.50 req/s',
      '12.3 req/s',
      '346 req/s',
    ])
    expect(formatRate(null)).toBe('—')
    expect([4.24, 180.4, 1840, 12_500].map(formatLatency)).toEqual(['4.2 ms', '180 ms', '1.8 s', '13 s'])
    expect(formatLatency(null)).toBe('—')
    expect(formatMetric(2.5, 'requests_per_sec')).toBe('2.50 req/s')
    expect(formatMetric(1840, 'milliseconds')).toBe('1.8 s')
    expect([formatTick(0.5, 'requests_per_sec'), formatTick(250, 'milliseconds'), formatTick(2000, 'milliseconds')]).toEqual(
      ['0.5/s', '250 ms', '2 s'],
    )
  })

  it('has 1-2-5 steps on the axes of requests and milliseconds', () => {
    expect(niceTicks(0.3, 'requests_per_sec')).toEqual([0, 0.1, 0.2, 0.3])
    expect(niceTicks(7, 'requests_per_sec')).toEqual([0, 2, 4, 6, 8])
    expect(niceTicks(1900, 'milliseconds')).toEqual([0, 500, 1000, 1500, 2000])
  })

  it('counts the error rate as the share of 5xx', () => {
    const web = target({ status_2xx: 90, status_4xx: 5, status_5xx: 5 })
    expect(totalRequests(web)).toBe(100)
    expect(errorRate(web)).toBe(5)
    expect(errorRate(target({}))).toBeNull()
  })

  it('names the dashboard and the unrouted requests', () => {
    expect(targetLabel(target({}))).toBe('web')
    expect(targetLabel(target({ target: 'dashboard', name: 'dashboard' }))).toBe('Dashboard')
    expect(targetLabel(target({ target: 'unrouted', name: 'unrouted' }))).toBe('Unrouted')
  })
})
