#!/usr/bin/env node
// The version and the notes of a Ferry release (.github/workflows/release.yml).
// Node only, no dependencies.
//
//   node .github/scripts/release.mjs plan [--notes <file>] [--publish]
//     Reads the Conventional Commits since the last `vX.Y.Z` tag merged into
//     HEAD and prints `release`, `version`, `tag`, `previous` and `reason` as
//     `key=value` lines (for $GITHUB_OUTPUT). `fix` bumps the patch, `feat`
//     the minor, a breaking change the major (the minor while on 0.x); with
//     none of them, `release=false` and `reason` says why. Before the first
//     tag, the version of [workspace.package] in Cargo.toml is the last
//     release and every commit is new. Writes the notes to <file>.
//     With --publish (a push to main), stops when the new tag already exists
//     outside HEAD's history. Without it (a check from another branch, such
//     as dev, which never holds main's tagged merge commits), the version is
//     only what the build reports.
//
//   node .github/scripts/release.mjs set-version <X.Y.Z>
//     Writes the version into [workspace.package] of Cargo.toml, so the
//     binaries report it (`ferry --version`, ferryd.json, the OpenAPI info),
//     and into npm/ferry-cli/package.json, so npm gets the same version.
//     The release build does this in its own checkout: the repository keeps
//     its version, and no commit goes back to main.

import { execFileSync } from 'node:child_process'
import { readFileSync, writeFileSync } from 'node:fs'
import { pathToFileURL } from 'node:url'

