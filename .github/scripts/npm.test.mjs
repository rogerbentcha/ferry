// Tests of npm.mjs: `node --test .github/scripts/npm.test.mjs`.

import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { after, describe, test } from 'node:test'
import { fileURLToPath } from 'node:url'

import { PLATFORMS } from '../../npm/ferry-cli/lib/platform.js'
import {
  checkMain,
  cliChanges,
  cliInputs,
  gitRepo,
  manifestDependencies,
  npmPlan,
  platformManifest,
  platformReadme,
  publishOrder,
  tomlTables,
} from './npm.mjs'

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

describe('tomlTables', () => {
  test('tables, entries, arrays on several lines and comments', () => {
    const tables = tomlTables(`version = 4 # top

[workspace]
members = [
    "crates/a", # a comment in the array
    "crates/b",
]

[target.'cfg(unix)'.dependencies]
libc = "0.2"
"quoted" = { version = "1", features = ["a#b"] }

[[package]]
name = "x"

[[package]]
name = "y"
`)
    assert.deepEqual(tables, [
      { table: '', entries: [{ key: 'version', value: '4' }] },
      { table: 'workspace', entries: [{ key: 'members', value: '[ "crates/a", "crates/b", ]' }] },
      {
        table: "target.'cfg(unix)'.dependencies",
        entries: [
          { key: 'libc', value: '"0.2"' },
          { key: 'quoted', value: '{ version = "1", features = ["a#b"] }' },
        ],
      },
      { table: 'package', entries: [{ key: 'name', value: '"x"' }] },
      { table: 'package', entries: [{ key: 'name', value: '"y"' }] },
    ])
  })

  test('a line it cannot read, a value that does not end', () => {
    assert.throws(() => tomlTables('[a]\nnot toml\n'), /cannot read the TOML line: not toml/)
    assert.throws(() => tomlTables('members = [\n "a",\n'), /value of members does not end/)
  })
})

describe('manifestDependencies', () => {
  test('every form of Cargo.toml, with the name of each package in Cargo.lock', () => {
    const deps = manifestDependencies(`[package]
name = "ferry-cli"
version.workspace = true

[[bin]]
name = "ferry"
path = "src/main.rs"

[dependencies]
ferry-core.workspace = true
tokio = { workspace = true, features = ["full"] }
clap = "4"
local = { path = "../local" }
renamed = { version = "1", package = "real-name" }

[target.'cfg(unix)'.dependencies]
libc = "0.2"

[build-dependencies]
cc = "1"

[dependencies.table-form]
version = "2"
package = "table-real"

[dev-dependencies]
axum.workspace = true
tokio = { workspace = true, features = ["test-util"] }
`)
    const dep = (key, dev = false) => deps.find((d) => d.key === key && d.dev === dev)
    assert.deepEqual(dep('ferry-core'), {
      key: 'ferry-core',
      package: 'ferry-core',
      dev: false,
      workspace: true,
      path: null,
    })
    assert.equal(dep('tokio').workspace, true)
    assert.equal(dep('clap').workspace, false)
    assert.equal(dep('local').path, '../local')
    assert.equal(dep('renamed').package, 'real-name')
    assert.equal(dep('table-form').package, 'table-real')
    assert.ok(dep('libc') && dep('cc'), 'target and build dependencies build the binary too')
    assert.ok(dep('axum', true) && dep('tokio', true))
    assert.equal(dep('axum'), undefined)
    assert.equal(deps.length, 10)
  })
})

const REGISTRY = 'registry+https://github.com/rust-lang/crates.io-index'

