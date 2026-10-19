import * as React from 'react'
import { useNavigate } from 'react-router'
import { Check, ExternalLink, Globe, Layers, MoreVertical, Network, Plus, Search, Trash2, X } from 'lucide-react'

import { CodeBlock } from '@/components/patterns/Copy'
import { EmptyState, ErrorState, StaleDataCallout } from '@/components/patterns/EmptyState'
import { SearchInput } from '@/components/patterns/ListToolbar'
import { PageContainer, PageHeader } from '@/components/patterns/Page'
import { CardStatusLine, RESOURCE_GRID, ResourceCard, ResourceCardSkeleton } from '@/components/patterns/ResourceCard'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { useEnvironments } from '@/lib/api/queries'
import { PRODUCTION, type EnvironmentView } from '@/lib/api/types'
import { dateTime, displayUrl, plural, relativeTime } from '@/lib/format'

import { DeleteEnvironmentDialog } from './DeleteEnvironmentDialog'
import { environmentPath, newServicePath, sortEnvironments } from './lib'
import { NewEnvironmentDialog } from './NewEnvironmentDialog'

/** What the card says about the network and the names of an environment. */
function NetworkLine({ env }: { env: EnvironmentView }) {
  if (env.name === PRODUCTION) {
    return (
      <CardStatusLine icon={<Check />} ring="border-primary/40 text-primary">
        Default environment
      </CardStatusLine>
    )
  }
  const first = env.dns[0]
  if (first) {
    const more = env.dns.length - 1
    return (
      <CardStatusLine icon={<Globe />} ring="border-border-strong text-foreground-lighter">
        <span title={env.dns.map((d) => d.wildcard).join('\n')}>
          DNS <span className="font-mono text-[12.5px]">{first.wildcard}</span>
          {more > 0 && ` +${more}`}
        </span>
      </CardStatusLine>
    )
  }
  return (
    <CardStatusLine icon={<Network />} ring="border-border-strong text-foreground-lighter">
      Private network
    </CardStatusLine>
  )
}

function EnvironmentCard({ env, onDelete }: { env: EnvironmentView; onDelete: () => void }) {
  const navigate = useNavigate()
  const to = environmentPath(env.name)
  const members = [...env.services, ...env.datastores]
  return (
    <ResourceCard
      href={to}
      name={env.name}
      icon={<Layers />}
      menu={
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              variant="ghost"
              size="icon-tiny"
              icon={<MoreVertical />}
              aria-label={`Actions for ${env.name}`}
              className="text-foreground-lighter"
            />
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-52">
            <DropdownMenuItem onSelect={() => void navigate(to)}>
              <ExternalLink /> Open environment
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={() => void navigate(newServicePath(env.name))}>
              <Plus /> New service in {env.name}
            </DropdownMenuItem>
            {env.name !== PRODUCTION && (
              <>
                <DropdownMenuSeparator />
                <DropdownMenuItem variant="destructive" onSelect={onDelete}>
                  <Trash2 /> Delete environment…
                </DropdownMenuItem>
              </>
            )}
          </DropdownMenuContent>
        </DropdownMenu>
      }
      subtitle={
        <span className="truncate font-mono text-[12.5px]" title={env.url_pattern}>
          {displayUrl(env.url_pattern)}
        </span>
      }
      badges={
        <>
          <Badge font="mono" shape="square" case="normal">
            {plural(env.services.length, 'service')}
          </Badge>
          <Badge font="mono" shape="square" variant="outline" case="normal">
            {plural(env.datastores.length, 'datastore')}
          </Badge>
        </>
      }
      footer={
        <>
          <span className="min-w-0 truncate text-[13px] text-foreground-lighter" title={members.join(', ') || undefined}>
            {members.length === 0 ? (
              'Nothing in it yet'
            ) : (
              <>
                Holds <span className="font-mono text-foreground-light">{members.join(', ')}</span>
              </>
            )}
          </span>
          <div className="flex min-w-0 flex-wrap items-center justify-between gap-x-3 gap-y-1">
            <NetworkLine env={env} />
            <time
              dateTime={env.created_at}
              title={dateTime(env.created_at)}
              className="text-[12.5px] text-foreground-lighter"
            >
              Created {relativeTime(env.created_at)}
            </time>
          </div>
        </>
      }
    />
  )
}

