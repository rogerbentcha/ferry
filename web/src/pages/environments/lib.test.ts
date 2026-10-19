import { describe, expect, it } from 'vitest'

import { hostLabel } from '@/lib/api/types'
import { previewServiceUrl } from '@/pages/services/lib'

import {
  activeEnvironment,
  environmentNameError,
  environmentPath,
  inEnvironment,
  membersSummary,
  newServicePath,
  showsEnvironments,
  sortEnvironments,
} from './lib'

const envs = (...names: string[]) => names.map((name) => ({ name }))

describe('environment names (mirror validate::environment_name)', () => {
  it('accepts the rules of service names', () => {
    expect(environmentNameError('staging')).toBeNull()
    expect(environmentNameError('pr-42')).toBeNull()
    // only a real id shape is refused
    expect(environmentNameError('env-db')).toBeNull()
  })
  it('refuses bad shapes, reserved names, ids and taken names', () => {
    expect(environmentNameError('')).not.toBeNull()
    expect(environmentNameError('Staging')).not.toBeNull()
    expect(environmentNameError('2nd')).not.toBeNull()
    expect(environmentNameError('qa_1')).not.toBeNull()
    expect(environmentNameError('qa-')).not.toBeNull()
    expect(environmentNameError('a'.repeat(41))).not.toBeNull()
    expect(environmentNameError('localhost')).toMatch(/reserved/)
    expect(environmentNameError('env-0123456789abcdef0123')).toMatch(/resource id/)
    expect(environmentNameError('production', new Set(['production']))).toMatch(/already exists/)
  })
})

describe('host labels and URL previews (mirror models::host_label)', () => {
  const info = { base_domain: 'example.com', proxy_url: 'http://0.0.0.0:80', tls_enabled: true, dashboard_url: null }
  it('keeps <name> in production and adds the environment elsewhere', () => {
    expect(hostLabel('shop', 'production')).toBe('shop')
    expect(hostLabel('shop')).toBe('shop')
    expect(hostLabel('shop', 'staging')).toBe('shop.staging')
  })
  it('previews <name>.<env>.<domain> outside production', () => {
    expect(previewServiceUrl('shop', info)).toBe('https://shop.example.com')
    expect(previewServiceUrl('shop', info, 'production')).toBe('https://shop.example.com')
    expect(previewServiceUrl('shop', info, 'staging')).toBe('https://shop.staging.example.com')
    // local domains keep the proxy's port
    expect(
      previewServiceUrl('shop', { ...info, base_domain: 'localhost', tls_enabled: false, proxy_url: 'http://127.0.0.1:19801' }, 'staging'),
    ).toBe('http://shop.staging.localhost:19801')
  })
})

describe('environment filter', () => {
  const services = [
    { name: 'shop', environment: 'production' },
    { name: 'shop-next', environment: 'staging' },
    { name: 'worker', environment: 'staging' },
  ]
  it('names environments only once there is more than production', () => {
    expect(showsEnvironments(undefined)).toBe(false)
    expect(showsEnvironments(envs('production'))).toBe(false)
    expect(showsEnvironments(envs('production', 'staging'))).toBe(true)
  })
  it('keeps the chosen environment while it exists', () => {
    expect(activeEnvironment('staging', envs('production', 'staging'))).toBe('staging')
    expect(activeEnvironment(null, envs('production', 'staging'))).toBeNull()
    // deleted since it was chosen, or nothing to choose from: every environment
    expect(activeEnvironment('qa', envs('production', 'staging'))).toBeNull()
    expect(activeEnvironment('staging', envs('production'))).toBeNull()
    expect(activeEnvironment('staging', undefined)).toBeNull()
  })
  it('filters by environment', () => {
    expect(inEnvironment(services, 'staging').map((s) => s.name)).toEqual(['shop-next', 'worker'])
    expect(inEnvironment(services, null)).toHaveLength(3)
    expect(inEnvironment(services, 'qa')).toEqual([])
  })
})

describe('environment list helpers', () => {
  it('sorts production first, then by name', () => {
    expect(sortEnvironments(envs('staging', 'dev', 'production')).map((e) => e.name)).toEqual([
      'production',
      'dev',
      'staging',
    ])
  })
  it('summarizes what an environment holds', () => {
    expect(membersSummary({ services: [], datastores: [] })).toBe('Empty')
    expect(membersSummary({ services: ['shop', 'worker'], datastores: ['shop-db'] })).toBe('2 services · 1 datastore')
  })
  it('links to an environment and to a new service in it', () => {
    expect(environmentPath('staging')).toBe('/environments/staging')
    expect(newServicePath('production')).toBe('/services/new')
    expect(newServicePath('staging')).toBe('/services/new?environment=staging')
  })
})
