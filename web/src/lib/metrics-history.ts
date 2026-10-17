/**
 * The rows the history charts of Server → Monitoring draw, from `GET /api/v1/server/metrics/history`:
 * one row per step, with a break where ferryd read nothing (it was stopped), and the values' units.
 */
import type { ContainerSeries, HostPoint, MetricsHistory, MetricsRange } from '@/lib/api/types'
import { bytes, percent } from '@/lib/format'

/** The ranges of the picker, the default first. */
export const METRICS_RANGES: { value: MetricsRange; label: string }[] = [
  { value: '1h', label: '1 h' },
  { value: '6h', label: '6 h' },
  { value: '24h', label: '24 h' },
]

/**
 * Two points further apart than this many steps have a break between them. One missing step (a
 * reading that failed or fell into the next step) does not break the line; ferryd stopped for
 * longer does.
 */
export const GAP_STEPS = 2

/** How a chart's values read. */
export type MetricUnit = 'percent' | 'bytes' | 'bytes_per_sec'

/** `37.5%`, `1.5 GiB`, `2.0 MiB/s`; `—` when unknown. */
export function formatMetric(value: number | null | undefined, unit: MetricUnit): string {
  switch (unit) {
    case 'percent':
      return percent(value)
    case 'bytes':
      return bytes(value)
    case 'bytes_per_sec':
      return value === null || value === undefined ? bytes(value) : `${bytes(value)}/s`
  }
}

/** An axis tick: `37%`, `1.5 GiB`, `2 MiB/s`. */
export function formatTick(value: number, unit: MetricUnit): string {
  return unit === 'percent' ? percent(value, 0) : formatMetric(value, unit).replace(/\.0 /, ' ')
}

const pad = (n: number) => String(n).padStart(2, '0')

/** An axis tick of time: `14:05` (local time). */
export function formatClock(ms: number): string {
  const d = new Date(ms)
  return `${pad(d.getHours())}:${pad(d.getMinutes())}`
}

/** The time between two ticks of the time axis, by step: every 15 min (1 h), hour (6 h) or 4 hours (24 h). */
function tickEvery(stepSeconds: number): number {
  return stepSeconds <= 10 ? 15 * 60_000 : stepSeconds <= 60 ? 3_600_000 : 4 * 3_600_000
}

/** The ticks of the time axis from `from` to `to` (ms): round local times (`14:00`, `14:15`…). */
export function timeTicks(from: number, to: number, stepSeconds: number): number[] {
  const every = tickEvery(stepSeconds)
  // Local time = UTC - offset; ticks fall on local multiples of `every`.
  const offset = new Date(from).getTimezoneOffset() * 60_000
  const ticks: number[] = []
  for (let t = Math.ceil((from - offset) / every) * every + offset; t <= to; t += every) ticks.push(t)
  return ticks
}

/** The ticks of a value axis that goes up to `max`: its quarters. */
export function quarterTicks(max: number): number[] {
  return [0, max / 4, max / 2, (max * 3) / 4, max]
}

/**
 * The ticks of a value axis from 0 to at least `max`, 4 steps at most: round percents (25%, 50%…)
 * or powers of two of bytes (128 MiB, 256 MiB…). `undefined` when there is no value above 0.
 */
export function niceTicks(max: number, unit: MetricUnit): number[] | undefined {
  if (!(max > 0) || !Number.isFinite(max)) return undefined
  const steps =
    unit === 'percent'
      ? [1, 2, 5, 10, 25, 50, 100, 200, 400, 800, 1600, 3200]
      : Array.from({ length: 51 }, (_, k) => 2 ** k)
  const step = steps.find((s) => Math.ceil(max / s) <= 4) ?? max / 4
  return Array.from({ length: Math.ceil(max / step) + 1 }, (_, i) => i * step)
}