/** A workspace like Ferry's: the CLI, the core it uses, and a server it does not use. */
function workspace() {
  const lockPackage = (name, version, deps = [], source = REGISTRY) =>
    [
      '[[package]]',
      `name = "${name}"`,
      `version = "${version}"`,
      ...(source ? [`source = "${source}"`] : []),
      ...(deps.length ? ['dependencies = [', ...deps.map((d) => ` "${d}",`), ']'] : []),
      '',
    ].join('\n')
  return {
    'Cargo.toml': `[workspace]
resolver = "3"
members = [
    "crates/ferry-core",
    "crates/ferry-cli",
    "crates/ferryd",
]

[workspace.package]
version = "0.1.0"

[workspace.dependencies]
ferry-core = { path = "crates/ferry-core" }
serde = { version = "1", features = ["derive"] } # the CLI and the server
bollard = "0.21"
axum = "0.8"

[profile.release]
strip = true
`,
    'Cargo.lock': [
      'version = 4\n',
      lockPackage('axum', '0.8.1', ['serde']),
      lockPackage('bollard', '0.21.0', ['itoa 0.4.8']),
      lockPackage('clap', '4.5.0'),
      lockPackage('ferry-cli', '0.1.0', ['axum', 'clap', 'ferry-core'], null),
      lockPackage('ferry-core', '0.1.0', ['serde'], null),
      lockPackage('ferryd', '0.1.0', ['axum', 'bollard', 'ferry-core'], null),
      lockPackage('itoa', '0.4.8'),
      lockPackage('itoa', '1.0.15'),
      lockPackage('serde', '1.0.200', ['itoa 1.0.15']),
    ].join('\n'),
    'crates/ferry-cli/Cargo.toml': `[package]
name = "ferry-cli"
version.workspace = true

[dependencies]
ferry-core.workspace = true
clap = "4"

[dev-dependencies]
axum.workspace = true
`,
    'crates/ferry-cli/src/main.rs': 'fn main() {}\n',
    'crates/ferry-cli/tests/cli.rs': '#[test]\nfn runs() {}\n',
    'crates/ferry-core/Cargo.toml': '[package]\nname = "ferry-core"\n\n[dependencies]\nserde.workspace = true\n',
    'crates/ferry-core/src/lib.rs': 'pub struct Service;\n',
    'crates/ferryd/Cargo.toml': `[package]
name = "ferryd"

[dependencies]
ferry-core.workspace = true
bollard.workspace = true
axum.workspace = true
`,
    'crates/ferryd/src/main.rs': 'fn main() {}\n',
    'npm/ferry-cli/bin/ferry.js': '// the launcher\n',
    'npm/ferry-cli/test/launcher.test.js': '// its tests\n',
  }
}

/** A repo of cliInputs over files in memory. */
const memoryRepo = (files) => ({
  read: (path) => files[path] ?? null,
  files: (dir) =>
    Object.keys(files)
      .filter((path) => path.startsWith(`${dir}/`))
      .map((path) => [path, createHash('sha1').update(files[path]).digest('hex')]),
})

const inputs = (files) => cliInputs(memoryRepo(files))

describe('cliInputs', () => {
  const lines = inputs(workspace())

  test('the files of the CLI, of the crates it uses and of the launcher, without their tests', () => {
    const files = lines.filter((l) => l.startsWith('file ')).map((l) => l.split(' ')[1])
    assert.deepEqual(files, [
      'crates/ferry-cli/Cargo.toml',
      'crates/ferry-cli/src/main.rs',
      'crates/ferry-core/Cargo.toml',
      'crates/ferry-core/src/lib.rs',
      'npm/ferry-cli/bin/ferry.js',
    ])
  })

  test('the packages of Cargo.lock it is built with: not its dev-dependencies, not those of the server', () => {
    assert.deepEqual(
      lines.filter((l) => l.startsWith('lock ')),
      [`lock clap 4.5.0 ${REGISTRY}`, `lock itoa 1.0.15 ${REGISTRY}`, `lock serde 1.0.200 ${REGISTRY}`],
    )
  })

  test('the root Cargo.toml it is built with: not the members, not the dependencies of the server', () => {
    assert.deepEqual(
      lines.filter((l) => l.startsWith('cargo ')),
      [
        'cargo [profile.release] strip = true',
        'cargo [workspace.dependencies] ferry-core = { path = "crates/ferry-core" }',
        'cargo [workspace.dependencies] serde = { version = "1", features = ["derive"] }',
        'cargo [workspace.package] version = "0.1.0"',
        'cargo [workspace] resolver = "3"',
      ],
    )
  })

  test('a missing file, a workspace dependency that is not in the root Cargo.toml', () => {
    const files = workspace()
    delete files['crates/ferry-core/Cargo.toml']
    assert.throws(() => inputs(files), /crates\/ferry-core\/Cargo\.toml is missing/)
    const unknown = workspace()
    unknown['crates/ferry-cli/Cargo.toml'] += '\n[dependencies.nope]\nworkspace = true\n'
    assert.throws(() => inputs(unknown), /nope is not in \[workspace.dependencies\]/)
  })
})

