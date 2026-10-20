// Tests of npm.mjs: `node --test .github/scripts/npm.test.mjs`.

import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { after, describe, test } from 'node:test'
import { fileURLToPath } from 'node:url'

import { PLATFORMS } from '../../npm/ferry-cli/lib/platform.js'
import { checkMain, platformManifest, platformReadme, publishOrder } from './npm.mjs'

const ROOT = fileURLToPath(new URL('../..', import.meta.url))
const MAIN = JSON.parse(readFileSync(join(ROOT, 'npm/ferry-cli/package.json'), 'utf8'))
const linuxX64 = PLATFORMS.find((p) => p.package === 'ferry-cli-linux-x64')
const darwinArm64 = PLATFORMS.find((p) => p.package === 'ferry-cli-darwin-arm64')

describe('platformManifest', () => {
  test('npm installs a binary package on its own system only', () => {
    const m = platformManifest(darwinArm64, MAIN)
    assert.equal(m.name, 'ferry-cli-darwin-arm64')
    assert.equal(m.version, MAIN.version)
    assert.deepEqual(m.os, ['darwin'])
    assert.deepEqual(m.cpu, ['arm64'])
    assert.equal(m.libc, undefined)
    assert.deepEqual(m.files, ['bin/ferry'])
    assert.equal(m.bin, undefined, 'the command is the launcher of ferry-cli, not the binary package')
  })

  test('the Linux packages are for glibc', () => {
    assert.deepEqual(platformManifest(linuxX64, MAIN).libc, ['glibc'])
  })

  test('provenance needs the repository of the workflow', () => {
    assert.equal(platformManifest(linuxX64, MAIN).repository.url, 'git+https://github.com/rogerbentcha/ferry.git')
    assert.equal(MAIN.repository.url, 'git+https://github.com/rogerbentcha/ferry.git')
  })

  test('the npm page of a binary package points to ferry-cli', () => {
    assert.match(platformManifest(linuxX64, MAIN).description, /Install ferry-cli, not this package/)
    assert.match(platformReadme(linuxX64), /npm install -g ferry-cli/)
  })
})

describe('checkMain', () => {
  test('the real ferry-cli depends on every binary package, at its own version', () => {
    checkMain(MAIN)
  })

  test('a missing or extra binary package', () => {
    const { 'ferry-cli-linux-x64': _, ...rest } = MAIN.optionalDependencies
    assert.throws(() => checkMain({ ...MAIN, optionalDependencies: rest }), /not ferry-cli-darwin-arm64/)
    const extra = { ...MAIN.optionalDependencies, 'ferry-cli-win32-x64': MAIN.version }
    assert.throws(() => checkMain({ ...MAIN, optionalDependencies: extra }), /optionalDependencies are/)
  })

  test('a binary package at another version', () => {
    const deps = { ...MAIN.optionalDependencies, 'ferry-cli-linux-x64': '0.0.1' }
    assert.throws(() => checkMain({ ...MAIN, optionalDependencies: deps }), /depends on ferry-cli-linux-x64 0\.0\.1/)
  })
})

test('publishOrder: the binary packages before ferry-cli', () => {
  const order = publishOrder()
  assert.equal(order.at(-1), 'ferry-cli')
  assert.deepEqual(order.slice(0, -1).sort(), PLATFORMS.map((p) => p.package).sort())
})

/** npm.mjs in the repository, with the output of the command. */
const npmMjs = (args, options = {}) =>
  execFileSync('node', ['.github/scripts/npm.mjs', ...args], {
    cwd: ROOT,
    encoding: 'utf8',
    stdio: 'pipe',
    ...options,
  })

/**
 * A temporary folder with release archives in `assets`: a `ferry` per archive
 * that says which one it is, next to a ferryd, as the release archives hold them.
 */
function releaseAssets() {
  const dir = mkdtempSync(join(tmpdir(), 'ferry-npm-'))
  after(() => rmSync(dir, { recursive: true, force: true }))
  const assets = join(dir, 'assets')
  mkdirSync(assets)
  for (const p of PLATFORMS) {
    const bin = join(dir, 'bin', p.target)
    mkdirSync(bin, { recursive: true })
    writeFileSync(join(bin, 'ferry'), `#!/bin/sh\necho ${p.target}\n`)
    chmodSync(join(bin, 'ferry'), 0o755)
    writeFileSync(join(bin, 'ferryd'), 'not for npm\n')
    execFileSync('tar', ['-czf', join(assets, `ferry-${p.target}.tar.gz`), '-C', bin, 'ferry', 'ferryd'])
  }
  return { dir, assets, out: join(dir, 'out') }
}

