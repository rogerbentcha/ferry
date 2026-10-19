import * as React from 'react'
import { Link } from 'react-router'
import { Activity, AlertTriangle, Boxes, Globe, LayoutDashboard, LineChart as LineChartIcon } from 'lucide-react'
import { Area, AreaChart, CartesianGrid, Line, LineChart, XAxis, YAxis } from 'recharts'

import { Callout, EmptyState, ErrorState } from '@/components/patterns/EmptyState'
import { MetricCard } from '@/components/patterns/MetricCard'
import { PageSection } from '@/components/patterns/Page'
import { type ChartConfig, ChartContainer, ChartTooltip, ChartTooltipContent } from '@/components/ui/chart'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { errorMessage } from '@/lib/api/client'
import { useMetricsHistory, useServices } from '@/lib/api/queries'
import { qualifiedName } from '@/lib/api/refs'
import type { MetricsHistory, MetricsRange, RequestMetrics } from '@/lib/api/types'
import { percent } from '@/lib/format'
import {
  type ChartRow,
  METRICS_RANGES,
  type MetricUnit,
  containerKey,
  containerLabel,
  containerRows,
  errorRate,
  formatClock,
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
} from '@/lib/metrics-history'

import { servicePath } from '../service/context'

/** What a value ferryd can't read shows (the same words as the live cards). */
const NOT_AVAILABLE = 'Not available'

/** Shown before the first reading is stored. */
export const COLLECTING = 'Collecting data… first points in about 10 s'

/** The requests area before the proxy answered anything. */
export const NO_REQUESTS = 'No requests yet'

/** The requests and latency charts when only the dashboard or unrouted hosts got requests. */
export const NO_SERVICE_REQUESTS = 'No request to your services in this range'

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
  /** Areas stacked on each other (the requests by status class) instead of lines. */
  stacked?: boolean
  /** What the card says when no value is known (default: "Not available"). */
  empty?: string
}

