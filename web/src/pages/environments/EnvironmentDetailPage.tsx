import * as React from 'react'
import { Link, useNavigate, useParams } from 'react-router'
import { ArrowLeft, Layers, Plus, Trash2 } from 'lucide-react'

import { CopyButton } from '@/components/patterns/Copy'
import { TableMessageRow, TableSkeletonRows } from '@/components/patterns/DataTable'
import { EmptyState, ErrorState } from '@/components/patterns/EmptyState'
import { DatastoreKindIcon, ServiceTypeIcon } from '@/components/patterns/icons'
import { PageContainer, PageHeader, PageSection } from '@/components/patterns/Page'
import { DatastoreStatusBadge, ServiceStatePill } from '@/components/patterns/StatusBadge'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { ApiError } from '@/lib/api/client'
import { useDatastores, useEnvironment, useServices } from '@/lib/api/queries'
import { qualifiedName } from '@/lib/api/refs'
import { PRODUCTION, type EnvironmentView } from '@/lib/api/types'
import { DATASTORE_KIND_LABELS, dateTime, relativeTime, SERVICE_TYPE_LABELS } from '@/lib/format'
import { datastorePath } from '@/pages/datastores/lib'
import { NewDatastoreDialog } from '@/pages/datastores/NewDatastoreDialog'
import { servicePath } from '@/pages/service/context'

import { DeleteEnvironmentDialog } from './DeleteEnvironmentDialog'
import { inEnvironment, membersSummary, newServicePath } from './lib'
import { EnvironmentDnsList, EnvironmentIconBox } from './parts'

const nameLink =
  'inline-flex items-center gap-2 rounded-sm font-medium outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring'

function BackLink() {
  return (
    <Link
      to="/environments"
      className="inline-flex items-center gap-1.5 rounded-sm text-foreground-lighter outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
    >
      <ArrowLeft className="size-3.5" aria-hidden="true" /> Environments
    </Link>
  )
}

function ServicesSection({ env }: { env: EnvironmentView }) {
  const { data: services, isLoading } = useServices()
  // The environment lists plain names: the same name may be another environment's too.
  const byName = new Map(inEnvironment(services ?? [], env.name).map((s) => [s.name, s]))
  return (
    <PageSection
      title="Services"
      description={`They reach the datastores and services of ${env.name} by name, and nothing of another environment.`}
      actions={
        <Button asChild icon={<Plus />}>
          <Link to={newServicePath(env.name)}>New service</Link>
        </Button>
      }
    >
      <Table>
        <TableHeader>
          <TableRow className="hover:bg-transparent">
            <TableHead>Service</TableHead>
            <TableHead className="hidden sm:table-cell">Type</TableHead>
            <TableHead>State</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {env.services.length === 0 ? (
            <TableMessageRow colSpan={3}>No services in {env.name} yet.</TableMessageRow>
          ) : isLoading ? (
            <TableSkeletonRows columns={3} rows={Math.min(env.services.length, 3)} />
          ) : (
            env.services.map((name) => {
              const s = byName.get(name)
              return (
                <TableRow key={name}>
                  <TableCell>
                    <Link to={servicePath(qualifiedName({ name, environment: env.name }))} className={nameLink}>
                      {s && <ServiceTypeIcon type={s.type} className="size-4 text-foreground-lighter" />}
                      {name}
                    </Link>
                  </TableCell>
                  <TableCell className="hidden text-[13px] text-foreground-light sm:table-cell">
                    {s ? SERVICE_TYPE_LABELS[s.type] : '—'}
                  </TableCell>
                  <TableCell>{s ? <ServiceStatePill state={s.state} /> : '—'}</TableCell>
                </TableRow>
              )
            })
          )}
        </TableBody>
      </Table>
    </PageSection>
  )
}

function DatastoresSection({ env }: { env: EnvironmentView }) {
  const { data: datastores, isLoading } = useDatastores()
  const [createOpen, setCreateOpen] = React.useState(false)
  const byName = new Map(inEnvironment(datastores ?? [], env.name).map((d) => [d.name, d]))
  return (
    <PageSection
      title="Datastores"
      description={`Only the services of ${env.name} reach them.`}
      actions={
        <Button icon={<Plus />} onClick={() => setCreateOpen(true)}>
          New datastore
        </Button>
      }
    >
      <Table>
        <TableHeader>
          <TableRow className="hover:bg-transparent">
            <TableHead>Datastore</TableHead>
            <TableHead className="hidden sm:table-cell">Type</TableHead>
            <TableHead>Status</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {env.datastores.length === 0 ? (
            <TableMessageRow colSpan={3}>No datastores in {env.name} yet.</TableMessageRow>
          ) : isLoading ? (
            <TableSkeletonRows columns={3} rows={Math.min(env.datastores.length, 3)} />
          ) : (
            env.datastores.map((name) => {
              const d = byName.get(name)
              return (
                <TableRow key={name}>
                  <TableCell>
                    <Link to={datastorePath(qualifiedName({ name, environment: env.name }))} className={nameLink}>
                      {d && <DatastoreKindIcon kind={d.kind} className="size-4 text-foreground-lighter" />}
                      {name}
                    </Link>
                  </TableCell>
                  <TableCell className="hidden text-[13px] text-foreground-light sm:table-cell">
                    {d ? `${DATASTORE_KIND_LABELS[d.kind]} ${d.version}` : '—'}
                  </TableCell>
                  <TableCell>{d ? <DatastoreStatusBadge status={d.status} /> : '—'}</TableCell>
                </TableRow>
              )
            })
          )}
        </TableBody>
      </Table>
      <NewDatastoreDialog key={env.name} open={createOpen} onOpenChange={setCreateOpen} environment={env.name} />
    </PageSection>
  )
}

