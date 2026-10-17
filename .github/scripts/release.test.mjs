// Tests of release.mjs: `node --test .github/scripts/release.test.mjs`.

import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { after, describe, test } from 'node:test'
import { fileURLToPath } from 'node:url'

import {
  bump,
  bumpLevel,
  lastReleaseTag,
  parseCommit,
  parseVersion,
  plan,
  releaseNotes,
  setWorkspaceVersion,
  workspaceVersion,
} from './release.mjs'

const REPO = 'https://github.com/rogerbentcha/ferry'
let n = 0
const commit = (subject, body = '') => ({ sha: `${String(++n).padStart(4, '0')}${'a'.repeat(36)}`, subject, body })

describe('parseCommit', () => {
  test('type, scope and description', () => {
    const c = parseCommit(commit('feat(api): serve Prometheus metrics at /metrics'))
    assert.equal(c.type, 'feat')
    assert.equal(c.scope, 'api')
    assert.equal(c.breaking, false)
    assert.equal(c.description, 'serve Prometheus metrics at /metrics')
  })

  test('no scope, any case', () => {
    const c = parseCommit(commit('Fix: keep the charts up'))
    assert.equal(c.type, 'fix')
    assert.equal(c.scope, null)
  })

  test('breaking with `!`', () => {
    const c = parseCommit(commit("feat(api)!: serve ferryd's log at /server/logs"))
    assert.equal(c.breaking, true)
    assert.equal(c.breakingNote, null)
  })

  test('breaking with a footer', () => {
    const body = 'Why.\n\nBREAKING CHANGE: --dir is now --path\nRefs: LAB-1'
    const c = parseCommit(commit('refactor(cli): rename a flag', body))
    assert.equal(c.breaking, true)
    assert.equal(c.breakingNote, '--dir is now --path')
    assert.equal(parseCommit(commit('fix: x', 'BREAKING-CHANGE: y')).breaking, true)
  })

  test('a footer that goes on over more lines, up to the next footer', () => {
    const body = [
      'Why.',
      '',
      'BREAKING CHANGE: `AppState` gains `server_logs`; struct',
      'literals must set it.',
      'Refs: LAB-15',
      'Co-Authored-By: A <a@b>',
    ].join('\n')
    const c = parseCommit(commit('feat(api)!: x', body))
    assert.equal(c.breakingNote, '`AppState` gains `server_logs`; struct literals must set it.')
  })

  test('a footer must be in capitals', () => {
    assert.equal(parseCommit(commit('fix: x', 'breaking change: y')).breaking, false)
  })

  test('not Conventional Commits', () => {
    for (const subject of [
      'Merge pull request #27 from rogerbentcha/dev',
      'Resource limits: per-service memory',
      'ferry-docker, ferry-cli: log rotation',
      'feat:no space',
      'feat(api):',
    ]) {
      assert.equal(parseCommit(commit(subject)), null, subject)
    }
  })
})

describe('versions', () => {
  test('parseVersion', () => {
    assert.deepEqual(parseVersion('0.1.0'), [0, 1, 0])
    assert.deepEqual(parseVersion('v12.3.45'), [12, 3, 45])
    assert.equal(parseVersion('v1.0.0-rc.1'), null)
    assert.equal(parseVersion('1.0'), null)
  })

  test('bumpLevel: breaking > feat > fix > nothing', () => {
    const p = (...subjects) => subjects.map((s) => parseCommit(commit(s)))
    assert.equal(bumpLevel(p('fix: a', 'feat!: b', 'feat: c')), 'major')
    assert.equal(bumpLevel(p('fix: a', 'feat: c', 'docs: d')), 'minor')
    assert.equal(bumpLevel(p('fix: a', 'docs: d')), 'patch')
    assert.equal(bumpLevel(p('docs: a', 'chore: b', 'ci: c', 'refactor: d', 'perf: e')), null)
    assert.equal(bumpLevel([]), null)
  })

  test('bump', () => {
    assert.deepEqual(bump([0, 2, 0], 'patch'), [0, 2, 1])
    assert.deepEqual(bump([0, 2, 1], 'minor'), [0, 3, 0])
    assert.deepEqual(bump([0, 2, 1], 'major'), [0, 3, 0], 'a breaking change bumps the minor on 0.x')
    assert.deepEqual(bump([1, 4, 2], 'major'), [2, 0, 0])
    assert.deepEqual(bump([1, 4, 2], 'minor'), [1, 5, 0])
  })

  test('lastReleaseTag: the highest vX.Y.Z', () => {
    assert.equal(lastReleaseTag(['v0.9.0', 'v0.10.0', 'v0.2.3', 'v1.0.0-rc.1', 'nightly', '0.11.0']), 'v0.10.0')
    assert.equal(lastReleaseTag([]), null)
  })
})

