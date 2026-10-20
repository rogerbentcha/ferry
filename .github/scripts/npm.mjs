#!/usr/bin/env node
// The npm packages of the ferry CLI (.github/workflows/release.yml), made
// from the release archives. Node only, no dependencies.
//
//   node .github/scripts/npm.mjs stage <assets> <out>
//     Writes one folder per package into <out>: one per system of PLATFORMS
//     (npm/ferry-cli/lib/platform.js), with the `ferry` of its archive in
//     <assets> and the `os` / `cpu` npm installs it on, then ferry-cli, the
//     `ferry` launcher that depends on them. Every package has the version of
//     npm/ferry-cli/package.json: run `release.mjs set-version` first.
//     Prints the folders in publish order.
//
//   node .github/scripts/npm.mjs publish <out>
//     Publishes the folders of `stage`, with provenance: the binary packages
//     first, ferry-cli last, so ferry-cli never names a missing package. A
//     package already on npm at this version is skipped: a failed publish
//     can run again.
//
//   node .github/scripts/npm.mjs plan <X.Y.Z>
//     Says whether npm gets ferry-cli <X.Y.Z>, and prints `publish`, `latest`
//     (the version npm has) and `reason` as `key=value` lines (for
//     $GITHUB_OUTPUT). Yes when npm has no ferry-cli yet, or when the CLI
//     changed since the release of the version npm has (its `vX.Y.Z` tag): a
//     file of the crates `ferry` is built from, a package of Cargo.lock they
//     use, an entry of the root Cargo.toml they use, or a file of the
//     launcher. Their tests, their dev-dependencies and the other crates do
//     not count (cliInputs). No when npm has this version or a newer one.

import { execFileSync } from 'node:child_process'
import { chmodSync, copyFileSync, cpSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join, posix } from 'node:path'
import { pathToFileURL } from 'node:url'

import { PLATFORMS } from '../../npm/ferry-cli/lib/platform.js'
import { parseVersion } from './release.mjs'

const MAIN = 'ferry-cli'
const MAIN_DIR = 'npm/ferry-cli'
// The crate of the `ferry` binary. The crates it depends on are found from it.
const CLI_CRATE = 'crates/ferry-cli'
// Folders of a crate or of the launcher that never go into what npm installs.
const NOT_SHIPPED = ['tests', 'benches', 'examples', 'test']
// Entries of the root [workspace] table that do not change how a crate builds.
const WORKSPACE_LAYOUT = ['members', 'default-members', 'exclude']

/** The package.json of the binary package of `platform`, from the manifest of ferry-cli. */
export function platformManifest(platform, main) {
  return {
    name: platform.package,
    version: main.version,
    description: `The ferry binary for ${platform.label}. Install ferry-cli, not this package: ` +
      'npm install -g ferry-cli',
    homepage: main.homepage,
    bugs: main.bugs,
    repository: { type: main.repository.type, url: main.repository.url },
    license: main.license,
    os: [platform.os],
    cpu: [platform.cpu],
    ...(platform.os === 'linux' ? { libc: ['glibc'] } : {}),
    files: ['bin/ferry'],
  }
}

/** The README of the binary package of `platform`: the npm page points to ferry-cli. */
export function platformReadme(platform) {
  return `# ${platform.package}

The \`ferry\` binary of [Ferry](https://github.com/rogerbentcha/ferry) for ${platform.label}.

Do not install this package yourself. Install \`ferry-cli\`: npm picks the package of your system.

\`\`\`bash
npm install -g ferry-cli
\`\`\`
`
}

/** Throws when ferry-cli does not depend on exactly the packages of PLATFORMS, at its own version. */
export function checkMain(main) {
  const deps = main.optionalDependencies ?? {}
  const expected = PLATFORMS.map((p) => p.package).sort()
  const actual = Object.keys(deps).sort()
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(`${MAIN} optionalDependencies are ${actual.join(', ') || 'none'}, not ${expected.join(', ')}`)
  }
  for (const [name, version] of Object.entries(deps)) {
    if (version !== main.version) throw new Error(`${MAIN} ${main.version} depends on ${name} ${version}`)
  }
}

/** The package names in publish order: the binary packages, then ferry-cli. */
export function publishOrder() {
  return [...PLATFORMS.map((p) => p.package), MAIN]
}

