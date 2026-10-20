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

import { execFileSync } from 'node:child_process'
import { chmodSync, copyFileSync, cpSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'

import { PLATFORMS } from '../../npm/ferry-cli/lib/platform.js'

const MAIN = 'ferry-cli'
const MAIN_DIR = 'npm/ferry-cli'

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

function main([command, ...args]) {
  if (command === 'stage' && args.length === 2) return stage(...args)
  if (command === 'publish' && args.length === 1) return publish(...args)
  throw new Error('usage: npm.mjs stage <assets> <out> | publish <out>')
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    main(process.argv.slice(2))
  } catch (e) {
    console.error(`npm.mjs: ${e.message}`)
    process.exit(1)
  }
}