describe('Cargo.toml', () => {
  const toml = `[workspace]
members = ["crates/a"]

[workspace.package]
version = "0.1.0"
edition = "2024"

[workspace.dependencies]
tokio = { version = "1", features = ["full"] }
serde = "1"

[profile.release]
strip = true
`

  test('workspaceVersion', () => {
    assert.equal(workspaceVersion(toml), '0.1.0')
    assert.throws(() => workspaceVersion('[package]\nversion = "1.0.0"\n'), /no version in \[workspace.package\]/)
  })

  test('setWorkspaceVersion changes that line only', () => {
    const updated = setWorkspaceVersion(toml, '0.2.0')
    assert.equal(updated, toml.replace('version = "0.1.0"', 'version = "0.2.0"'))
    assert.equal(workspaceVersion(updated), '0.2.0')
  })

  test('setWorkspaceVersion takes X.Y.Z only', () => {
    assert.throws(() => setWorkspaceVersion(toml, 'v0.2.0'), /not a version/)
    assert.throws(() => setWorkspaceVersion(toml, '0.2'), /not a version/)
  })
})

describe('releaseNotes', () => {
  test('grouped, oldest first, each change once', () => {
    // Newest first, as git log lists them.
    const commits = [
      commit('fix(web): keep Server → Monitoring up'),
      commit('docs: describe the logs'),
      commit('feat(api): serve Prometheus metrics'),
      commit('feat(api)!: serve the log at /server/logs', 'BREAKING CHANGE: /logs is gone'),
      commit('feat: add Server logs'),
    ]
      .map(parseCommit)
      .filter(Boolean)
    const notes = releaseNotes({ tag: 'v0.3.0', previous: 'v0.2.1', commits, repoUrl: REPO })
    const [fix, , feat, breaking, first] = commits.map((c) => c.sha)
    assert.equal(
      notes,
      [
        '## Breaking changes',
        '',
        `- **api:** serve the log at /server/logs ([${breaking.slice(0, 7)}](${REPO}/commit/${breaking}))`,
        '  /logs is gone',
        '',
        '## Features',
        '',
        `- add Server logs ([${first.slice(0, 7)}](${REPO}/commit/${first}))`,
        `- **api:** serve Prometheus metrics ([${feat.slice(0, 7)}](${REPO}/commit/${feat}))`,
        '',
        '## Fixes',
        '',
        `- **web:** keep Server → Monitoring up ([${fix.slice(0, 7)}](${REPO}/commit/${fix}))`,
        '',
        `**Full changelog:** ${REPO}/compare/v0.2.1...v0.3.0`,
        '',
      ].join('\n'),
    )
  })

  test('the first release links to its commits', () => {
    const commits = [parseCommit(commit('fix: a'))]
    const notes = releaseNotes({ tag: 'v0.2.0', previous: null, commits, repoUrl: REPO })
    assert.match(notes, /\*\*Full changelog:\*\* https:\/\/github.com\/rogerbentcha\/ferry\/commits\/v0.2.0\n$/)
  })
})