function stage(assets, out) {
  const main = JSON.parse(readFileSync(join(MAIN_DIR, 'package.json'), 'utf8'))
  checkMain(main)
  for (const platform of PLATFORMS) {
    const archive = join(assets, `ferry-${platform.target}.tar.gz`)
    if (!existsSync(archive)) throw new Error(`${archive} is missing`)
    const dir = join(out, platform.package)
    mkdirSync(join(dir, 'bin'), { recursive: true })
    execFileSync('tar', ['-xzf', archive, '-C', join(dir, 'bin'), 'ferry'], { stdio: 'inherit' })
    chmodSync(join(dir, 'bin', 'ferry'), 0o755)
    writeFileSync(join(dir, 'package.json'), `${JSON.stringify(platformManifest(platform, main), null, 2)}\n`)
    writeFileSync(join(dir, 'README.md'), platformReadme(platform))
    copyFileSync('LICENSE', join(dir, 'LICENSE'))
  }
  const dir = join(out, MAIN)
  // The files of package.json `files` are what npm packs; the tests stay out.
  cpSync(MAIN_DIR, dir, { recursive: true, filter: (src) => !src.startsWith(join(MAIN_DIR, 'test')) })
  copyFileSync('LICENSE', join(dir, 'LICENSE'))
  for (const name of publishOrder()) console.log(join(out, name))
}

/** True when npm has `name` at `version`. */
function published(name, version) {
  try {
    const out = execFileSync('npm', ['view', `${name}@${version}`, 'version'], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
    })
    return out.trim() === version
  } catch {
    // E404: no package of that name yet.
    return false
  }
}

function publish(out) {
  for (const name of publishOrder()) {
    const dir = join(out, name)
    const { version } = JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8'))
    if (published(name, version)) {
      console.log(`${name}@${version} is on npm already: skipped`)
      continue
    }
    execFileSync('npm', ['publish', dir, '--access', 'public', '--provenance'], { stdio: 'inherit' })
    console.log(`Published ${name}@${version}`)
  }
}

/**
 * The tables of a TOML file as Cargo writes them, in file order: `{ table,
 * entries }`, each entry `{ key, value }` with the text of the value on one
 * line. A `[[x]]` table comes once per header. Enough for Cargo.toml and
 * Cargo.lock: not a full TOML parser.
 */
export function tomlTables(text) {
  const tables = [{ table: '', entries: [] }]
  let open = null // an entry whose array or inline table goes on to the next lines
  for (const raw of text.split('\n')) {
    const comment = syntax(raw).indexOf('#')
    const line = (comment < 0 ? raw : raw.slice(0, comment)).trim()
    if (open) {
      open.value += line ? ` ${line}` : ''
      if (depth(open.value) <= 0) open = null
      continue
    }
    if (!line) continue
    const header = /^\[\[?\s*(.+?)\s*\]\]?$/.exec(line)
    if (header) {
      tables.push({ table: header[1], entries: [] })
      continue
    }
    const m = /^([^=]+?)\s*=\s*(.+)$/.exec(line)
    if (!m) throw new Error(`cannot read the TOML line: ${raw.trim()}`)
    const entry = { key: unquote(m[1]), value: m[2] }
    tables.at(-1).entries.push(entry)
    if (depth(entry.value) > 0) open = entry
  }
  if (open) throw new Error(`the TOML value of ${open.key} does not end`)
  return tables
}

/** `text` with the inside of its strings blanked: a `#`, `[` or `{` left is TOML syntax. */
const syntax = (text) => text.replace(/"(?:[^"\\]|\\.)*"|'[^']*'/g, (s) => `${s[0]}${' '.repeat(s.length - 2)}${s[0]}`)

/** How many `[` and `{` of `value` are still open. */
const depth = (value) => [...syntax(value)].reduce((n, c) => n + '[{'.includes(c) - ']}'.includes(c), 0)

const unquote = (key) => key.trim().replace(/^"(.*)"$|^'(.*)'$/, '$1$2')

/** The text of a TOML string value, or null when `value` is not a string. */
function stringValue(value) {
  const m = /^"((?:[^"\\]|\\.)*)"$|^'([^']*)'$/.exec(value?.trim() ?? '')
  return m ? (m[1] ?? m[2]) : null
}

/** The strings of a TOML array value. */
const stringArray = (value) => [...value.matchAll(/"((?:[^"\\]|\\.)*)"/g)].map((m) => m[1])