describe('cliChanges', () => {
  const before = inputs(workspace())
  const changesAfter = (change) => {
    const files = workspace()
    change(files)
    return cliChanges(before, inputs(files))
  }

  test('the server, the tests and the layout of the workspace do not change the CLI', () => {
    assert.deepEqual(changesAfter(() => {}), [])
    assert.deepEqual(
      changesAfter((f) => {
        f['crates/ferryd/src/main.rs'] = 'fn main() { serve() }\n'
        f['crates/ferry-cli/tests/cli.rs'] += '#[test]\nfn more() {}\n'
        f['npm/ferry-cli/test/launcher.test.js'] += '// more\n'
        f['Cargo.toml'] = f['Cargo.toml'].replace('bollard = "0.21"', 'bollard = "0.22"').replace(
          '"crates/ferryd",',
          '"crates/ferryd",\n    "crates/ferry-new",',
        )
        // A new bollard and its own itoa: the CLI uses another itoa.
        f['Cargo.lock'] = f['Cargo.lock']
          .replace('version = "0.21.0"', 'version = "0.22.0"')
          .replaceAll('0.4.8', '0.4.9')
      }),
      [],
    )
  })

  test('a dev-dependency of the CLI does not change it', () => {
    const axum = (f) => (f['Cargo.lock'] = f['Cargo.lock'].replace('version = "0.8.1"', 'version = "0.8.2"'))
    assert.deepEqual(changesAfter(axum), [])
  })

  test('the code of the CLI, of a crate it uses or of the launcher', () => {
    const cli = (f) => (f['crates/ferry-cli/src/main.rs'] = 'fn main() { run() }\n')
    const core = (f) => (f['crates/ferry-core/src/new.rs'] = 'pub struct Deploy;\n')
    const launcher = (f) => (f['npm/ferry-cli/bin/ferry.js'] += '// more\n')
    assert.deepEqual(changesAfter(cli), ['crates/ferry-cli'])
    assert.deepEqual(changesAfter(core), ['crates/ferry-core'])
    assert.deepEqual(changesAfter(launcher), ['npm/ferry-cli'])
  })

  test('a package it is built with, or an entry of Cargo.toml it uses', () => {
    assert.deepEqual(
      changesAfter((f) => (f['Cargo.lock'] = f['Cargo.lock'].replaceAll('1.0.15', '1.0.16'))),
      ['Cargo.lock (itoa)'],
    )
    assert.deepEqual(
      changesAfter((f) => {
        f['Cargo.toml'] = f['Cargo.toml']
          .replace('strip = true', 'strip = false')
          .replace('["derive"]', '["derive", "rc"]')
      }),
      ['Cargo.toml [profile.release]', 'Cargo.toml [workspace.dependencies]'],
    )
  })

  test('a new crate the CLI depends on counts with its packages', () => {
    assert.deepEqual(
      changesAfter((f) => {
        f['crates/ferry-cli/Cargo.toml'] = f['crates/ferry-cli/Cargo.toml'].replace(
          'clap = "4"',
          'clap = "4"\nferry-extra = { path = "../ferry-extra" }',
        )
        f['crates/ferry-extra/Cargo.toml'] =
          '[package]\nname = "ferry-extra"\n\n[dependencies]\nbollard.workspace = true\n'
        f['crates/ferry-extra/src/lib.rs'] = '\n'
        f['Cargo.lock'] = f['Cargo.lock']
          .replace('"clap",\n "ferry-core",', '"clap",\n "ferry-core",\n "ferry-extra",')
          .concat('\n[[package]]\nname = "ferry-extra"\nversion = "0.1.0"\ndependencies = [\n "bollard",\n]\n')
      }),
      [
        'crates/ferry-cli',
        'crates/ferry-extra',
        'Cargo.lock (bollard, itoa)',
        'Cargo.toml [workspace.dependencies]',
      ],
    )
  })

  test('more than three packages: the first three and a count', () => {
    const lock = ['a', 'b', 'c', 'd', 'e'].map((n) => `lock ${n} 1.0.0 ${REGISTRY}`)
    assert.deepEqual(cliChanges([], lock), ['Cargo.lock (a, b, c and 2 more)'])
  })
})