describe('plan', () => {
  const run = (previousTag, subjects) =>
    plan({ previousTag, workspace: '0.1.0', commits: subjects.map((s) => commit(s)), repoUrl: REPO })

  test('a feature since v0.1.0 ships v0.2.0', () => {
    const p = run('v0.1.0', ['docs: a', 'feat(web): b', 'fix: c'])
    assert.equal(p.release, true)
    assert.equal(p.version, '0.2.0')
    assert.equal(p.tag, 'v0.2.0')
    assert.equal(p.previous, 'v0.1.0')
    assert.equal(p.reason, 'minor bump: 1 docs, 1 feat, 1 fix since v0.1.0')
    assert.match(p.notes, /## Features/)
  })

  test('only fixes since v0.2.0 ship v0.2.1', () => {
    const p = run('v0.2.0', ['fix: a', 'Merge branch dev'])
    assert.equal(p.version, '0.2.1')
    assert.equal(p.reason, 'patch bump: 1 fix, 1 other since v0.2.0')
  })

  test('only docs and chores: no release, and why', () => {
    const p = run('v0.2.1', ['docs: a', 'chore: b', 'docs: c', 'Web: d', 'Merge e'])
    assert.equal(p.release, false)
    assert.equal(p.version, '0.2.1')
    assert.equal(p.reason, 'no feat, fix or breaking commit since v0.2.1 (2 docs, 1 chore, 2 other)')
    assert.equal(p.notes, undefined)
  })

  test('nothing since the last release', () => {
    const p = run('v0.2.1', [])
    assert.equal(p.release, false)
    assert.equal(p.reason, 'no commit since v0.2.1')
  })

  test('before the first tag, the workspace version is the last release', () => {
    const p = run(null, ['feat(api)!: a', 'chore: b'])
    assert.equal(p.version, '0.2.0')
    assert.equal(p.previous, null)
    assert.equal(p.reason, 'major bump: 1 breaking, 1 chore since 0.1.0 (no release tag yet)')
  })
})

describe('the command, on a real repository', () => {
  const script = fileURLToPath(new URL('./release.mjs', import.meta.url))
  const dir = mkdtempSync(join(tmpdir(), 'ferry-release-'))
  after(() => rmSync(dir, { recursive: true, force: true }))
  const env = {
    ...process.env,
    GIT_CONFIG_GLOBAL: '/dev/null',
    GIT_CONFIG_NOSYSTEM: '1',
    GIT_AUTHOR_NAME: 'Test',
    GIT_AUTHOR_EMAIL: 'test@example.com',
    GIT_COMMITTER_NAME: 'Test',
    GIT_COMMITTER_EMAIL: 'test@example.com',
    GITHUB_SERVER_URL: 'https://github.com',
    GITHUB_REPOSITORY: 'rogerbentcha/ferry',
  }
  const git = (...args) => execFileSync('git', args, { cwd: dir, env, encoding: 'utf8' })
  const commitAll = (message) => git('commit', '--allow-empty', '-q', '-m', message)
  const release = (...args) => execFileSync('node', [script, ...args], { cwd: dir, env, encoding: 'utf8' })
  // The `key=value` lines of `plan`, as an object.
  const planned = (...flags) => {
    const lines = release('plan', '--notes', join(dir, 'notes.md'), ...flags).trim().split('\n')
    return Object.fromEntries(lines.map((line) => line.split(/=(.*)/s).slice(0, 2)))
  }

  git('init', '-q', '-b', 'main')
  writeFileSync(join(dir, 'Cargo.toml'), '[workspace.package]\nversion = "0.1.0"\n')
  git('add', 'Cargo.toml')
  commitAll('chore: start')

  test('before the first tag', () => {
    commitAll('feat(api): first feature\n\nThe body.\n\nRefs: LAB-1')
    commitAll('docs: explain it')
    const p = planned()
    assert.deepEqual(p, {
      release: 'true',
      version: '0.2.0',
      tag: 'v0.2.0',
      previous: '',
      reason: 'minor bump: 1 docs, 1 feat, 1 chore since 0.1.0 (no release tag yet)',
    })
    const notes = readFileSync(join(dir, 'notes.md'), 'utf8')
    const link = /\(\[[0-9a-f]{7}\]\(https:\/\/github.com\/rogerbentcha\/ferry\/commit\/[0-9a-f]{40}\)\)/.source
    assert.match(notes, new RegExp(`^## Features\\n\\n- \\*\\*api:\\*\\* first feature ${link}\\n`))
    assert.match(notes, /commits\/v0.2.0\n$/)
  })

  test('after a tag, from that tag; merges do not count', () => {
    git('tag', 'v0.2.0')
    git('switch', '-q', '-c', 'dev')
    commitAll('fix(web): a fix')
    git('switch', '-q', 'main')
    git('merge', '-q', '--no-ff', '-m', 'Merge pull request #2 from rogerbentcha/dev', 'dev')
    const p = planned()
    assert.equal(p.version, '0.2.1')
    assert.equal(p.previous, 'v0.2.0')
    assert.equal(p.reason, 'patch bump: 1 fix since v0.2.0')
    assert.match(readFileSync(join(dir, 'notes.md'), 'utf8'), /compare\/v0.2.0...v0.2.1\n$/)
  })

  test('no release for docs and chores, and an empty notes file', () => {
    git('tag', 'v0.2.1')
    commitAll('docs: more')
    commitAll('chore: tidy')
    const p = planned()
    assert.equal(p.release, 'false')
    assert.equal(p.version, '0.2.1')
    assert.equal(p.tag, '')
    assert.equal(p.reason, 'no feat, fix or breaking commit since v0.2.1 (1 chore, 1 docs)')
    assert.equal(readFileSync(join(dir, 'notes.md'), 'utf8'), '')
  })

  test('a tag that is not merged into HEAD does not count', () => {
    git('switch', '-q', '-c', 'other')
    commitAll('feat: elsewhere')
    git('tag', 'v9.0.0')
    git('switch', '-q', 'main')
    assert.equal(planned().previous, 'v0.2.1')
  })

  test('the next tag already exists outside HEAD: a publish stops, a check goes on', () => {
    git('switch', '-q', 'other')
    git('tag', 'v0.3.0')
    git('switch', '-q', 'main')
    commitAll('feat: on main')
    assert.throws(() => planned('--publish'), /tag v0.3.0 already exists on a commit outside the history of HEAD/)
    assert.equal(planned().version, '0.3.0')
  })

  test('dev never holds the tags of main: a check from dev still plans a version', () => {
    // main's releases are tagged on its merge commits, which dev does not have.
    git('switch', '-q', '-c', 'dev-after', 'v0.2.0~1')
    commitAll('fix: on dev')
    const p = planned()
    assert.equal(p.release, 'true')
    assert.equal(p.previous, '')
    git('switch', '-q', 'main')
  })

  test('set-version', () => {
    release('set-version', '0.3.0')
    assert.equal(readFileSync(join(dir, 'Cargo.toml'), 'utf8'), '[workspace.package]\nversion = "0.3.0"\n')
    assert.throws(() => release('set-version', 'latest'), /not a version: latest/)
  })
})
