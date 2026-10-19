import { describe, expect, it } from 'vitest'

import { findInEnvironment, isRef, qualifiedName, splitRef } from './refs'

const prodApi = { id: 'srv-1', name: 'api', environment: 'production' }
const stagingApi = { id: 'srv-2', name: 'api', environment: 'staging' }

describe('qualifiedName (mirrors models::host_label)', () => {
  it('is the name alone in production, or without an environment', () => {
    expect(qualifiedName(prodApi)).toBe('api')
    expect(qualifiedName({ name: 'api' })).toBe('api')
  })
  it('adds the environment elsewhere', () => {
    expect(qualifiedName(stagingApi)).toBe('api.staging')
    expect(qualifiedName({ name: 'db', environment: 'pr-42' })).toBe('db.pr-42')
  })
})

describe('splitRef', () => {
  it('keeps ids and production names as they are', () => {
    expect(splitRef('api')).toEqual({ seg: 'api' })
    expect(splitRef('srv-0123456789abcdef0123')).toEqual({ seg: 'srv-0123456789abcdef0123' })
  })
  it('splits a qualified name into its name and environment', () => {
    expect(splitRef('api.staging')).toEqual({ seg: 'api', environment: 'staging' })
    expect(splitRef('my-db.pr-42')).toEqual({ seg: 'my-db', environment: 'pr-42' })
  })
  it('reads back what qualifiedName writes', () => {
    for (const r of [prodApi, stagingApi]) {
      const { seg, environment } = splitRef(qualifiedName(r))
      expect(seg).toBe(r.name)
      expect(environment ?? 'production').toBe(r.environment)
    }
  })
})

describe('isRef', () => {
  it('matches the id or the qualified name, never the bare name of another environment', () => {
    expect(isRef(stagingApi, 'srv-2')).toBe(true)
    expect(isRef(stagingApi, 'api.staging')).toBe(true)
    expect(isRef(stagingApi, 'api')).toBe(false)
    expect(isRef(prodApi, 'api')).toBe(true)
    expect(isRef(prodApi, 'api.staging')).toBe(false)
  })
})

describe('findInEnvironment', () => {
  it('finds the one of that name in that environment only', () => {
    const list = [prodApi, stagingApi]
    expect(findInEnvironment(list, 'api', 'staging')).toBe(stagingApi)
    expect(findInEnvironment(list, 'api', 'production')).toBe(prodApi)
    expect(findInEnvironment(list, 'api', 'qa')).toBeUndefined()
    expect(findInEnvironment(undefined, 'api', 'staging')).toBeUndefined()
  })
})