describe('npmPlan', () => {
  const never = () => assert.fail('the changes do not matter here')

  test('npm has no ferry-cli yet', () => {
    assert.deepEqual(npmPlan({ version: '0.5.0', latest: null, changes: never }), {
      publish: true,
      reason: 'npm has no ferry-cli yet',
    })
  })

  test('the CLI changed since the version npm has', () => {
    const changes = () => ['crates/ferry-cli', 'Cargo.lock (clap)']
    assert.deepEqual(npmPlan({ version: '0.6.0', latest: '0.4.1', changes }), {
      publish: true,
      reason: 'the CLI changed since ferry-cli 0.4.1: crates/ferry-cli, Cargo.lock (clap)',
    })
  })

  test('a release of the server only', () => {
    assert.deepEqual(npmPlan({ version: '0.6.0', latest: '0.4.1', changes: () => [] }), {
      publish: false,
      reason: 'the CLI has not changed since ferry-cli 0.4.1',
    })
  })

  test('no tag of the version npm has: publish', () => {
    assert.deepEqual(npmPlan({ version: '0.6.0', latest: '0.4.1', changes: () => null }), {
      publish: true,
      reason: 'no tag v0.4.1: what changed since ferry-cli 0.4.1 is unknown',
    })
  })

  test('npm has this version, or a newer one: a run again of an old release', () => {
    assert.deepEqual(npmPlan({ version: '0.6.0', latest: '0.6.0', changes: never }), {
      publish: false,
      reason: 'npm has ferry-cli 0.6.0 already',
    })
    assert.deepEqual(npmPlan({ version: '0.6.0', latest: '0.10.0', changes: never }), {
      publish: false,
      reason: 'npm has ferry-cli 0.10.0, newer than 0.6.0',
    })
  })
})

test('cliInputs of this repository: the CLI and ferry-core, not the server', () => {
  const lines = cliInputs(gitRepo('HEAD'))
  const folder = (line) => line.split(' ')[1].split('/').slice(0, 2).join('/')
  const folders = new Set(lines.filter((l) => l.startsWith('file ')).map(folder))
  assert.deepEqual([...folders].sort(), ['crates/ferry-cli', 'crates/ferry-core', 'npm/ferry-cli'])
  const packages = new Set(lines.filter((l) => l.startsWith('lock ')).map((l) => l.split(' ')[1]))
  for (const name of ['clap', 'reqwest', 'serde']) assert.ok(packages.has(name), `${name} builds ferry`)
  // bollard: the server's Docker client; axum: a dev-dependency of the CLI.
  for (const name of ['bollard', 'axum']) assert.ok(!packages.has(name), `${name} does not build ferry`)
})