/** The value of the field `name` of an inline table `{ … }`, as text, or null. */
function inlineField(value, name) {
  const m = new RegExp(`(?:^\\{|,)\\s*${name}\\s*=\\s*("[^"]*"|'[^']*'|true|false)`).exec(value)
  return m ? m[1] : null
}

/** The value of `key` in the table `table` of `tables`, as text, or null. */
function entry(tables, table, key) {
  return tables.find((t) => t.table === table)?.entries.find((e) => e.key === key)?.value ?? null
}

// [dependencies], [dev-dependencies], [build-dependencies], their
// [target.'cfg(…)'.…] forms, and [dependencies.<name>] for one dependency.
const DEPENDENCY_TABLE = /^(?:target\..+\.)?(dev-|build-)?dependencies(?:\.(.+))?$/

/**
 * The dependencies of a crate's Cargo.toml: `{ key, package, dev, workspace,
 * path }`. `package` is the name in Cargo.lock: the key, or the `package` of a
 * renamed dependency. `dev` is true for a dev-dependency, which the binary is
 * not built with. `path` is the folder of a path dependency, as written.
 */
export function manifestDependencies(cargoToml) {
  const deps = new Map()
  const dependency = (key, dev) => {
    const id = `${dev} ${key}`
    if (!deps.has(id)) deps.set(id, { key, package: key, dev, workspace: false, path: null })
    return deps.get(id)
  }
  const set = (dep, field, value) => {
    if (field === 'workspace') dep.workspace = value === 'true'
    else if (field === 'package') dep.package = stringValue(value) ?? dep.package
    else if (field === 'path') dep.path = stringValue(value)
  }
  for (const { table, entries } of tomlTables(cargoToml)) {
    const m = DEPENDENCY_TABLE.exec(table)
    if (!m) continue
    const dev = m[1] === 'dev-'
    if (m[2]) {
      const dep = dependency(unquote(m[2]), dev)
      for (const { key, value } of entries) set(dep, key, value)
      continue
    }
    for (const { key, value } of entries) {
      // `name = "1"`, `name = { … }` or a dotted key: `name.workspace = true`.
      const [name, field] = key.split(/\.(.*)/)
      const dep = dependency(name, dev)
      if (field) set(dep, field, value)
      else if (value.startsWith('{')) {
        for (const f of ['workspace', 'package', 'path']) {
          const v = inlineField(value, f)
          if (v !== null) set(dep, f, v)
        }
      }
    }
  }
  return [...deps.values()]
}

/** The entries of [workspace.dependencies]: key → `{ package, path }`. */
function workspaceDependencies(root) {
  const out = new Map()
  for (const { table, entries } of root) {
    const one = /^workspace\.dependencies\.(.+)$/.exec(table)
    if (one) {
      out.set(unquote(one[1]), {
        package: stringValue(entries.find((e) => e.key === 'package')?.value) ?? unquote(one[1]),
        path: stringValue(entries.find((e) => e.key === 'path')?.value),
      })
    } else if (table === 'workspace.dependencies') {
      for (const { key, value } of entries) {
        const inline = (f) => (value.startsWith('{') ? stringValue(inlineField(value, f)) : null)
        out.set(key, { package: inline('package') ?? key, path: inline('path') })
      }
    }
  }
  return out
}

/**
 * The packages of Cargo.lock that the crates are built with, one
 * `lock <name> <version> <source>` line each: every package their
 * dependencies reach, for every target. `crates` maps the name of each crate
 * to the packages it depends on, without its dev-dependencies: Cargo.lock
 * lists those too, for the crates of the workspace.
 */
function lockLines(cargoLock, crates) {
  const packages = tomlTables(cargoLock)
    .filter((t) => t.table === 'package')
    .map(({ entries }) => {
      const value = (key) => entries.find((e) => e.key === key)?.value
      return {
        name: stringValue(value('name')),
        version: stringValue(value('version')),
        source: stringValue(value('source')),
        dependencies: value('dependencies') ? stringArray(value('dependencies')) : [],
      }
    })
  // A dependency is `name`, `name version` or `name version (source)`: the
  // shortest text that names one package of the file.
  const find = (spec) => {
    const [, name, version, source] = /^(\S+)(?: (\S+))?(?: \((.+)\))?$/.exec(spec)
    const found = packages.filter(
      (p) => p.name === name && (!version || p.version === version) && (!source || p.source === source),
    )
    if (found.length !== 1) throw new Error(`Cargo.lock: ${found.length} packages match ${spec}`)
    return found[0]
  }
  const lines = new Set()
  const seen = new Set()
  const queue = [...crates.keys()].map(find)
  while (queue.length) {
    const p = queue.pop()
    const id = `${p.name} ${p.version} ${p.source ?? '(path)'}`
    if (seen.has(id)) continue
    seen.add(id)
    const crate = p.source === null ? crates.get(p.name) : undefined
    // A crate of the workspace: its files count, not its version (0.1.0 in the repository).
    if (!crate) lines.add(`lock ${id}`)
    for (const spec of p.dependencies) {
      if (!crate || crate.has(spec.split(' ')[0])) queue.push(find(spec))
    }
  }
  return [...lines]
}

