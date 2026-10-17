import { useQuery } from '@tanstack/react-query'

import { endpoints } from '../endpoints'
import { keys } from '../keys'

/** How often the Monitoring section asks for fresh numbers. */
export const SERVER_METRICS_INTERVAL_MS = 5_000

/**
 * `GET /api/v1/server/metrics` — how busy the machine and its containers
 * are. Polled every 5 s while the tab is visible: TanStack Query stops the
 * interval in a hidden tab and asks again when it comes back (always stale,
 * so coming back or reopening the section never shows old numbers alone).
 */
export function useServerMetrics() {
  return useQuery({
    queryKey: keys.serverMetrics(),
    queryFn: ({ signal }) => endpoints.serverMetrics(signal),
    refetchInterval: SERVER_METRICS_INTERVAL_MS,
    refetchIntervalInBackground: false,
    refetchOnWindowFocus: true,
    staleTime: 0,
  })
}
