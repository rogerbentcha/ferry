import { keepPreviousData, useQuery } from '@tanstack/react-query'

import { endpoints } from '../endpoints'
import { keys } from '../keys'
import type { MetricsRange } from '../types'

/** How often the Monitoring page asks for fresh numbers. */
export const SERVER_METRICS_INTERVAL_MS = 5_000

/**
 * `GET /api/v1/server/metrics` — how busy the machine and its containers
 * are. Polled every 5 s while the tab is visible: TanStack Query stops the
 * interval in a hidden tab and asks again when it comes back (always stale,
 * so coming back or reopening the page never shows old numbers alone).
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

/** How often the history charts ask again: ferryd reads the server every 10 s. */
export const METRICS_HISTORY_INTERVAL_MS = 10_000

/**
 * `GET /api/v1/server/metrics/history?range=` — the last hour, 6 hours or 24 hours of the server's
 * metrics. Polled every 10 s while the tab is visible, like `useServerMetrics`; switching the range
 * keeps the previous charts until the new ones arrive.
 */
export function useMetricsHistory(range: MetricsRange) {
  return useQuery({
    queryKey: keys.metricsHistory(range),
    queryFn: ({ signal }) => endpoints.metricsHistory(range, signal),
    refetchInterval: METRICS_HISTORY_INTERVAL_MS,
    refetchIntervalInBackground: false,
    refetchOnWindowFocus: true,
    staleTime: 0,
    placeholderData: keepPreviousData,
  })
}
