// Tests of the launcher's choice of package: `node --test npm/ferry-cli/test/platform.test.js`.

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import { PLATFORMS, platformPackage, thisSystem } from '../lib/platform.js'

const SUPPORTED =
  /macOS 14 or newer \(Apple Silicon or Intel\), and on Linux x86_64 or arm64 with glibc 2\.35 or newer/

describe('platformPackage', () => {
  test('each supported system gets its package', () => {
    const pick = (system) => platformPackage(system).package
    assert.equal(pick({ os: 'darwin', cpu: 'arm64', darwin: 23 }), 'ferry-cli-darwin-arm64')
    assert.equal(pick({ os: 'darwin', cpu: 'x64', darwin: 25 }), 'ferry-cli-darwin-x64')
    assert.equal(pick({ os: 'linux', cpu: 'x64', glibc: '2.35' }), 'ferry-cli-linux-x64')
    assert.equal(pick({ os: 'linux', cpu: 'arm64', glibc: '2.39' }), 'ferry-cli-linux-arm64')
    assert.deepEqual(Object.keys(platformPackage({ os: 'linux', cpu: 'x64', glibc: '2.35' })), ['package'])
  })

  test('Windows: not supported yet, and the message names the supported systems', () => {
    const { error } = platformPackage({ os: 'win32', cpu: 'x64' })
    assert.match(error, /^Windows x86_64 is not supported yet\./)
    assert.match(error, SUPPORTED)
  })

  test('another CPU of a supported system', () => {
    const error = (cpu) => platformPackage({ os: 'linux', cpu, glibc: '2.39' }).error
    assert.match(error('ia32'), /^Linux x86 is not supported yet\./)
    assert.match(error('s390x'), /^Linux s390x is not supported yet\./)
  })

  test('Alpine and other musl Linux: no glibc', () => {
    const { error } = platformPackage({ os: 'linux', cpu: 'x64', glibc: undefined })
    assert.match(error, /no glibc \(Alpine and other musl systems are not supported yet\)/)
    assert.match(error, SUPPORTED)
  })

  test('glibc older than 2.35', () => {
    assert.match(platformPackage({ os: 'linux', cpu: 'x64', glibc: '2.31' }).error, /glibc 2\.31, older than 2\.35/)
    assert.match(platformPackage({ os: 'linux', cpu: 'x64', glibc: '2.4' }).error, /glibc 2\.4, older than 2\.35/)
    assert.equal(platformPackage({ os: 'linux', cpu: 'x64', glibc: '2.36' }).package, 'ferry-cli-linux-x64')
    assert.equal(platformPackage({ os: 'linux', cpu: 'x64', glibc: '3.0' }).package, 'ferry-cli-linux-x64')
  })

  test('macOS older than 14 (Darwin 23)', () => {
    const { error } = platformPackage({ os: 'darwin', cpu: 'arm64', darwin: 22 })
    assert.match(error, /older than macOS 14/)
    assert.match(error, SUPPORTED)
  })
})

describe('PLATFORMS', () => {
  test('one package per system, named after Node’s platform and arch', () => {
    for (const p of PLATFORMS) assert.equal(p.package, `ferry-cli-${p.os}-${p.cpu}`)
    assert.equal(new Set(PLATFORMS.map((p) => p.package)).size, PLATFORMS.length)
  })
})

describe('thisSystem', () => {
  test('reads this machine', () => {
    const system = thisSystem()
    assert.equal(system.os, process.platform)
    assert.equal(system.cpu, process.arch)
    if (system.os === 'darwin') assert.ok(system.darwin >= 20, `Darwin ${system.darwin}`)
    if (system.os === 'linux') assert.equal(typeof system.glibc, 'string', 'the test runs on a glibc Linux')
  })
})
