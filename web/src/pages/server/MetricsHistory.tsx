import * as React from 'react'
import { AlertTriangle, LineChart as LineChartIcon } from 'lucide-react'
import { CartesianGrid, Line, LineChart, XAxis, YAxis } from 'recharts'

import { Callout, EmptyState, ErrorState } from '@/components/patterns/EmptyState'
import { MetricCard } from '@/components/patterns/MetricCard'
import { PageSection } from '@/components/patterns/Page'
import { type ChartConfig, ChartContainer, ChartTooltip, ChartTooltipContent } from '@/components/ui/chart'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { errorMessage } from '@/lib/api/client'
import { useMetricsHistory } from '@/lib/api/queries'
import type { MetricsHistory, MetricsRange } from '@/lib/api/types'
import {
  type ChartRow,
  METRICS_RANGES,
  type MetricUnit,
  containerKey,
  containerLabel,
  containerRows,
  formatClock,
  formatMetric,
  formatStep,
  formatTick,
  hasValues,
  hostRows,
  maxValue,
  niceTicks,
  quarterTicks,
  timeTicks,
} from '@/lib/metrics-history'

/** What a value ferryd can't read shows (the same words as the live cards). */
const NOT_AVAILABLE = 'Not available'

/** Shown before the first reading is stored. */
export const COLLECTING = 'Collecting data… first points in about 10 s'

/** The color of the `i`-th line of a chart (the busiest container first): `--chart-1` to `--chart-5`. */
function color(i: number): string {
  return `var(--chart-${(i % 5) + 1})`
}

interface ChartLine {
  key: string
  label: string
  color: string
}

interface HistoryChartProps {
  title: string
  info: string
  rows: ChartRow[]
  lines: ChartLine[]
  unit: MetricUnit
  history: MetricsHistory
  /** The top of the value axis (default: the highest value). */
  max?: number
  /** A legend in the header (the lines of one card). */
  legend?: boolean
}

/** One line chart of the history, in a metric card. */
function HistoryChart({ title, info, rows, lines, unit, history, max, legend = false }: HistoryChartProps) {
  const config = Object.fromEntries(lines.map((l) => [l.key, { label: l.label, color: l.color }])) satisfies ChartConfig
  const known = hasValues(
    rows,
    lines.map((l) => l.key),
  )
  const [from, to] = [Date.parse(history.from), Date.parse(history.to)]
  const valueTicks =
    max === undefined
      ? niceTicks(
          maxValue(
            rows,
            lines.map((l) => l.key),
          ),
          unit,
        )
      : quarterTicks(max)
  const top = valueTicks?.[valueTicks.length - 1]
  return (
    <MetricCard
      label={title}
      info={info}
      aside={legend && known ? lines.map((l) => <LegendItem key={l.key} color={l.color} label={l.label} />) : undefined}
    >
      {known ? (
        <ChartContainer config={config} className="aspect-auto h-44 w-full" aria-label={`${title} over time`}>
          <LineChart data={rows} margin={{ top: 4, right: 4, bottom: 0, left: 0 }}>
            <CartesianGrid vertical={false} />
            <XAxis
              dataKey="t"
              type="number"
              scale="time"
              domain={[from, to]}
              ticks={timeTicks(from, to, history.step_seconds)}
              tickFormatter={formatClock}
              tickLine={false}
              axisLine={false}
              minTickGap={40}
              tickMargin={6}
            />
            <YAxis
              domain={[0, top ?? 'auto']}
              ticks={valueTicks}
              tickFormatter={(v: number) => formatTick(v, unit)}
              tickLine={false}
              axisLine={false}
              width={64}
              allowDataOverflow={max !== undefined}
            />
            <ChartTooltip
              isAnimationActive={false}
              content={
                <ChartTooltipContent
                  className="min-w-44"
                  labelFormatter={(_, payload) => {
                    const t = payload?.[0]?.payload?.t
                    return typeof t === 'number' ? formatStep(t, history.step_seconds) : null
                  }}
                  formatter={(value, name, item) => (
                    <TooltipRow
                      color={item.color ?? color(0)}
                      label={config[String(name)]?.label ?? String(name)}
                      value={formatMetric(typeof value === 'number' ? value : null, unit)}
                    />
                  )}
                />
              }
            />
            {lines.map((l) => (
              <Line
                key={l.key}
                dataKey={l.key}
                name={l.key}
                type="linear"
                stroke={`var(--color-${l.key})`}
                strokeWidth={1.5}
                dot={false}
                activeDot={{ r: 3 }}
                connectNulls={false}
                isAnimationActive={false}
              />
            ))}
          </LineChart>
        </ChartContainer>
      ) : (
        <p className="flex h-44 items-center justify-center text-[13px] text-foreground-lighter">{NOT_AVAILABLE}</p>
      )}
    </MetricCard>
  )
}

/** A line of the tooltip: its color, its name, its value with the unit. */
function TooltipRow({ color, label, value }: { color: string; label: React.ReactNode; value: string }) {
  return (
    <div className="flex w-full items-center gap-2">
      <span aria-hidden="true" className="size-2 shrink-0 rounded-[2px]" style={{ background: color }} />
      <span className="min-w-0 flex-1 truncate text-foreground-lighter">{label}</span>
      <span className="font-mono font-medium text-foreground tabular-nums">{value}</span>
    </div>
  )
}

/** `■ web · 9f8e7d` */
function LegendItem({ color, label }: { color: string; label: string }) {
  return (
    <span className="inline-flex min-w-0 items-center gap-1.5 text-[12px] text-foreground-light">
      <span aria-hidden="true" className="size-2 shrink-0 rounded-[2px]" style={{ background: color }} />
      <span className="truncate">{label}</span>
    </span>
  )
}