/**
 * What `ferry` and its launcher are made of in `repo`, one line each, sorted,
 * so two commits compare line by line (cliChanges):
 * - `file <path> <blob>`: each file of the crate of `ferry`, of the crates of
 *   the workspace it depends on (path dependencies, found one from another),
 *   and of the launcher npm/ferry-cli, without their tests;
 * - `lock …`: each package of Cargo.lock these crates are built with;
 * - `cargo [table] key = value`: each entry of the root Cargo.toml, but the
 *   members of the workspace and the [workspace.dependencies] these crates do
 *   not use.
 * Dev-dependencies do not count: they build the tests, not the binary.
 * `repo` reads a commit: `read(path)` gives the text of a file or null,
 * `files(dir)` gives `[path, blob]` for each file under `dir`.
 */
export function cliInputs(repo) {
  const read = (path) => {
    const text = repo.read(path)
    if (text === null) throw new Error(`${path} is missing`)
    return text
  }
  const root = tomlTables(read('Cargo.toml'))
  const workspace = workspaceDependencies(root)
  const crates = new Map() // folder → { name, deps: the packages it is built with }
  const used = new Set() // the keys of [workspace.dependencies] the crates use
  const queue = [CLI_CRATE]
  while (queue.length) {
    const dir = queue.shift()
    if (crates.has(dir)) continue
    const manifest = read(`${dir}/Cargo.toml`)
    const name = stringValue(entry(tomlTables(manifest), 'package', 'name'))
    if (!name) throw new Error(`${dir}/Cargo.toml: no name in [package]`)
    const deps = new Set()
    for (const dep of manifestDependencies(manifest)) {
      if (dep.dev) continue
      const shared = dep.workspace ? workspace.get(dep.key) : null
      if (dep.workspace && !shared) throw new Error(`${dir}/Cargo.toml: ${dep.key} is not in [workspace.dependencies]`)
      if (dep.workspace) used.add(dep.key)
      deps.add(shared ? shared.package : dep.package)
      // A path dependency: of the crate's folder, or of the root for a workspace one.
      const path = dep.path ? posix.join(dir, dep.path) : shared?.path
      if (path) queue.push(posix.normalize(path).replace(/\/$/, ''))
    }
    crates.set(dir, { name, deps })
  }

  const lines = []
  for (const dir of [...crates.keys(), MAIN_DIR]) {
    for (const [path, blob] of repo.files(dir)) {
      const inside = path.slice(dir.length + 1)
      if (!NOT_SHIPPED.some((d) => inside.startsWith(`${d}/`))) lines.push(`file ${path} ${blob}`)
    }
  }
  const lock = read('Cargo.lock')
  lines.push(...lockLines(lock, new Map([...crates.values()].map((c) => [c.name, c.deps]))))
  for (const { table, entries } of root) {
    const shared = /^workspace\.dependencies(?:\.(.+))?$/.exec(table)
    for (const { key, value } of entries) {
      if (table === 'workspace' && WORKSPACE_LAYOUT.includes(key)) continue
      if (shared && !used.has(shared[1] ? unquote(shared[1]) : key)) continue
      lines.push(`cargo [${table}] ${key} = ${value}`)
    }
  }
  return [...new Set(lines)].sort()
}

/**
 * What changed between two cliInputs, in a few words: the folders, then the
 * packages of Cargo.lock, then the tables of Cargo.toml. Empty when nothing.
 */