const SEMVER = /^v?(\d+)\.(\d+)\.(\d+)$/
// `type(scope)!: description`; the scope and the `!` are optional.
const HEADER = /^([A-Za-z]+)(?:\(([^)]*)\))?(!)?: +(\S.*)$/
const BREAKING_FOOTER = /^BREAKING[ -]CHANGE: */
// A footer starts with a token: `Refs: LAB-1`, `Co-Authored-By: …`, `Fixes #12`.
const FOOTER_TOKEN = /^[A-Za-z-]+(: | #)/
// The types of the commit skill (.claude/skills/commit); others count as "other".
const TYPES = ['feat', 'fix', 'docs', 'refactor', 'perf', 'test', 'build', 'ci', 'style', 'chore', 'revert']
// The npm package of the CLI (.github/scripts/npm.mjs).
const NPM_PACKAGE = 'npm/ferry-cli/package.json'

/** `[major, minor, patch]` of `X.Y.Z` or `vX.Y.Z`, or null. */
export function parseVersion(text) {
  const m = SEMVER.exec(text.trim())
  return m ? [Number(m[1]), Number(m[2]), Number(m[3])] : null
}

/** A commit's Conventional Commits header and breaking footer, or null when it has no such header. */
export function parseCommit({ sha, subject, body = '' }) {
  const m = HEADER.exec(subject.trim())
  if (!m) return null
  const note = breakingFooter(body)
  return {
    sha,
    type: m[1].toLowerCase(),
    scope: m[2]?.trim() || null,
    breaking: Boolean(m[3]) || note !== null,
    description: m[4].trim(),
    breakingNote: note || null,
  }
}

/** The text of a `BREAKING CHANGE:` footer, on one line ('' when empty), or null without one. */
function breakingFooter(body) {
  const lines = body.split('\n')
  const start = lines.findIndex((line) => BREAKING_FOOTER.test(line))
  if (start < 0) return null
  const text = [lines[start].replace(BREAKING_FOOTER, '')]
  for (const line of lines.slice(start + 1)) {
    if (!line.trim() || FOOTER_TOKEN.test(line)) break
    text.push(line.trim())
  }
  return text.join(' ').trim()
}

/** The bump the parsed commits call for: 'major', 'minor', 'patch', or null for none. */
export function bumpLevel(commits) {
  if (commits.some((c) => c.breaking)) return 'major'
  if (commits.some((c) => c.type === 'feat')) return 'minor'
  if (commits.some((c) => c.type === 'fix')) return 'patch'
  return null
}

/**
 * The version after `current` (`[major, minor, patch]`) for a bump level.
 * While on 0.x, a breaking change bumps the minor.
 */
export function bump([major, minor, patch], level) {
  if (level === 'major' && major > 0) return [major + 1, 0, 0]
  if (level === 'major' || level === 'minor') return [major, minor + 1, 0]
  if (level === 'patch') return [major, minor, patch + 1]
  throw new Error(`unknown bump level: ${level}`)
}

/** The highest `vX.Y.Z` tag of a list, or null. */
export function lastReleaseTag(tags) {
  let best = null
  for (const tag of tags) {
    const v = tag.startsWith('v') ? parseVersion(tag) : null
    if (v && (!best || compare(v, best.v) > 0)) best = { tag, v }
  }
  return best?.tag ?? null
}

function compare(a, b) {
  return a[0] - b[0] || a[1] - b[1] || a[2] - b[2]
}

/** The `version` of the `[workspace.package]` table of a Cargo.toml. */
export function workspaceVersion(cargoToml) {
  const line = workspaceVersionLine(cargoToml)
  if (!line) throw new Error('Cargo.toml: no version in [workspace.package]')
  return line.value
}

/** Cargo.toml with `version` of `[workspace.package]` set to `version`. Nothing else changes. */
export function setWorkspaceVersion(cargoToml, version) {
  if (!parseVersion(version) || version.startsWith('v')) throw new Error(`not a version: ${version}`)
  const line = workspaceVersionLine(cargoToml)
  if (!line) throw new Error('Cargo.toml: no version in [workspace.package]')
  const lines = cargoToml.split('\n')
  lines[line.index] = lines[line.index].replace(`"${line.value}"`, `"${version}"`)
  return lines.join('\n')
}

/**
 * The package.json of the npm package ferry-cli with `version` set, and each
 * of its `optionalDependencies` (the binary of each system) pinned to it.
 */
export function setNpmVersion(packageJson, version) {
  if (!parseVersion(version) || version.startsWith('v')) throw new Error(`not a version: ${version}`)
  const manifest = JSON.parse(packageJson)
  manifest.version = version
  for (const name of Object.keys(manifest.optionalDependencies ?? {})) manifest.optionalDependencies[name] = version
  return `${JSON.stringify(manifest, null, 2)}\n`
}

function workspaceVersionLine(cargoToml) {
  let inTable = false
  const lines = cargoToml.split('\n')
  for (const [index, text] of lines.entries()) {
    const table = /^\s*\[([^\]]+)\]\s*(#.*)?$/.exec(text)
    if (table) {
      inTable = table[1].trim() === 'workspace.package'
      continue
    }
    const m = inTable && /^\s*version\s*=\s*"([^"]*)"/.exec(text)
    if (m) return { index, value: m[1] }
  }
  return null
}

const GROUPS = [
  ['Breaking changes', (c) => c.breaking],
  ['Features', (c) => !c.breaking && c.type === 'feat'],
  ['Fixes', (c) => !c.breaking && c.type === 'fix'],
]

/**
 * The notes of a release: its breaking changes, features and fixes (oldest
 * first, each one once), then the link to every change since `previous`.
 */
export function releaseNotes({ tag, previous, commits, repoUrl }) {
  const ordered = [...commits].reverse()
  const out = []
  for (const [title, keep] of GROUPS) {
    const items = ordered.filter(keep)
    if (!items.length) continue
    out.push(`## ${title}`, '')
    for (const c of items) {
      const scope = c.scope ? `**${c.scope}:** ` : ''
      out.push(`- ${scope}${c.description} ([${c.sha.slice(0, 7)}](${repoUrl}/commit/${c.sha}))`)
      if (c.breakingNote) out.push(`  ${c.breakingNote}`)
    }
    out.push('')
  }
  const changes = previous ? `${repoUrl}/compare/${previous}...${tag}` : `${repoUrl}/commits/${tag}`
  out.push(`**Full changelog:** ${changes}`, '')
  return out.join('\n')
}