/** The highest value of `keys` in the rows (0 when none). */
export function maxValue(rows: ChartRow[], keys: readonly string[]): number {
  let max = 0
  for (const row of rows) {
    for (const key of keys) {
      const v = row[key]
      if (typeof v === 'number' && v > max) max = v
    }
  }
  return max
}

/** The time of a tooltip: `Oct 17, 14:05:30` (local time), and the length of the step it averages. */
export function formatStep(ms: number, stepSeconds: number): string {
  const d = new Date(ms)
  const day = d.toLocaleString('en-US', { month: 'short', day: 'numeric' })
  const time = `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`
  const step = stepSeconds < 60 ? `${stepSeconds} s` : `${stepSeconds / 60} min`
  return `${day}, ${time} · ${step} average`
}

/** A row of a chart: its time (ms since the epoch) and one value per line; `null` breaks the lines. */
export type ChartRow = { t: number } & Record<string, number | null>

/**
 * `rows` (sorted by time) with a row of `null`s inserted where two rows are more than
 * `GAP_STEPS` steps apart, one step after the first: the lines stop there instead of joining the
 * readings on both sides of the time ferryd was stopped.
 */
export function withGaps(rows: ChartRow[], keys: string[], stepSeconds: number): ChartRow[] {
  const step = stepSeconds * 1000
  const out: ChartRow[] = []
  for (const row of rows) {
    const previous = out[out.length - 1]
    if (previous && row.t - previous.t > GAP_STEPS * step) {
      const gap: ChartRow = { t: previous.t + step }
      for (const key of keys) gap[key] = null
      out.push(gap)
    }
    out.push(row)
  }
  return out
}

/** The values of the host charts, by line. */
export const HOST_KEYS = ['cpu', 'memory', 'memoryTotal', 'diskRead', 'diskWrite', 'networkRx', 'networkTx'] as const

function hostRow(p: HostPoint): ChartRow {
  return {
    t: Date.parse(p.at),
    cpu: p.cpu_percent,
    memory: p.memory_used_bytes,
    memoryTotal: p.memory_total_bytes,
    diskRead: p.disk_read_bytes_per_sec,
    diskWrite: p.disk_write_bytes_per_sec,
    networkRx: p.network_rx_bytes_per_sec,
    networkTx: p.network_tx_bytes_per_sec,
  }
}

/** The host's rows, with the breaks. */
export function hostRows(history: MetricsHistory): ChartRow[] {
  return withGaps(history.host.map(hostRow), [...HOST_KEYS], history.step_seconds)
}

/** The line of a container in the charts: `c0` for the busiest, `c1`…; a CSS-safe name for its color. */
export function containerKey(index: number): string {
  return `c${index}`
}

/** `web · 9f8e7d` for a service instance, `db` for a datastore. */
export function containerLabel(c: ContainerSeries): string {
  return c.instance ? `${c.name} · ${c.instance}` : c.name
}

/**
 * The rows of a container chart: one per step any of them has a point at, one line per container
 * (`containerKey`). A container without a point at a step (not running, or not measured) has `null`
 * there.
 */
export function containerRows(history: MetricsHistory, metric: 'cpu' | 'memory'): ChartRow[] {
  const keys = history.containers.map((_, i) => containerKey(i))
  const byTime = new Map<number, ChartRow>()
  history.containers.forEach((series, i) => {
    const key = containerKey(i)
    for (const p of series.points) {
      const t = Date.parse(p.at)
      let row = byTime.get(t)
      if (!row) {
        row = { t }
        for (const key of keys) row[key] = null
        byTime.set(t, row)
      }
      row[key] = metric === 'cpu' ? p.cpu_percent : p.memory_bytes
    }
  })
  const rows = [...byTime.values()].sort((a, b) => a.t - b.t)
  return withGaps(rows, keys, history.step_seconds)
}

/** Whether a value of `keys` is known in any row (a chart of nothing says "Not available"). */
export function hasValues(rows: ChartRow[], keys: readonly string[]): boolean {
  return rows.some((row) => keys.some((key) => row[key] !== null && row[key] !== undefined))
}
