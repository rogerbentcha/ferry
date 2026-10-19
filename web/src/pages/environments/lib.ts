/**
 * Helpers of the environment pages and of the places that name an
 * environment (services, datastores): name validation (mirrors
 * `validate::environment_name`), paths, ordering, the environment filter, and
 * when the dashboard shows environments at all.
 */

import { useEnvironments } from '@/lib/api/queries'
import { PRODUCTION, type EnvironmentView } from '@/lib/api/types'
import { plural } from '@/lib/format'

/** `/environments/<name>` with the name encoded. */
export function environmentPath(name: string): string {
  return `/environments/${encodeURIComponent(name)}`
}

/** `/services/new` with the environment picked (production is the form's default). */
export function newServicePath(environment: string): string {
  return environment === PRODUCTION ? '/services/new' : `/services/new?environment=${encodeURIComponent(environment)}`
}

const RESERVED_NAMES = new Set(['ferry', 'localhost'])
const ID_PREFIXES = ['srv', 'dep', 'job', 'dbs', 'evg', 'git', 'env']

/** `validate::looks_like_id`: a prefix, `-` and 20 lowercase hex characters (`env-1a2b…`). */
function looksLikeId(name: string): boolean {
  return ID_PREFIXES.some(
    (p) => name.length === p.length + 21 && name.startsWith(`${p}-`) && /^[0-9a-f]+$/.test(name.slice(p.length + 1)),
  )
}

/**
 * `validate::environment_name` (the rules of service names: it is a DNS
 * label of `<service>.<environment>.<domain>`), plus uniqueness against the
 * environments we know about. Returns an error message, or null when valid.
 */
export function environmentNameError(name: string, taken: ReadonlySet<string> = new Set()): string | null {
  if (name === '') return 'Name is required'
  if (name.length > 40) return 'Use at most 40 characters'
  if (!/^[a-z]/.test(name)) return 'Start with a lowercase letter'
  if (!/^[a-z0-9-]+$/.test(name)) return 'Use lowercase letters, digits and “-” only'
  if (name.endsWith('-')) return 'Don’t end the name with “-”'
  if (RESERVED_NAMES.has(name)) return `“${name}” is reserved`
  if (looksLikeId(name)) return 'This looks like a resource id; choose another name'
  if (taken.has(name)) return `An environment named “${name}” already exists`
  return null
}

/** Production first, then by name. */
export function sortEnvironments<T extends { name: string }>(list: readonly T[]): T[] {
  return [...list].sort((a, b) =>
    a.name === PRODUCTION ? -1 : b.name === PRODUCTION ? 1 : a.name.localeCompare(b.name),
  )
}

/**
 * Whether the dashboard names the environment of services and datastores:
 * only once there is more than production (unknown yet = no).
 */
export function showsEnvironments(environments: readonly { name: string }[] | undefined): boolean {
  return (environments?.length ?? 0) > 1
}

/**
 * The environment a list is filtered by: the chosen one while it exists and
 * there is more than one, otherwise none (`null` = every environment).
 */
export function activeEnvironment(chosen: string | null, environments: readonly { name: string }[] | undefined): string | null {
  if (!chosen || !showsEnvironments(environments)) return null
  return environments?.some((e) => e.name === chosen) ? chosen : null
}

/** The items of one environment (`null` = all of them). */
export function inEnvironment<T extends { environment: string }>(items: readonly T[], environment: string | null): T[] {
  return environment === null ? [...items] : items.filter((i) => i.environment === environment)
}

/** `2 services · 1 datastore`, or `Empty`. */
export function membersSummary(env: Pick<EnvironmentView, 'services' | 'datastores'>): string {
  if (env.services.length === 0 && env.datastores.length === 0) return 'Empty'
  return `${plural(env.services.length, 'service')} · ${plural(env.datastores.length, 'datastore')}`
}

/** The environments, and whether to name them next to services and datastores ({@link showsEnvironments}). */
export function useShownEnvironments(): { environments: EnvironmentView[] | undefined; shown: boolean } {
  const { data } = useEnvironments()
  return { environments: data, shown: showsEnvironments(data) }
}
