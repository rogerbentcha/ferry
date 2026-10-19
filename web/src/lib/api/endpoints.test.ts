import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { buildUrl } from './client'
import { endpoints, resourcePath, streamPaths } from './endpoints'

describe('resourcePath', () => {
  it('names a production service or an id in the path alone', () => {
    expect(resourcePath('services', 'api')).toBe('/api/v1/services/api')
    expect(resourcePath('services', 'srv-0123456789abcdef0123', '/status')).toBe(
      '/api/v1/services/srv-0123456789abcdef0123/status',
    )
    expect(resourcePath('datastores', 'db')).toBe('/api/v1/datastores/db')
  })
  it('puts the environment of a qualified name in the query', () => {
    expect(resourcePath('services', 'api.staging', '/deploys')).toBe('/api/v1/services/api/deploys?environment=staging')
    expect(resourcePath('datastores', 'db.pr-42')).toBe('/api/v1/datastores/db?environment=pr-42')
  })
})

describe('buildUrl', () => {
  it('adds the parameters after the environment of the path', () => {
    expect(buildUrl('/api/v1/services/api?environment=staging', { force: true })).toBe(
      '/api/v1/services/api?environment=staging&force=true',
    )
    expect(buildUrl('/api/v1/services/api?environment=staging', { restart: undefined })).toBe(
      '/api/v1/services/api?environment=staging',
    )
    expect(buildUrl('/api/v1/services/api', { limit: 20 })).toBe('/api/v1/services/api?limit=20')
  })
  it('streams the runtime logs of a service of another environment', () => {
    expect(buildUrl(streamPaths.serviceLogs('api.staging'), { follow: true, tail: 500 })).toBe(
      '/api/v1/services/api/logs?environment=staging&follow=true&tail=500',
    )
    expect(buildUrl(streamPaths.serviceLogs('api'), { follow: true })).toBe('/api/v1/services/api/logs?follow=true')
  })
})

describe('endpoints of a service or datastore', () => {
  let requests: string[]

  beforeEach(() => {
    requests = []
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string) => {
        requests.push(String(url))
        return new Response(null, { status: 204 })
      }),
    )
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('look a production name up without an environment', async () => {
    await endpoints.getService('api')
    await endpoints.listDeploys('api', 5)
    await endpoints.deleteDatastore('db', true)
    expect(requests).toEqual([
      '/api/v1/services/api',
      '/api/v1/services/api/deploys?limit=5',
      '/api/v1/datastores/db?force=true',
    ])
  })

  it('look a qualified name up in its environment, with their own parameters', async () => {
    await endpoints.getService('api.staging')
    await endpoints.deleteService('api.staging', true)
    await endpoints.listJobs('api.staging', 20)
    await endpoints.replaceServiceEnv('api.staging', { vars: [] }, true)
    await endpoints.unlinkEnvGroup('api.staging', 'shared')
    await endpoints.removeDomain('api.staging', 'app.example.com')
    await endpoints.updateDatastore('db.staging', {})
    expect(requests).toEqual([
      '/api/v1/services/api?environment=staging',
      '/api/v1/services/api?environment=staging&force=true',
      '/api/v1/services/api/jobs?environment=staging&limit=20',
      '/api/v1/services/api/env?environment=staging&restart=true',
      '/api/v1/services/api/env-groups/shared?environment=staging',
      '/api/v1/services/api/domains/app.example.com?environment=staging',
      '/api/v1/datastores/db?environment=staging',
    ])
  })

  it('take an id whatever its environment', async () => {
    await endpoints.restartService('srv-0123456789abcdef0123')
    expect(requests).toEqual(['/api/v1/services/srv-0123456789abcdef0123/restart'])
  })
})
