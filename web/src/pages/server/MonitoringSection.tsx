import * as React from 'react'
import { Link } from 'react-router'
import { AlertTriangle, Boxes, Database } from 'lucide-react'

import { Callout, EmptyState, ErrorState } from '@/components/patterns/EmptyState'
import { MetricCard, UsageBar } from '@/components/patterns/MetricCard'
import { MonoLabel } from '@/components/patterns/MonoLabel'
import { PageHeader, PageSection } from '@/components/patterns/Page'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { errorMessage } from '@/lib/api/client'
import { useServerMetrics } from '@/lib/api/queries'
import type { ContainerMetrics, DiskMetrics, HostMetrics, ServerMetrics } from '@/lib/api/types'
import { bytes, percent } from '@/lib/format'
import { formatCpus } from '@/lib/resources'
import { cn } from '@/lib/utils'

import { servicePath } from '../service/context'

// The charts and their library load when the section is opened.
const MetricsHistorySection = React.lazy(() =>
  import('./MetricsHistory').then((m) => ({ default: m.MetricsHistorySection })),
)

/** What a value ferryd can't read shows (for example on macOS, which has no `/proc`). */
export const NOT_AVAILABLE = 'Not available'

function NotAvailable() {
  return <span className="text-[15px] text-foreground-lighter">{NOT_AVAILABLE}</span>
}

/** `1.5 MiB/s`; `—` when unknown. */
function speed(n: number | null): string {
  return n === null ? '—' : `${bytes(n)}/s`
}

/** `used` in percent of `total`, when both are known. */
function share(used: number | null, total: number | null): number | null {
  return used !== null && total ? (used / total) * 100 : null
}

/** A small label + value pair under a card's main number. */
function Pair({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="flex min-w-0 flex-col gap-0.5">
      <MonoLabel as="dt">{label}</MonoLabel>
      <dd className="truncate text-[13px] text-foreground tabular">{value}</dd>
    </div>
  )
}

function DiskCard({ disk }: { disk: DiskMetrics }) {
  const io = disk.read_bytes_per_sec !== null || disk.write_bytes_per_sec !== null
  return (
    <MetricCard
      label={
        <>
          Disk <span className="normal-case">{disk.mount_point}</span>
        </>
      }
      info="Space used on the filesystem mounted there, and how fast its device reads and writes."
      value={bytes(disk.used_bytes)}
      unit={`/ ${bytes(disk.total_bytes)}`}
    >
      <UsageBar value={share(disk.used_bytes, disk.total_bytes)} label={`Disk space used on ${disk.mount_point}`} />
      {io ? (
        <dl className="grid grid-cols-2 gap-3">
          <Pair label="Read" value={speed(disk.read_bytes_per_sec)} />
          <Pair label="Write" value={speed(disk.write_bytes_per_sec)} />
        </dl>
      ) : (
        <p className="text-[12.5px] text-foreground-lighter">Read and write speed: {NOT_AVAILABLE}</p>
      )}
    </MetricCard>
  )
}

/** The machine: CPU, memory, each disk, network. */
export function HostCards({ host }: { host: HostMetrics }) {
  const memory = share(host.memory_used_bytes, host.memory_total_bytes)
  const network = host.network_rx_bytes_per_sec !== null || host.network_tx_bytes_per_sec !== null
  return (
    <div className="grid gap-4 sm:grid-cols-2">
      <MetricCard
        label="CPU"
        info="Use of all the CPUs of the machine: 100% = every core is busy."
        value={host.cpu_percent === null ? <NotAvailable /> : percent(host.cpu_percent)}
        unit={host.cpu_percent === null ? formatCpus(host.cpu_count) : `of ${formatCpus(host.cpu_count)}`}
      >
        {host.cpu_percent !== null && <UsageBar value={host.cpu_percent} label="CPU of the server" />}
      </MetricCard>
      <MetricCard
        label="Memory"
        info="Memory in use, without the cache the kernel gives back when programs need it (like the free command)."
        value={host.memory_used_bytes === null ? <NotAvailable /> : bytes(host.memory_used_bytes)}
        unit={host.memory_used_bytes !== null && host.memory_total_bytes ? `/ ${bytes(host.memory_total_bytes)}` : undefined}
      >
        {memory !== null && <UsageBar value={memory} label="Memory of the server" />}
      </MetricCard>
      {host.disks.length > 0 ? (
        host.disks.map((d) => <DiskCard key={d.mount_point} disk={d} />)
      ) : (
        <MetricCard label="Disk" value={<NotAvailable />} />
      )}
      <MetricCard
        label="Network"
        info="Traffic of the machine's network interfaces, without the loopback and Docker's own interfaces."
        value={network ? undefined : <NotAvailable />}
      >
        {network && (
          <dl className="grid grid-cols-2 gap-3">
            <div className="flex min-w-0 flex-col gap-0.5">
              <MonoLabel as="dt">Received</MonoLabel>
              <dd className="truncate text-xl text-foreground tabular md:text-[22px]">
                {speed(host.network_rx_bytes_per_sec)}
              </dd>
            </div>
            <div className="flex min-w-0 flex-col gap-0.5">
              <MonoLabel as="dt">Sent</MonoLabel>
              <dd className="truncate text-xl text-foreground tabular md:text-[22px]">
                {speed(host.network_tx_bytes_per_sec)}
              </dd>
            </div>
          </dl>
        )}
      </MetricCard>
    </div>
  )
}

/** The small grey line under a value of the table (instance, limit). */
function SubLine({ children, className }: { children: React.ReactNode; className?: string }) {
  return <span className={cn('mb-1 text-[12px] text-foreground-lighter', className)}>{children}</span>
}