/** One line chart of the history (or a chart of stacked areas), in a metric card. */
function HistoryChart({
  title,
  info,
  rows,
  lines,
  unit,
  history,
  max,
  legend = false,
  stacked = false,
  empty = NOT_AVAILABLE,
}: HistoryChartProps) {
  const config = Object.fromEntries(lines.map((l) => [l.key, { label: l.label, color: l.color }])) satisfies ChartConfig
  const keys = lines.map((l) => l.key)
  const known = hasValues(rows, keys)
  const [from, to] = [Date.parse(history.from), Date.parse(history.to)]
  const valueTicks =
    max === undefined ? niceTicks((stacked ? maxStacked : maxValue)(rows, keys), unit) : quarterTicks(max)
  const top = valueTicks?.[valueTicks.length - 1]
  const Chart = stacked ? AreaChart : LineChart
  return (
    <MetricCard
      label={title}
      info={info}
      aside={legend && known ? lines.map((l) => <LegendItem key={l.key} color={l.color} label={l.label} />) : undefined}
    >
      {known ? (
        <ChartContainer config={config} className="aspect-auto h-44 w-full" aria-label={`${title} over time`}>
          <Chart data={rows} margin={{ top: 4, right: 4, bottom: 0, left: 0 }}>
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
            {lines.map((l) =>
              stacked ? (
                <Area
                  key={l.key}
                  dataKey={l.key}
                  name={l.key}
                  type="linear"
                  stackId="stack"
                  stroke={`var(--color-${l.key})`}
                  fill={`var(--color-${l.key})`}
                  fillOpacity={0.35}
                  strokeWidth={1.5}
                  dot={false}
                  activeDot={{ r: 3 }}
                  connectNulls={false}
                  isAnimationActive={false}
                />
              ) : (
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
              ),
            )}
          </Chart>
        </ChartContainer>
      ) : (
        <p className="flex h-44 items-center justify-center px-4 text-center text-[13px] text-foreground-lighter">
          {empty}
        </p>
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

/** The status classes of the requests chart: green for success, blue for redirects, then orange and red. */
const STATUS_LINES: ChartLine[] = [
  { key: 's2xx', label: '2xx', color: 'var(--chart-1)' },
  { key: 's3xx', label: '3xx', color: 'var(--chart-4)' },
  { key: 's4xx', label: '4xx', color: 'var(--chart-2)' },
  { key: 's5xx', label: '5xx', color: 'var(--chart-3)' },
]

/** The small grey line under a value of the table. */
function SubLine({ children }: { children: React.ReactNode }) {
  return <span className="mb-1 text-[12px] text-foreground-lighter">{children}</span>
}

/**
 * The name of a target: a link to its service's page under its current qualified name (`current`,
 * `api.staging` outside production; a deleted service has none), or what the dashboard and
 * unrouted requests are.
 */
function TargetName({ target, current }: { target: RequestMetrics; current: string | undefined }) {
  const [Icon, note] =
    target.target === 'dashboard'
      ? [LayoutDashboard, "Ferry's own host"]
      : target.target === 'unrouted'
        ? [Globe, 'Hosts no service has']
        : [Boxes, null]
  return (
    <span className="flex min-w-0 items-start gap-2">
      <Icon className="mt-0.5 size-4 shrink-0 text-foreground-lighter" aria-hidden="true" />
      <span className="flex min-w-0 flex-col">
        {note === null && current !== undefined ? (
          <Link
            to={servicePath(current)}
            className="truncate rounded-sm font-medium text-foreground underline-offset-4 outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring"
          >
            {current}
          </Link>
        ) : (
          <>
            <span className="truncate font-medium text-foreground">{targetLabel(target)}</span>
            {note !== null && <SubLine>{note}</SubLine>}
          </>
        )}
      </span>
    </span>
  )
}

/**
 * One row per target, in the order of the API: the services, the busiest first, then the dashboard
 * and unrouted. A service links to its page while it exists (the service list the shell keeps).
 */
export function RequestsTable({ targets }: { targets: RequestMetrics[] }) {
  const { data: services } = useServices()
  const names = new Map(services?.map((s) => [s.id, qualifiedName(s)]))
  return (
    <Table aria-label="Requests by service">
      <TableHeader>
        <TableRow className="hover:bg-transparent">
          <TableHead className="w-[40%]">Name</TableHead>
          <TableHead>Requests</TableHead>
          <TableHead>Error rate</TableHead>
          <TableHead>p95</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {targets.map((t) => {
          const total = totalRequests(t)
          return (
            <TableRow key={t.target} className="hover:bg-transparent">
              <TableCell>
                <TargetName target={t} current={names.get(t.target)} />
              </TableCell>
              <TableCell className="tabular">
                <span className="flex flex-col">
                  {formatRate(t.requests_per_sec)}
                  <SubLine>{`${total.toLocaleString('en-US')} ${total === 1 ? 'request' : 'requests'}`}</SubLine>
                </span>
              </TableCell>
              <TableCell className="tabular">
                <span className="flex flex-col">
                  {percent(errorRate(t))}
                  <SubLine>{`${t.status_5xx.toLocaleString('en-US')} 5xx`}</SubLine>
                </span>
              </TableCell>
              <TableCell className="tabular">{formatLatency(t.p95_ms)}</TableCell>
            </TableRow>
          )
        })}
      </TableBody>
    </Table>
  )
}

/** The requests the proxy answered in the range: by status class, their latency, and per service. */
export function RequestsView({ history }: { history: MetricsHistory }) {
  if (history.request_targets.length === 0) {
    return (
      <EmptyState
        icon={<Activity />}
        title={NO_REQUESTS}
        description="Ferry counts the requests its proxy answers for your services and stores them every 10 seconds."
        size="sm"
      />
    )
  }
  const rows = requestRows(history)
  const anyRequest = history.requests.some((p) => totalRequests(p) > 0)
  return (
    <div className="flex flex-col gap-4">
      <div className="grid gap-4 lg:grid-cols-2">
        <HistoryChart
          title="Requests"
          info="Requests to your services per second, by status: 2xx (with the WebSockets that opened), 3xx, 4xx, and 5xx (with the proxy's own 502 and 503 pages). Neither the dashboard's requests nor the unrouted ones."
          rows={anyRequest ? rows : []}
          lines={STATUS_LINES}
          unit="requests_per_sec"
          history={history}
          stacked
          legend
          empty={NO_SERVICE_REQUESTS}
        />
        <HistoryChart
          title="Latency"
          info="How long your services took to send their response headers. p50: half the requests were faster. p95: 95% of them were faster. WebSockets are left out."
          rows={rows}
          lines={[
            { key: 'p50', label: 'p50', color: color(0) },
            { key: 'p95', label: 'p95', color: color(1) },
          ]}
          unit="milliseconds"
          history={history}
          legend
          empty={NO_SERVICE_REQUESTS}
        />
      </div>
      <RequestsTable targets={history.request_targets} />
    </div>
  )
}

/** A titled part of **Over time**. */
function Subsection({ title, description, children }: { title: string; description: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-col gap-1">
        <h3 className="text-base font-medium text-foreground">{title}</h3>
        <p className="text-sm text-foreground-light">{description}</p>
      </div>
      {children}
    </div>
  )
}

/** Every chart for one answer of the API: "collecting data" until the first reading. */
export function MetricsHistoryView({ history }: { history: MetricsHistory }) {
  if (history.host.length === 0 && history.containers.length === 0 && history.request_targets.length === 0) {
    return <EmptyState icon={<LineChartIcon />} title={COLLECTING} description="Ferry reads this server every 10 seconds." />
  }
  return (
    <div className="flex flex-col gap-8">
      <HostCharts history={history} />
      <Subsection
        title="Requests"
        description="What the proxy answered, until the response headers went out. Error rate: the share of 5xx."
      >
        <RequestsView history={history} />
      </Subsection>
      <Subsection title="Busiest services and datastores" description="The 5 whose CPU use peaked highest in this range.">
        <ContainerCharts history={history} />
      </Subsection>
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
