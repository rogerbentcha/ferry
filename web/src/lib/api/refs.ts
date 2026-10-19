/**
 * How the dashboard names one service or datastore. A name is unique within
 * its environment only (`api` of production and `api` of staging are two
 * services), so outside of it the dashboard says `<name>.<environment>`: the
 * qualified name, the name alone in production (`hostLabel`, like its
 * hostnames). Names and environments never hold a `.`, and ids neither, so
 * the API reads one back (`splitRef`): the name in the path, its environment
 * in `?environment=`.
 *
 * A `ref` is an id or a qualified name: the `:name` of the service and
 * datastore routes, the key of their cached views, what the endpoints take.
 * Server-wide lists (an env group's services, a git connection's, the
 * metrics) hold qualified names; an environment's own lists, plain names.
 */

import { hostLabel } from './types'

/** `api` in production (or without an environment), `api.staging` elsewhere. */
export function qualifiedName(r: { name: string; environment?: string }): string {
  return hostLabel(r.name, r.environment)
}

/**
 * A ref as the API takes it: the path segment, and the environment of a
 * qualified name outside production (none for an id or a production name).
 */
export function splitRef(ref: string): { seg: string; environment?: string } {
  const dot = ref.indexOf('.')
  return dot === -1 ? { seg: ref } : { seg: ref.slice(0, dot), environment: ref.slice(dot + 1) }
}

/** Whether `ref` is the id or the qualified name of `r` (never a bare name of another environment). */
export function isRef(r: { id: string; name: string; environment?: string }, ref: string): boolean {
  return r.id === ref || qualifiedName(r) === ref
}

/**
 * The one named `name` in `environment`: what `${{service.NAME.…}}` and
 * `${{datastore.NAME.…}}` mean in the variables of a service of that
 * environment (a reference never leaves it).
 */
export function findInEnvironment<T extends { name: string; environment: string }>(
  list: readonly T[] | undefined,
  name: string,
  environment: string,
): T | undefined {
  return list?.find((x) => x.name === name && x.environment === environment)
}