function containerPath(c: ContainerMetrics): string {
  return c.kind === 'service' ? servicePath(c.name) : `/datastores/${encodeURIComponent(c.name)}`
}

/** Every running service instance and datastore, in the order of the API (the busiest first). */
export function ContainersTable({ containers }: { containers: ContainerMetrics[] | null }) {
  if (containers === null) {
    return (
      <EmptyState
        icon={<AlertTriangle />}
        title={NOT_AVAILABLE}
        description="Docker did not answer, so the CPU and memory of the containers can't be read right now."
        size="sm"
      />
    )
  }
  if (containers.length === 0) {
    return (
      <EmptyState
        icon={<Boxes />}
        title="Nothing is running"
        description="The instances of your services and your datastores show up here while they run."
        size="sm"
      />
    )
  }
  return (
    <Table aria-label="Containers by CPU use">
      <TableHeader>
        <TableRow className="hover:bg-transparent">
          <TableHead className="w-[44%]">Name</TableHead>
          <TableHead>CPU</TableHead>
          <TableHead className="w-[32%]">Memory</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {containers.map((c) => {
          const Icon = c.kind === 'service' ? Boxes : Database
          const memory = share(c.memory_bytes, c.memory_limit_bytes)
          return (
            <TableRow key={`${c.kind}/${c.id}/${c.instance ?? ''}`} className="hover:bg-transparent">
              <TableCell>
                <span className="flex min-w-0 items-start gap-2">
                  <Icon className="mt-0.5 size-4 shrink-0 text-foreground-lighter" aria-hidden="true" />
                  <span className="flex min-w-0 flex-col">
                    <Link
                      to={containerPath(c)}
                      className="truncate rounded-sm font-medium text-foreground underline-offset-4 outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring"
                    >
                      {c.name}
                    </Link>
                    <SubLine className="font-mono">{c.instance ?? 'datastore'}</SubLine>
                  </span>
                </span>
              </TableCell>
              <TableCell className="tabular">
                <span className="flex flex-col">
                  {percent(c.cpu_percent)}
                  <SubLine>{c.cpu_limit ? `limit ${formatCpus(c.cpu_limit)}` : 'no limit'}</SubLine>
                </span>
              </TableCell>
              <TableCell className="tabular">
                <span className="flex flex-col">
                  {bytes(c.memory_bytes)}
                  <SubLine>{c.memory_limit_bytes ? `limit ${bytes(c.memory_limit_bytes)}` : 'no limit'}</SubLine>
                  {memory !== null && (
                    <UsageBar value={memory} label={`Memory of ${c.name} ${c.instance ?? ''}`.trim()} />
                  )}
                </span>
              </TableCell>
            </TableRow>
          )
        })}
      </TableBody>
    </Table>
  )
}

/** The whole section for one answer of the API. */
export function MonitoringView({ metrics }: { metrics: ServerMetrics }) {
  return (
    <>
      <PageSection title="This server" description="The machine ferryd runs on, right now. Refreshed every 5 seconds.">
        <HostCards host={metrics.host} />
      </PageSection>
      <PageSection
        title="Services and datastores"
        description="Every running instance and datastore, the busiest first. CPU: 100% = one full core."
      >
        <ContainersTable containers={metrics.containers} />
      </PageSection>
    </>
  )
}

interface ChartsBoundaryState {
  failed: boolean
  error?: unknown
}

/**
 * An error in place of the charts when their code fails to load (a tab left
 * open while ferryd was upgraded asks for a file that is gone) or to render,
 * so the live cards stay. Retry reloads the page, which gets the current files.
 */
export class ChartsBoundary extends React.Component<{ children: React.ReactNode }, ChartsBoundaryState> {
  override state: ChartsBoundaryState = { failed: false }

  static getDerivedStateFromError(error: unknown): ChartsBoundaryState {
    return { failed: true, error }
  }

  override render() {
    if (!this.state.failed) return this.props.children
    return (
      <PageSection title="Over time">
        <ErrorState error={this.state.error} title="Could not load the charts" onRetry={() => window.location.reload()} />
      </PageSection>
    )
  }
}

function MonitoringSkeleton() {
  return (
    <PageSection title="This server" description="The machine ferryd runs on.">
      <div className="grid gap-4 sm:grid-cols-2">
        {['CPU', 'Memory', 'Disk', 'Network'].map((label) => (
          <MetricCard key={label} label={label} loading />
        ))}
      </div>
    </PageSection>
  )
}

/**
 * Server → Monitoring: how busy the machine is right now and which service
 * or datastore uses it (`GET /api/v1/server/metrics`, asked every 5 s while
 * the tab is visible), then the charts of the last hours.
 */
export function MonitoringSection() {
  const { data, error, isLoading, refetch, isRefetching } = useServerMetrics()
  return (
    <>
      <PageHeader
        title="Monitoring"
        description="How busy this server is right now and over the last 24 hours, which service or datastore uses it, and how your services answer their requests."
      />
      {isLoading ? (
        <MonitoringSkeleton />
      ) : data ? (
        <>
          {error && (
            <Callout tone="warning" icon={<AlertTriangle />} title="These numbers are not current" className="mb-6">
              The last refresh failed: {errorMessage(error)}
            </Callout>
          )}
          <MonitoringView metrics={data} />
        </>
      ) : (
        <ErrorState
          error={error}
          title="Could not load the server's metrics"
          onRetry={() => void refetch()}
          retrying={isRefetching}
        />
      )}
      <ChartsBoundary>
        <React.Suspense fallback={null}>
          <MetricsHistorySection />
        </React.Suspense>
      </ChartsBoundary>
    </>
  )
}