describe('stage, from release archives', () => {
  const { assets, out } = releaseAssets()
  const stage = () => npmMjs(['stage', assets, out])

  test('one folder per package, printed in publish order', () => {
    const printed = stage().trim().split('\n')
    assert.deepEqual(printed, publishOrder().map((name) => join(out, name)))
  })

  test('each binary package holds the ferry of its archive, executable', () => {
    for (const p of PLATFORMS) {
      const ferry = join(out, p.package, 'bin', 'ferry')
      assert.equal(execFileSync(ferry, { encoding: 'utf8' }).trim(), p.target)
      assert.equal(statSync(ferry).mode & 0o777, 0o755)
      const manifest = JSON.parse(readFileSync(join(out, p.package, 'package.json'), 'utf8'))
      assert.deepEqual(manifest, platformManifest(p, MAIN))
      assert.ok(readFileSync(join(out, p.package, 'LICENSE'), 'utf8').includes('MIT'))
    }
  })

  test('ferry-cli: the launcher and its package.json, without the tests', () => {
    const main = join(out, 'ferry-cli')
    assert.deepEqual(JSON.parse(readFileSync(join(main, 'package.json'), 'utf8')), MAIN)
    for (const file of ['bin/ferry.js', 'lib/platform.js', 'README.md', 'LICENSE']) statSync(join(main, file))
    assert.throws(() => statSync(join(main, 'test')), /ENOENT/)
  })

  test('a missing archive stops the stage', () => {
    rmSync(join(assets, 'ferry-x86_64-apple-darwin.tar.gz'))
    assert.throws(stage, /ferry-x86_64-apple-darwin\.tar\.gz is missing/)
  })
})

describe('publish, with a fake npm', () => {
  const { dir, assets, out } = releaseAssets()
  npmMjs(['stage', assets, out])
  // npm on PATH logs its arguments. `npm view NAME@VERSION version` prints the
  // version for the packages of ON_NPM, nothing for ferry-cli-linux-arm64 (the
  // package exists, not this version), and fails with E404 for the others, as
  // npm 11 does. `npm publish` fails for the package of FAIL.
  const fake = join(dir, 'fake-npm')
  mkdirSync(fake)
  writeFileSync(
    join(fake, 'npm'),
    `#!/bin/sh
echo "$*" >> "$NPM_LOG"
if [ "$1" = view ]; then
  name=\${2%@*}
  case " $ON_NPM " in *" $name "*) echo "\${2##*@}"; exit 0 ;; esac
  [ "$name" = ferry-cli-linux-arm64 ] && exit 0
  echo "npm error code E404" >&2
  exit 1
fi
case "$2" in *"/$FAIL") exit 1 ;; esac
`,
  )
  chmodSync(join(fake, 'npm'), 0o755)
  let runs = 0
  const publish = (env) => {
    const log = join(dir, `npm-${++runs}.log`)
    const path = `${fake}:${process.env.PATH}`
    const run = () => npmMjs(['publish', out], { env: { ...process.env, PATH: path, NPM_LOG: log, ...env } })
    const calls = () => readFileSync(log, 'utf8').trim().split('\n')
    return { run, calls }
  }
  const view = (name) => `view ${name}@${MAIN.version} version`
  const published = (name) => `publish ${join(out, name)} --access public --provenance`

  test('binary packages first, ferry-cli last, with provenance; a version on npm is skipped', () => {
    const { run, calls } = publish({ ON_NPM: 'ferry-cli-darwin-x64' })
    const printed = run()
    assert.match(printed, new RegExp(`ferry-cli-darwin-x64@${MAIN.version} is on npm already: skipped`))
    const expected = []
    for (const name of publishOrder()) {
      expected.push(view(name))
      if (name !== 'ferry-cli-darwin-x64') expected.push(published(name))
    }
    assert.deepEqual(calls(), expected)
  })

  test('a run again after a failed publish publishes the rest only', () => {
    const first = publish({ FAIL: 'ferry-cli-linux-arm64' })
    assert.throws(first.run)
    // ferry-cli never names a package that npm does not have.
    assert.ok(!first.calls().includes(published('ferry-cli')))
    const done = publishOrder().slice(0, publishOrder().indexOf('ferry-cli-linux-arm64'))
    const again = publish({ ON_NPM: done.join(' ') })
    again.run()
    assert.deepEqual(
      again.calls().filter((call) => call.startsWith('publish')),
      publishOrder().filter((name) => !done.includes(name)).map(published),
    )
  })
})