export function cliChanges(before, now) {
  const a = new Set(before)
  const b = new Set(now)
  const changed = [...before.filter((line) => !b.has(line)), ...now.filter((line) => !a.has(line))]
  const folders = new Set()
  const packages = new Set()
  const tables = new Set()
  for (const line of changed) {
    const [kind, what] = line.split(' ')
    if (kind === 'file') folders.add(what.split('/').slice(0, 2).join('/'))
    else if (kind === 'lock') packages.add(what)
    else tables.add(`Cargo.toml ${what}`)
  }
  const out = [...folders].sort()
  if (packages.size) {
    const names = [...packages].sort()
    const more = names.length > 3 ? ` and ${names.length - 3} more` : ''
    out.push(`Cargo.lock (${names.slice(0, 3).join(', ')}${more})`)
  }
  return [...out, ...[...tables].sort()]
}

/**
 * Whether npm gets ferry-cli `version`: `{ publish, reason }`. `latest` is the
 * version npm has, null when it has none. `changes()` gives what changed in
 * the CLI since the release of `latest` (cliChanges), or null when that
 * release has no tag here: it runs only when the answer depends on it.
 */
export function npmPlan({ version, latest, changes }) {
  if (latest === null) return { publish: true, reason: `npm has no ${MAIN} yet` }
  const [a, b] = [parseVersion(latest), parseVersion(version)]
  if (!a) throw new Error(`npm has ${MAIN} ${latest}: not a version`)
  const order = a[0] - b[0] || a[1] - b[1] || a[2] - b[2]
  if (order === 0) return { publish: false, reason: `npm has ${MAIN} ${version} already` }
  if (order > 0) return { publish: false, reason: `npm has ${MAIN} ${latest}, newer than ${version}` }
  const changed = changes()
  if (changed === null) {
    return { publish: true, reason: `no tag v${latest}: what changed since ${MAIN} ${latest} is unknown` }
  }
  if (!changed.length) return { publish: false, reason: `the CLI has not changed since ${MAIN} ${latest}` }
  return { publish: true, reason: `the CLI changed since ${MAIN} ${latest}: ${changed.join(', ')}` }
}

function git(...args) {
  return execFileSync('git', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], maxBuffer: 64 * 1024 * 1024 })
}

/** The files of the commit `ref`, for cliInputs. Run from the root of the repository. */
export function gitRepo(ref) {
  return {
    read(path) {
      try {
        return git('show', `${ref}:${path}`)
      } catch {
        return null
      }
    },
    files(dir) {
      return git('ls-tree', '-r', '-z', ref, '--', `${dir}/`)
        .split('\0')
        .filter(Boolean)
        .map((line) => {
          const [meta, path] = line.split('\t')
          return [path, meta.split(' ')[2]]
        })
    },
  }
}

/** The version of the `latest` tag of `name` on npm, or null when npm has no such package. */
function latestOnNpm(name) {
  let out
  try {
    const options = { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }
    out = execFileSync('npm', ['view', name, 'dist-tags.latest'], options)
  } catch (e) {
    // E404: no package of that name, or one that was unpublished.
    if (/\bE404\b/.test(e.stderr ?? '')) return null
    throw new Error(`npm view ${name}: ${(e.stderr || e.message).trim()}`)
  }
  // npm keeps a `latest` tag on every package it has.
  if (!out.trim()) throw new Error(`npm view ${name}: no version on the latest tag`)
  return out.trim()
}

function plan(version) {
  if (!parseVersion(version) || version.startsWith('v')) throw new Error(`not a version: ${version}`)
  const latest = latestOnNpm(MAIN)
  const inputs = (ref) => {
    try {
      return cliInputs(gitRepo(ref))
    } catch (e) {
      throw new Error(`at ${ref}: ${e.message}`)
    }
  }
  const changes = () => {
    try {
      git('rev-parse', '--verify', '--quiet', `refs/tags/v${latest}^{commit}`)
    } catch {
      return null
    }
    return cliChanges(inputs(`v${latest}`), inputs('HEAD'))
  }
  const result = npmPlan({ version, latest, changes })
  console.log(`publish=${result.publish}`)
  console.log(`latest=${latest ?? ''}`)
  console.log(`reason=${result.reason}`)
}

function main([command, ...args]) {
  if (command === 'stage' && args.length === 2) return stage(...args)
  if (command === 'publish' && args.length === 1) return publish(...args)
  if (command === 'plan' && args.length === 1) return plan(...args)
  throw new Error('usage: npm.mjs stage <assets> <out> | publish <out> | plan <X.Y.Z>')
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    main(process.argv.slice(2))
  } catch (e) {
    console.error(`npm.mjs: ${e.message}`)
    process.exit(1)
  }
}