describe('plan, on a repository with a fake npm', () => {
  const dir = mkdtempSync(join(tmpdir(), 'ferry-npm-plan-'))
  after(() => rmSync(dir, { recursive: true, force: true }))
  // `npm view ferry-cli dist-tags.latest` prints NPM_LATEST, or fails with
  // E404 when it is empty; with NPM_DOWN, npm fails as when the registry is
  // down; with NPM_EMPTY, it prints nothing.
  const fake = join(dir, 'fake-npm')
  mkdirSync(fake)
  writeFileSync(
    join(fake, 'npm'),
    `#!/bin/sh
[ "$*" = "view ferry-cli dist-tags.latest" ] || { echo "unexpected: npm $*" >&2; exit 9; }
[ -n "$NPM_DOWN" ] && { echo "npm error code ETIMEDOUT" >&2; exit 1; }
[ -n "$NPM_EMPTY" ] && exit 0
[ -z "$NPM_LATEST" ] && { echo "npm error code E404" >&2; exit 1; }
echo "$NPM_LATEST"
`,
  )
  chmodSync(join(fake, 'npm'), 0o755)
  const repo = join(dir, 'repo')
  mkdirSync(repo)
  const env = {
    ...process.env,
    PATH: `${fake}:${process.env.PATH}`,
    GIT_CONFIG_GLOBAL: '/dev/null',
    GIT_CONFIG_NOSYSTEM: '1',
    GIT_AUTHOR_NAME: 'Test',
    GIT_AUTHOR_EMAIL: 'test@example.com',
    GIT_COMMITTER_NAME: 'Test',
    GIT_COMMITTER_EMAIL: 'test@example.com',
  }
  const git = (...args) => execFileSync('git', args, { cwd: repo, env, encoding: 'utf8' })
  const write = (files) => {
    for (const [path, text] of Object.entries(files)) {
      mkdirSync(join(repo, path, '..'), { recursive: true })
      writeFileSync(join(repo, path), text)
    }
    git('add', '-A')
    git('commit', '-q', '-m', 'change')
  }
  const script = join(ROOT, '.github/scripts/npm.mjs')
  const plan = (version, npmEnv = {}) => {
    const options = { cwd: repo, env: { ...env, ...npmEnv }, encoding: 'utf8', stdio: 'pipe' }
    const out = execFileSync('node', [script, 'plan', version], options)
    return Object.fromEntries(out.trim().split('\n').map((line) => line.split(/=(.*)/s).slice(0, 2)))
  }

  git('init', '-q', '-b', 'main')
  write(workspace())
  git('tag', 'v0.1.0')

  test('npm has nothing yet', () => {
    assert.deepEqual(plan('0.1.0'), { publish: 'true', latest: '', reason: 'npm has no ferry-cli yet' })
  })

  test('a release of the server only, then one of the CLI', () => {
    write({ 'crates/ferryd/src/main.rs': 'fn main() { serve() }\n' })
    assert.deepEqual(plan('0.2.0', { NPM_LATEST: '0.1.0' }), {
      publish: 'false',
      latest: '0.1.0',
      reason: 'the CLI has not changed since ferry-cli 0.1.0',
    })
    write({ 'crates/ferry-core/src/lib.rs': 'pub struct Service;\npub struct Deploy;\n' })
    assert.deepEqual(plan('0.3.0', { NPM_LATEST: '0.1.0' }), {
      publish: 'true',
      latest: '0.1.0',
      reason: 'the CLI changed since ferry-cli 0.1.0: crates/ferry-core',
    })
  })

  test('no tag of the version npm has', () => {
    assert.equal(plan('0.3.0', { NPM_LATEST: '0.0.9' }).publish, 'true')
  })

  test('npm cannot answer: the plan fails, it does not guess', () => {
    assert.throws(
      () => plan('0.3.0', { NPM_DOWN: '1' }),
      (e) => /npm view ferry-cli: npm error code ETIMEDOUT/.test(e.stderr),
    )
    assert.throws(
      () => plan('0.3.0', { NPM_EMPTY: '1' }),
      (e) => /npm view ferry-cli: no version on the latest tag/.test(e.stderr),
    )
  })
})
