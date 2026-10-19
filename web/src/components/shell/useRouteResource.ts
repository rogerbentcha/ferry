import { useMatch } from 'react-router'

export type RouteResource =
  | { kind: 'service'; name: string }
  | { kind: 'datastore'; name: string }
  | { kind: 'env-group'; name: string }
  | { kind: 'environment'; name: string }
  | { kind: null; name: null }

/**
 * Which resource detail page is open (drives the breadcrumb switchers, ⌘K
 * actions, Connect). `name` is the route's: a service or a datastore by its
 * qualified name (`api.staging`) or id.
 */
export function useRouteResource(): RouteResource {
  const service = useMatch('/services/:name/*')
  const datastore = useMatch('/datastores/:name/*')
  const group = useMatch('/env-groups/:name/*')
  const environment = useMatch('/environments/:name/*')
  const sName = service?.params.name
  if (sName && sName !== 'new') return { kind: 'service', name: sName }
  if (datastore?.params.name) return { kind: 'datastore', name: datastore.params.name }
  if (group?.params.name) return { kind: 'env-group', name: group.params.name }
  if (environment?.params.name) return { kind: 'environment', name: environment.params.name }
  return { kind: null, name: null }
}
