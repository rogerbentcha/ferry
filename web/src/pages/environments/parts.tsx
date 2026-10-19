import { Link } from 'react-router'
import { AlertTriangle, Layers } from 'lucide-react'

import { CopyButton } from '@/components/patterns/Copy'
import { Callout } from '@/components/patterns/EmptyState'
import { Badge } from '@/components/ui/badge'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { PRODUCTION, type EnvironmentDns, type EnvironmentView } from '@/lib/api/types'
import { namesInParentZone, recordFullName } from '@/lib/domains'
import { cn } from '@/lib/utils'

import { environmentPath, sortEnvironments } from './lib'

const code = 'font-mono text-[12.5px]'

/**
 * The environment of a service or datastore as a small chip (cards, rows,
 * headers). `link` makes it lead to the environment's page.
 */
export function EnvironmentBadge({
  name,
  link = false,
  className,
}: {
  name: string
  link?: boolean
  className?: string
}) {
  const content = (
    <>
      <Layers aria-hidden="true" />
      <span className="sr-only">Environment </span>
      {name}
    </>
  )
  const classes = cn(link && 'relative z-10 hover:border-border-stronger hover:text-foreground', className)
  if (link) {
    return (
      <Badge asChild font="mono" shape="square" variant="outline" case="normal" className={classes} title={`Environment ${name}`}>
        <Link to={environmentPath(name)}>{content}</Link>
      </Badge>
    )
  }
  return (
    <Badge font="mono" shape="square" variant="outline" case="normal" className={classes} title={`Environment ${name}`}>
      {content}
    </Badge>
  )
}

/** Outlined square with the environment icon (card header / page title). */
export function EnvironmentIconBox({ className }: { className?: string }) {
  return (
    <span
      aria-hidden="true"
      className={cn(
        'flex size-11 shrink-0 items-center justify-center rounded-lg border bg-surface-100 text-foreground-light shadow-card',
        className,
      )}
    >
      <Layers className="size-5" strokeWidth={1.6} />
    </span>
  )
}

/**
 * Environment picker of the forms that create a service or a datastore.
 * Production first; the environments are those of the server, or production
 * alone while they load.
 */
export function EnvironmentSelect({
  id,
  value,
  onChange,
  environments,
  invalid,
  describedBy,
  className,
}: {
  id: string
  value: string
  onChange: (name: string) => void
  environments: EnvironmentView[] | undefined
  invalid?: boolean
  describedBy?: string
  className?: string
}) {
  const listed: readonly { name: string }[] = environments ?? [{ name: PRODUCTION }]
  const names = sortEnvironments(listed).map((e) => e.name)
  // A name that is not (or no longer) listed stays selectable, so the select never shows blank.
  if (!names.includes(value)) names.push(value)
  return (
    <Select value={value} onValueChange={onChange}>
      <SelectTrigger
        id={id}
        className={cn('w-full', className)}
        aria-invalid={invalid ? true : undefined}
        aria-describedby={describedBy}
      >
        <SelectValue />
      </SelectTrigger>
      <SelectContent position="popper">
        {names.map((name) => (
          <SelectItem key={name} value={name}>
            <Layers className="size-4 text-foreground-lighter" aria-hidden="true" />
            <span className="font-mono text-[12.5px] text-foreground">{name}</span>
            {name === PRODUCTION && <span className="text-foreground-lighter">(default)</span>}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}

/**
 * The records the services of an environment need under one domain of the
 * server (`*.staging` in the zone of `example.com`), each with a copy
 * button — the look of a connected domain's records (Server → Domains).
 */
export function EnvironmentDnsRecords({ environment, dns }: { environment: string; dns: EnvironmentDns }) {
  const parent = namesInParentZone(dns.domain)
  const unknown = dns.records.some((r) => r.value === null)
  return (
    <div className="flex flex-col gap-2.5">
      <Table aria-label={`DNS records of ${environment} under ${dns.domain}`} containerClassName="shadow-none">
        <TableHeader>
          <TableRow className="hover:bg-transparent">
            <TableHead className="w-16">Type</TableHead>
            <TableHead>Name</TableHead>
            <TableHead>Value</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {dns.records.map((r) => (
            <TableRow key={`${r.type} ${r.name} ${r.value ?? ''}`} className="hover:bg-transparent">
              <TableCell>
                <Badge font="mono" shape="square" case="normal">
                  {r.type}
                </Badge>
              </TableCell>
              <TableCell>
                <span className="flex items-center gap-1">
                  <span className="font-mono text-[13px]">{r.name}</span>
                  <CopyButton value={r.name} what={`the name ${r.name}`} variant="ghost" />
                </span>
                <span className="block font-mono text-[11.5px] text-foreground-lighter">
                  {recordFullName(r, dns.domain)}
                </span>
                {!r.required && <span className="block text-[11.5px] text-foreground-lighter">Optional (IPv6)</span>}
              </TableCell>
              <TableCell>
                {r.value ? (
                  <span className="flex items-center gap-1">
                    <span className="font-mono text-[13px]">{r.value}</span>
                    <CopyButton value={r.value} what={`the address ${r.value}`} variant="ghost" />
                  </span>
                ) : (
                  <span className="text-[13px] text-foreground-light">The public IP address of this server</span>
                )}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
      <p className="text-[12.5px] leading-relaxed text-foreground-lighter">
        One wildcard record answers for <span className={code}>{dns.wildcard}</span>: every service of {environment}{' '}
        under {dns.domain}, so a new one needs no new record.
        {parent && (
          <>
            {' '}
            If {dns.domain} lives in the DNS zone of {parent.zone}, name it{' '}
            <span className={code}>
              {dns.records[0]?.name ?? `*.${environment}`}.{parent.apex}
            </span>{' '}
            there.
          </>
        )}
      </p>
      {unknown && (
        <Callout tone="warning" icon={<AlertTriangle />}>
          Ferry could not find the public address of this server. Point the record at the address you reach the server
          at, and start ferryd with <code className={cn(code, 'whitespace-nowrap')}>--public-ip &lt;address&gt;</code>.
        </Callout>
      )}
    </div>
  )
}

/** Every domain's records, one block per domain (empty for production and local domains). */
export function EnvironmentDnsList({ environment }: { environment: Pick<EnvironmentView, 'name' | 'dns'> }) {
  return (
    <div className="flex flex-col gap-5">
      {environment.dns.map((d) => (
        <div key={d.domain} className="flex flex-col gap-2">
          {environment.dns.length > 1 && (
            <p className="text-[13px] font-medium text-foreground">
              Under <span className="font-mono">{d.domain}</span>
            </p>
          )}
          <EnvironmentDnsRecords environment={environment.name} dns={d} />
        </div>
      ))}
    </div>
  )
}
