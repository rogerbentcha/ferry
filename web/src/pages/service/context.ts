import { useOutletContext } from 'react-router'

import type { ServiceView } from '@/lib/api/types'

export interface ServiceOutletContext {
  /** The loaded service (always present inside service pages). */
  service: ServiceView
  /**
   * The `:name` route param: the service's qualified name (`api`,
   * `api.staging`) or its id (use it as `ref` for queries/mutations).
   */
  name: string
}

/** Inside `/services/:name/*` pages: the service loaded by ServiceLayout. */
export function useServiceOutlet(): ServiceOutletContext {
  return useOutletContext<ServiceOutletContext>()
}

/**
 * `/services/<ref><sub>` with the ref encoded. `ref` is the service's
 * qualified name (`qualifiedName(s)`: `api` in production, `api.staging`
 * elsewhere) or its id, never its bare name outside production.
 */
export function servicePath(ref: string, sub = ''): string {
  return `/services/${encodeURIComponent(ref)}${sub}`
}