/** The machine: CPU, memory, disks, network. */
export function HostCharts({ history }: { history: MetricsHistory }) {
  const rows = hostRows(history)
  const totals = rows.map((r) => r.memoryTotal).filter((v): v is number => typeof v === 'number')
  return (
    <div className="grid gap-4 lg:grid-cols-2">
      <HistoryChart
        title="CPU"
        info="Use of all the CPUs of the machine: 100% = every core is busy."
        rows={rows}
        lines={[{ key: 'cpu', label: 'CPU', color: color(0) }]}
        unit="percent"
        history={history}
        max={100}
      />
      <HistoryChart
        title="Memory"
        info="Memory in use, without the cache the kernel gives back when programs need it. The chart goes up to the machine's memory."
        rows={rows}
        lines={[{ key: 'memory', label: 'Used', color: color(0) }]}
        unit="bytes"
        history={history}
        max={totals.length ? Math.max(...totals) : undefined}
      />
      <HistoryChart
        title="Disk"
        info="How fast the disks of the live view read and write, all together."
        rows={rows}
        lines={[
          { key: 'diskRead', label: 'Read', color: color(0) },
          { key: 'diskWrite', label: 'Write', color: color(1) },
        ]}
        unit="bytes_per_sec"
        history={history}
        legend
      />
      <HistoryChart
        title="Network"
        info="Traffic of the machine's network interfaces, without the loopback and Docker's own interfaces."
        rows={rows}
        lines={[
          { key: 'networkRx', label: 'Received', color: color(0) },
          { key: 'networkTx', label: 'Sent', color: color(1) },
        ]}
        unit="bytes_per_sec"
        history={history}
        legend
      />
    </div>
  )
}

/** The busiest service instances and datastores of the range: CPU and memory, with one legend. */
export function ContainerCharts({ history }: { history: MetricsHistory }) {
  if (history.containers.length === 0) {
    return (
      <EmptyState
        icon={<LineChartIcon />}
        title="Nothing ran in this range"
        description="The instances of your services and your datastores show up here once they have run."
        size="sm"
      />
    )
  }
  const lines = history.containers.map((c, i) => ({
    key: containerKey(i),
    label: containerLabel(c),
    color: color(i),
  }))
  return (
    <div className="flex flex-col gap-4">
      <ul aria-label="Legend" className="flex flex-wrap gap-x-5 gap-y-2">
        {lines.map((l) => (
          <li key={l.key} className="min-w-0">
            <LegendItem color={l.color} label={l.label} />
          </li>
        ))}
      </ul>
      <div className="grid gap-4 lg:grid-cols-2">
        <HistoryChart
          title="CPU"
          info="CPU use of each container: 100% = one full core."
          rows={containerRows(history, 'cpu')}
          lines={lines}
          unit="percent"
          history={history}
        />
        <HistoryChart
          title="Memory"
          info="Memory use of each container, without the page cache."
          rows={containerRows(history, 'memory')}
          lines={lines}
          unit="bytes"
          history={history}
        />
      </div>
    </div>
  )
}

/** Every chart for one answer of the API: "collecting data" until the first reading. */
export function MetricsHistoryView({ history }: { history: MetricsHistory }) {
  if (history.host.length === 0 && history.containers.length === 0) {
    return <EmptyState icon={<LineChartIcon />} title={COLLECTING} description="Ferry reads this server every 10 seconds." />
  }
  return (
    <div className="flex flex-col gap-8">
      <HostCharts history={history} />
      <div className="flex flex-col gap-4">
        <div className="flex flex-col gap-1">
          <h3 className="text-base font-medium text-foreground">Busiest services and datastores</h3>
          <p className="text-sm text-foreground-light">The 5 whose CPU use peaked highest in this range.</p>
        </div>
        <ContainerCharts history={history} />
      </div>
    </div>
  )
}

function RangePicker({ value, onChange }: { value: MetricsRange; onChange: (range: MetricsRange) => void }) {
  return (
    <ToggleGroup
      type="single"
      variant="outline"
      value={value}
      onValueChange={(v) => {
        const range = METRICS_RANGES.find((r) => r.value === v)
        if (range) onChange(range.value)
      }}
      aria-label="Time range"
    >
      {METRICS_RANGES.map((r) => (
        <ToggleGroupItem key={r.value} value={r.value} className="px-3 tabular">
          {r.label}
        </ToggleGroupItem>
      ))}
    </ToggleGroup>
  )
}

function HistorySkeleton() {
  return (
    <div className="grid gap-4 lg:grid-cols-2">
      {['CPU', 'Memory', 'Disk', 'Network'].map((label) => (
        <MetricCard key={label} label={label} loading />
      ))}
    </div>
  )
}

/**
 * The last hour, 6 hours or 24 hours of the server (`GET /api/v1/server/metrics/history`, asked
 * every 10 s while the tab is visible).
 */
export function MetricsHistorySection() {
  const [range, setRange] = React.useState<MetricsRange>('1h')
  const { data, error, isLoading, refetch, isRefetching } = useMetricsHistory(range)
  return (
    <PageSection
      title="Over time"
      description="Ferry reads this server every 10 seconds and keeps the last 24 hours. A gap is a time Ferry was stopped."
      actions={<RangePicker value={range} onChange={setRange} />}
    >
      {isLoading ? (
        <HistorySkeleton />
      ) : data ? (
        <>
          {error && (
            <Callout tone="warning" icon={<AlertTriangle />} title="These charts are not current">
              The last refresh failed: {errorMessage(error)}
            </Callout>
          )}
          <MetricsHistoryView history={data} />
        </>
      ) : (
        <ErrorState
          error={error}
          title="Could not load the server's history"
          onRetry={() => void refetch()}
          retrying={isRefetching}
        />
      )}
    </PageSection>
  )
}
