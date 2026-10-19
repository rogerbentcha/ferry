import { useMutation, useQuery, useQueryClient, type QueryClient } from '@tanstack/react-query'

import { endpoints } from '../endpoints'
import { usePollInterval } from '../events'
import { keys } from '../keys'
import { isRef, qualifiedName } from '../refs'
import type { CreateDatastore, DatastoreView, UpdateDatastore } from '../types'

/** Find a datastore in the cached list by qualified name or id (instant placeholder for details). */
export function cachedDatastore(qc: QueryClient, ref: string): DatastoreView | undefined {
  return qc.getQueryData<DatastoreView[]>(keys.datastoreList())?.find((d) => isRef(d, ref))
}

/** `GET /api/v1/datastores` */
export function useDatastores() {
  const refetchInterval = usePollInterval(5000)
  return useQuery({
    queryKey: keys.datastoreList(),
    queryFn: ({ signal }) => endpoints.listDatastores(signal),
    refetchInterval,
  })
}

/** `GET /api/v1/datastores/{ref}` — polls while `creating`. */
export function useDatastore(ref: string | undefined) {
  const qc = useQueryClient()
  const poll = usePollInterval(2000)
  return useQuery<DatastoreView, Error, DatastoreView, ReturnType<typeof keys.datastore>>({
    queryKey: keys.datastore(ref ?? ''),
    queryFn: ({ signal }) => endpoints.getDatastore(ref ?? '', signal),
    enabled: Boolean(ref),
    placeholderData: () => (ref ? cachedDatastore(qc, ref) : undefined),
    refetchInterval: (q) => (q.state.data?.status === 'creating' ? poll : false),
  })
}

/** `POST /api/v1/datastores` → 201 (status `creating`, then provisioned). */
export function useCreateDatastore() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (body: CreateDatastore) => endpoints.createDatastore(body),
    onSuccess: (ds) => {
      qc.setQueryData(keys.datastore(qualifiedName(ds)), ds)
      void qc.invalidateQueries({ queryKey: keys.datastoreList() })
      // Environments list their datastores.
      void qc.invalidateQueries({ queryKey: keys.environments() })
    },
  })
}

/** `PATCH /api/v1/datastores/{ref}` — resource limits, applied to the running container in place. */
export function useUpdateDatastore(ref: string) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (body: UpdateDatastore) => endpoints.updateDatastore(ref, body),
    onSuccess: (ds) => {
      qc.setQueryData(keys.datastore(ref), ds)
      const qualified = qualifiedName(ds)
      if (ref !== qualified) qc.setQueryData(keys.datastore(qualified), ds)
      qc.setQueryData<DatastoreView[]>(keys.datastoreList(), (list) =>
        list ? list.map((d) => (d.id === ds.id ? ds : d)) : list,
      )
      void qc.invalidateQueries({ queryKey: keys.datastores() })
    },
    // The limits may be saved even when applying them to the container failed.
    onError: () => void qc.invalidateQueries({ queryKey: keys.datastores() }),
  })
}

/** `DELETE /api/v1/datastores/{ref}?force=` (409 while referenced unless force). */
export function useDeleteDatastore() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({ ref, force = false }: { ref: string; force?: boolean }) => endpoints.deleteDatastore(ref, force),
    onSuccess: (_v, { ref }) => {
      const cached = cachedDatastore(qc, ref)
      qc.setQueryData<DatastoreView[]>(keys.datastoreList(), (list) => list?.filter((d) => !isRef(d, ref)))
      qc.removeQueries({ queryKey: keys.datastore(ref) })
      if (cached) qc.removeQueries({ queryKey: keys.datastore(qualifiedName(cached)) })
      void qc.invalidateQueries({ queryKey: keys.datastores() })
      void qc.invalidateQueries({ queryKey: keys.environments() })
    },
  })
}
