import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { endpoints } from '../endpoints'
import { usePollInterval } from '../events'
import { keys } from '../keys'
import type { CreateEnvironment, EnvironmentView } from '../types'

/** `GET /api/v1/environments` — production first. */
export function useEnvironments() {
  const refetchInterval = usePollInterval(10_000)
  return useQuery({
    queryKey: keys.environmentList(),
    queryFn: ({ signal }) => endpoints.listEnvironments(signal),
    refetchInterval,
  })
}

/** `GET /api/v1/environments/{ref}` — placeholder from the list cache while loading. */
export function useEnvironment(ref: string | undefined) {
  const qc = useQueryClient()
  return useQuery<EnvironmentView, Error, EnvironmentView, ReturnType<typeof keys.environment>>({
    queryKey: keys.environment(ref ?? ''),
    queryFn: ({ signal }) => endpoints.getEnvironment(ref ?? '', signal),
    enabled: Boolean(ref),
    placeholderData: () =>
      qc.getQueryData<EnvironmentView[]>(keys.environmentList())?.find((e) => e.name === ref || e.id === ref),
  })
}

/** `POST /api/v1/environments` → 201 (400 invalid name, 409 taken, 502 Docker refused its network). */
export function useCreateEnvironment() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (body: CreateEnvironment) => endpoints.createEnvironment(body),
    onSuccess: (env) => {
      qc.setQueryData(keys.environment(env.name), env)
      qc.setQueryData<EnvironmentView[]>(keys.environmentList(), (list) =>
        list && !list.some((e) => e.id === env.id) ? [...list, env] : list,
      )
      void qc.invalidateQueries({ queryKey: keys.environmentList() })
    },
  })
}

/** `DELETE /api/v1/environments/{ref}` (409 for production, or while it holds services or datastores). */
export function useDeleteEnvironment() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (ref: string) => endpoints.deleteEnvironment(ref),
    onSuccess: (_void, ref) => {
      qc.setQueryData<EnvironmentView[]>(keys.environmentList(), (list) =>
        list?.filter((e) => e.name !== ref && e.id !== ref),
      )
      qc.removeQueries({ queryKey: keys.environment(ref) })
      void qc.invalidateQueries({ queryKey: keys.environments() })
    },
  })
}
