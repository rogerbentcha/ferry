// Tests of the `ferry` launcher, with a fake ferry binary in a temporary
// node_modules: `node --test npm/ferry-cli/test/launcher.test.js`.

import assert from 'node:assert/strict'
import { spawn, spawnSync } from 'node:child_process'
import { chmodSync, cpSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { after, describe, test } from 'node:test'
import { fileURLToPath } from 'node:url'

import { platformPackage, thisSystem } from '../lib/platform.js'

const PACKAGE_DIR = fileURLToPath(new URL('..', import.meta.url))
const found = platformPackage(thisSystem())

describe('the ferry launcher', { skip: found.error && `no ferry for this system: ${found.error}` }, () => {
  const dir = mkdtempSync(join(tmpdir(), 'ferry-launcher-'))
  after(() => rmSync(dir, { recursive: true, force: true }))
  // As npm installs them: ferry-cli and the binary package of this system, side by side.
  const modules = join(dir, 'node_modules')
  const main = join(modules, 'ferry-cli')
  for (const path of ['package.json', 'bin', 'lib']) {
    cpSync(join(PACKAGE_DIR, path), join(main, path), { recursive: true })
  }
  const binary = join(modules, found.package)
  mkdirSync(join(binary, 'bin'), { recursive: true })
  writeFileSync(join(binary, 'package.json'), JSON.stringify({ name: found.package, version: '0.0.0' }))
  writeFileSync(
    join(binary, 'bin', 'ferry'),
    `#!/bin/sh
case "$1" in
  args) shift; printf '%s\\n' "$@" ;;
  exit) exit "$2" ;;
  kill) kill -"$2" $$ ;;
  wait) trap 'echo "got $2"; exit 3' "$2"; echo ready; while :; do sleep 0.1; done ;;
esac
`,
  )
  chmodSync(join(binary, 'bin', 'ferry'), 0o755)

  const launcher = join(main, 'bin', 'ferry.js')
  const ferry = (...args) => spawnSync(process.execPath, [launcher, ...args], { encoding: 'utf8' })

  test('runs the binary of this system with the same arguments', () => {
    const r = ferry('args', 'a b', '--json', '')
    assert.equal(r.status, 0)
    assert.equal(r.stdout, 'a b\n--json\n\n')
  })

  test('exits with the exit code of ferry', () => {
    assert.equal(ferry('exit', '0').status, 0)
    assert.equal(ferry('exit', '2').status, 2)
    assert.equal(ferry('exit', '130').status, 130)
  })

  test('ends with the signal that ended ferry', () => {
    assert.equal(ferry('kill', 'TERM').signal, 'SIGTERM')
    assert.equal(ferry('kill', 'KILL').signal, 'SIGKILL')
  })

  test('a signal Node ignores: the exit code is 128 + its number', () => {
    const r = ferry('kill', 'PIPE')
    assert.equal(r.status, 141)
  })

  for (const signal of ['SIGTERM', 'SIGINT']) {
    test(`a ${signal} sent to the launcher alone goes on to ferry`, async () => {
      const child = spawn(process.execPath, [launcher, 'wait', signal.slice(3)])
      let stdout = ''
      await new Promise((resolve) => {
        child.stdout.on('data', (chunk) => {
          stdout += chunk
          if (stdout.includes('ready')) resolve()
        })
      })
      child.kill(signal)
      const [code] = await new Promise((resolve) => child.on('exit', (...end) => resolve(end)))
      assert.equal(code, 3, 'the exit code of ferry, after its handler ran')
      assert.match(stdout, new RegExp(`got ${signal.slice(3)}`))
    })
  }

  test('without the binary package: a message, and exit code 1', () => {
    rmSync(binary, { recursive: true })
    const r = ferry('--version')
    assert.equal(r.status, 1)
    assert.match(r.stderr, new RegExp(`^ferry: the package ${found.package}, which holds the ferry of this system`))
    assert.match(r.stderr, /is not installed/)
    assert.match(r.stderr, /Install again: npm install -g ferry-cli/)
  })
})