/** `/environments` — production and the other environments as Studio project cards. */
export function EnvironmentsPage() {
  const { data, error, isLoading, refetch, isRefetching } = useEnvironments()
  const [query, setQuery] = React.useState('')
  const [createOpen, setCreateOpen] = React.useState(false)
  const [deleting, setDeleting] = React.useState<EnvironmentView | null>(null)

  const all = React.useMemo(() => sortEnvironments(data ?? []), [data])
  const q = query.trim().toLowerCase()
  const shown = all.filter(
    (e) =>
      q === '' ||
      e.name.includes(q) ||
      e.services.some((s) => s.includes(q)) ||
      e.datastores.some((d) => d.includes(q)),
  )
  const onlyProduction = !isLoading && all.length === 1 && all[0]?.name === PRODUCTION

  const newButton = (
    <Button variant="primary" icon={<Plus />} onClick={() => setCreateOpen(true)}>
      New environment
    </Button>
  )

  return (
    <PageContainer>
      <PageHeader
        title="Environments"
        description="Groups of services and datastores, each on a private network of its own. Services reach only the datastores and services of their environment by name."
      />

      {(isLoading || all.length > 0) && (
        <div className="mb-4 flex flex-wrap items-center gap-2">
          <SearchInput
            value={query}
            onChange={setQuery}
            placeholder="Search by name, service or datastore"
            label="Search environments"
          />
          <div className="ml-auto shrink-0">{newButton}</div>
        </div>
      )}

      {error && data && (
        <StaleDataCallout error={error} onRetry={() => void refetch()} retrying={isRefetching} className="mb-4" />
      )}

      {error && !data ? (
        <ErrorState
          error={error}
          title="Could not load environments"
          onRetry={() => void refetch()}
          retrying={isRefetching}
        />
      ) : isLoading ? (
        <ul className={RESOURCE_GRID} aria-busy="true" aria-label="Loading environments">
          <ResourceCardSkeleton />
          <ResourceCardSkeleton />
        </ul>
      ) : shown.length === 0 ? (
        <EmptyState
          icon={<Search />}
          title="No environments match your search"
          description={`Nothing matches “${query.trim()}”.`}
          actions={
            <Button icon={<X />} onClick={() => setQuery('')}>
              Clear search
            </Button>
          }
        />
      ) : (
        <ul className={RESOURCE_GRID} aria-label="Environments">
          {shown.map((e) => (
            <EnvironmentCard key={e.id} env={e} onDelete={() => setDeleting(e)} />
          ))}
        </ul>
      )}

      {onlyProduction && q === '' && (
        <EmptyState
          className="mt-4"
          icon={<Layers />}
          title="Only production so far"
          description="Create staging to try changes away from production: its services and datastores get a private network of their own, and its web services answer at <service>.staging.<domain>."
          actions={newButton}
        >
          <div className="mt-3 w-full max-w-md">
            <CodeBlock code="ferry environments create staging" prompt />
          </div>
        </EmptyState>
      )}

      <p className="sr-only" role="status">
        {isLoading || q === '' ? '' : `Showing ${shown.length} of ${all.length} environments`}
      </p>

      <NewEnvironmentDialog open={createOpen} onOpenChange={setCreateOpen} />
      {deleting && (
        <DeleteEnvironmentDialog
          environment={deleting}
          open
          onOpenChange={(o) => !o && setDeleting(null)}
          onDeleted={() => setDeleting(null)}
        />
      )}
    </PageContainer>
  )
}