function DetailSkeleton() {
  return (
    <div aria-busy="true" aria-label="Loading environment" className="flex flex-col gap-8">
      <div className="flex items-center gap-4">
        <Skeleton className="size-11 rounded-lg" />
        <div className="flex flex-col gap-2">
          <Skeleton className="h-7 w-48" />
          <Skeleton className="h-4 w-64" />
        </div>
      </div>
      <Skeleton className="h-6 w-40" />
      <Skeleton className="h-32 w-full" />
    </div>
  )
}

/** `/environments/:name` — what it holds (linked), where its services are found, its DNS records, delete. */
export function EnvironmentDetailPage() {
  const { name = '' } = useParams()
  const navigate = useNavigate()
  const { data: env, error, refetch, isRefetching } = useEnvironment(name)
  const [deleteOpen, setDeleteOpen] = React.useState(false)

  if (error instanceof ApiError && error.isNotFound && !env) {
    return (
      <PageContainer size="narrow">
        <EmptyState
          size="lg"
          icon={<Layers />}
          title={`Environment “${name}” not found`}
          description="It may have been deleted."
          actions={
            <Button asChild variant="primary">
              <Link to="/environments">All environments</Link>
            </Button>
          }
        />
      </PageContainer>
    )
  }

  const production = env?.name === PRODUCTION

  return (
    <PageContainer size="narrow">
      {error && !env ? (
        <>
          <div className="mb-6 text-[13px]">
            <BackLink />
          </div>
          <ErrorState
            error={error}
            title="Could not load the environment"
            onRetry={() => void refetch()}
            retrying={isRefetching}
          />
        </>
      ) : !env ? (
        <DetailSkeleton />
      ) : (
        <>
          <PageHeader
            eyebrow={<BackLink />}
            title={
              <span className="flex min-w-0 items-center gap-3">
                <EnvironmentIconBox />
                <span className="truncate">{env.name}</span>
              </span>
            }
            badges={
              <>
                {production && <Badge variant="outline">Default</Badge>}
                <Badge variant="outline" font="mono" case="normal">
                  {membersSummary(env)}
                </Badge>
              </>
            }
            description={
              <span title={dateTime(env.created_at)}>
                Created {relativeTime(env.created_at)} · <span className="font-mono text-[13px]">{env.id}</span>
              </span>
            }
          />

          <PageSection
            title="Web addresses"
            description={
              production
                ? 'Services created without picking an environment land here. Its web services answer under each domain of the server.'
                : env.dns.length > 0
                  ? 'Its web services answer under each domain of the server once the DNS record below points at this server.'
                  : 'Its web services answer under each domain of the server. A local domain needs no DNS record.'
            }
          >
            <Card className="flex flex-row items-center gap-2 px-5 py-4 md:px-6">
              <span className="min-w-0 flex-1 font-mono text-[13px] break-all text-foreground">{env.url_pattern}</span>
              <CopyButton value={env.url_pattern} what="URL pattern" />
            </Card>
          </PageSection>

          {env.dns.length > 0 && (
            <PageSection
              title="DNS"
              description={`Create the wildcard record${env.dns.length > 1 ? 's' : ''} where the DNS of ${env.dns.map((d) => d.domain).join(', ')} is managed: your registrar or your DNS provider.`}
            >
              <EnvironmentDnsList environment={env} />
            </PageSection>
          )}

          <ServicesSection env={env} />
          <DatastoresSection env={env} />

          {!production && (
            <PageSection title="Danger zone">
              <Card className="border-destructive-border">
                <div className="flex flex-col gap-4 px-5 py-5 sm:flex-row sm:items-center sm:justify-between md:px-6">
                  <div className="flex min-w-0 flex-col gap-1">
                    <p className="text-sm font-medium text-foreground">Delete this environment</p>
                    <p className="text-[13px] text-foreground-light">
                      Removes its private network. Delete its services and datastores first.
                    </p>
                  </div>
                  <Button variant="danger" icon={<Trash2 />} onClick={() => setDeleteOpen(true)} className="shrink-0">
                    Delete environment
                  </Button>
                </div>
              </Card>
            </PageSection>
          )}

          {!production && (
            <DeleteEnvironmentDialog
              environment={env}
              open={deleteOpen}
              onOpenChange={setDeleteOpen}
              onDeleted={() => void navigate('/environments', { replace: true })}
            />
          )}
        </>
      )}
    </PageContainer>
  )
}
