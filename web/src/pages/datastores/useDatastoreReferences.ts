import { useQueries } from '@tanstack/react-query'

import { endpoints } from '@/lib/api/endpoints'
import { keys } from '@/lib/api/keys'
import { useEnvGroups, useServices } from '@/lib/api/queries'
import { qualifiedName } from '@/lib/api/refs'
import type { DatastoreView, ServiceView } from '@/lib/api/types'

import { referencedDatastores } from './lib'

export interface DatastoreReference {
  service: ServiceView
  /** Env var keys whose value references the datastore. */
  keys: string[]
  /** Set when the reference comes from a linked env group. */
  group?: string
}

/**
 * Services whose variables reference the datastore through
 * `${{datastore.NAME.…}}` — their own variables or a linked env group's.
 * A reference names a datastore of the service's own environment, so only
 * the services of the datastore's environment count (`db` of staging is not
 * production's `db`). (There is no server endpoint for this; it scans the
 * envs client-side, the same way the server decides a delete is referenced.)
 */
export function useDatastoreReferences(ds: Pick<DatastoreView, 'name' | 'environment'> | undefined) {
  const services = useServices()
  const groups = useEnvGroups()
  const name = ds?.name
  const list = ds ? (services.data ?? []).filter((s) => s.environment === ds.environment) : []
  const envs = useQueries({
    queries: list.map((s) => ({
      queryKey: keys.serviceEnv(qualifiedName(s)),
      queryFn: ({ signal }: { signal: AbortSignal }) => endpoints.getServiceEnv(qualifiedName(s), signal),
      staleTime: 15_000,
    })),
  })

  // Cheap (a few services × a few vars): recomputed on every render.
  const refs: DatastoreReference[] = []
  if (name) {
    list.forEach((service, i) => {
      const vars = envs[i]?.data ?? []
      const own = vars.filter((v) => referencedDatastores(v.value).includes(name)).map((v) => v.key)
      if (own.length > 0) refs.push({ service, keys: own })
    })
    for (const g of groups.data ?? []) {
      const groupKeys = g.vars.filter((v) => referencedDatastores(v.value).includes(name)).map((v) => v.key)
      if (groupKeys.length === 0) continue
      // Qualified names (an env group is linked to services of any environment).
      for (const linked of g.services) {
        const service = list.find((s) => qualifiedName(s) === linked)
        if (service) refs.push({ service, keys: groupKeys, group: g.name })
      }
    }
    refs.sort((a, b) => a.service.name.localeCompare(b.service.name))
  }

  const loading = services.isLoading || groups.isLoading || envs.some((q) => q.isLoading)
  const error = services.error ?? groups.error ?? envs.find((q) => q.error)?.error ?? null
  return { refs, loading, error }
}
