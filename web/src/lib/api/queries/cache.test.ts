import { QueryClient } from '@tanstack/react-query'
import { describe, expect, it } from 'vitest'

import { keys } from '../keys'
import type { DatastoreView, ServiceView } from '../types'
import { cachedDatastore } from './datastores'
import { cachedService, putService } from './services'

// Only what the cache helpers read.
const service = (id: string, name: string, environment: string) => ({ id, name, environment }) as ServiceView
const datastore = (id: string, name: string, environment: string) => ({ id, name, environment }) as DatastoreView

const prodApi = service('srv-1', 'api', 'production')
const stagingApi = service('srv-2', 'api', 'staging')

function client(): QueryClient {
  const qc = new QueryClient()
  qc.setQueryData(keys.serviceList(), [prodApi, stagingApi])
  qc.setQueryData(keys.datastoreList(), [datastore('dbs-1', 'db', 'production'), datastore('dbs-2', 'db', 'staging')])
  return qc
}

describe('cachedService / cachedDatastore', () => {
  it('find one by qualified name or id, never by a bare name of another environment', () => {
    const qc = client()
    expect(cachedService(qc, 'api')).toBe(prodApi)
    expect(cachedService(qc, 'api.staging')).toBe(stagingApi)
    expect(cachedService(qc, 'srv-2')).toBe(stagingApi)
    expect(cachedService(qc, 'api.qa')).toBeUndefined()
    expect(cachedDatastore(qc, 'db.staging')?.id).toBe('dbs-2')
    expect(cachedDatastore(qc, 'db')?.id).toBe('dbs-1')
  })
})

describe('putService', () => {
  it('caches the view under its ref and its qualified name, not under the bare name', () => {
    const qc = client()
    const fresh = { ...stagingApi, instances: 3 } as ServiceView
    putService(qc, 'srv-2', fresh)
    expect(qc.getQueryData(keys.service('srv-2'))).toBe(fresh)
    expect(qc.getQueryData(keys.service('api.staging'))).toBe(fresh)
    expect(qc.getQueryData(keys.service('api'))).toBeUndefined()
    expect(qc.getQueryData<ServiceView[]>(keys.serviceList())).toEqual([prodApi, fresh])
  })
})