/** The release that HEAD calls for, from the commits (newest first) since the last release. */
export function plan({ previousTag, workspace, commits, repoUrl }) {
  const since = previousTag ?? `${workspace} (no release tag yet)`
  const current = previousTag ? parseVersion(previousTag) : parseVersion(workspace)
  if (!current) throw new Error(`not a version: ${previousTag ?? workspace}`)
  const parsed = commits.map(parseCommit).filter(Boolean)
  const level = bumpLevel(parsed)
  if (!level) {
    const reason = commits.length
      ? `no feat, fix or breaking commit since ${since} (${describe(commits.length, parsed)})`
      : `no commit since ${since}`
    return { release: false, version: current.join('.'), previous: previousTag, reason }
  }
  const version = bump(current, level).join('.')
  const tag = `v${version}`
  return {
    release: true,
    version,
    tag,
    previous: previousTag,
    reason: `${level} bump: ${describe(commits.length, parsed)} since ${since}`,
    notes: releaseNotes({ tag, previous: previousTag, commits: parsed, repoUrl }),
  }
}

function describe(total, parsed) {
  const counts = new Map()
  let other = total - parsed.length
  for (const c of parsed) {
    const key = c.breaking ? 'breaking' : c.type
    if (key !== 'breaking' && !TYPES.includes(key)) other += 1
    else counts.set(key, (counts.get(key) ?? 0) + 1)
  }
  if (other) counts.set('other', other)
  return [...counts].map(([k, n]) => `${n} ${k}`).join(', ')
}

function git(...args) {
  return execFileSync('git', args, { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 })
}

/** The commits of `range` without merge commits, newest first. */
function readCommits(range) {
  // Fields end with US (0x1f), commits with RS (0x1e): neither shows up in a message.
  const log = git('log', '--no-merges', '--format=%H%x1f%s%x1f%b%x1e', range)
  return log
    .split('\x1e')
    .map((entry) => entry.replace(/^\n/, ''))
    .filter((entry) => entry.trim())
    .map((entry) => {
      const [sha, subject, body] = entry.split('\x1f')
      return { sha, subject, body }
    })
}

function repoUrl() {
  const server = process.env.GITHUB_SERVER_URL ?? 'https://github.com'
  const repo = process.env.GITHUB_REPOSITORY ?? 'rogerbentcha/ferry'
  return `${server}/${repo}`
}

function main([command, ...args]) {
  if (command === 'plan') {
    const notesAt = args.indexOf('--notes')
    const notesFile = notesAt >= 0 ? args[notesAt + 1] : null
    if (notesAt >= 0 && !notesFile) throw new Error('--notes needs a file')
    const publish = args.includes('--publish')
    const previousTag = lastReleaseTag(git('tag', '--list', 'v*', '--merged', 'HEAD').split('\n').filter(Boolean))
    const result = plan({
      previousTag,
      workspace: workspaceVersion(readFileSync('Cargo.toml', 'utf8')),
      commits: readCommits(previousTag ? `${previousTag}..HEAD` : 'HEAD'),
      repoUrl: repoUrl(),
    })
    // A tag outside HEAD's history: publishing would put this release on another commit.
    if (publish && result.release && git('tag', '--list', result.tag).trim()) {
      throw new Error(`tag ${result.tag} already exists on a commit outside the history of HEAD: no release published`)
    }
    if (notesFile) writeFileSync(notesFile, result.notes ?? '')
    console.log(`release=${result.release}`)
    console.log(`version=${result.version}`)
    console.log(`tag=${result.tag ?? ''}`)
    console.log(`previous=${result.previous ?? ''}`)
    console.log(`reason=${result.reason}`)
    return
  }
  if (command === 'set-version') {
    const [version] = args
    if (!version) throw new Error('set-version needs a version')
    writeFileSync('Cargo.toml', setWorkspaceVersion(readFileSync('Cargo.toml', 'utf8'), version))
    writeFileSync(NPM_PACKAGE, setNpmVersion(readFileSync(NPM_PACKAGE, 'utf8'), version))
    return
  }
  throw new Error('usage: release.mjs plan [--notes <file>] [--publish] | set-version <X.Y.Z>')
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    main(process.argv.slice(2))
  } catch (e) {
    console.error(`release.mjs: ${e.message}`)
    process.exit(1)
  }
}
