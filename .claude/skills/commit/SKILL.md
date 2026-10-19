---
name: commit
description: 'Commit the current changes with Conventional Commits messages (`type(scope): description`), split into logical commits. Use when the user asks to commit ("commit", "commite", "fais le commit") or when another skill, such as implement-linear-issue, reaches its commit step.'
argument-hint: "[linear-issue-id] [context]"
---

# Commit

Arguments: $ARGUMENTS

Turn the current changes into one or more commits that follow
[Conventional Commits 1.0](https://www.conventionalcommits.org/en/v1.0.0/).
This skill commits only: it never pushes.

## 1. Look at what changed

```bash
git branch --show-current
git status --short
git diff
git diff --cached
git ls-files --others --exclude-standard
```

- Nothing to commit → say so and stop.
- On `main` → stop and ask: work reaches `main` through `dev`.
- Leave out, and say so, anything that must not be committed: secrets (`.env`,
  keys, tokens), `ferry-data/`, `*.db`, build output, large binaries, files
  unrelated to the work.

## 2. Group the changes

One commit = one logical change that builds and passes its tests on its own.

- Usually that is one commit. Split when the changes mix unrelated things: a
  refactor and a feature, a dependency bump and a fix, two separate fixes.
- Tests go in the same commit as the code they test.
- Generated files (the docs' generated references, `Cargo.lock`,
  `package-lock.json`) go with the change that caused them.

## 3. Write the message

Load the `caveman-commit` skill
([../caveman-commit/SKILL.md](../caveman-commit/SKILL.md)) and write each
message by its rules: terse, why over what, a body only when it earns its
place. Ferry's rules below win where the two differ: the scopes, `Refs:`
instead of `Closes`, and the attribution trailers the environment asks for.
caveman-commit only writes the message; this skill still stages and commits
(step 4).

```
<type>(<scope>)!: <description>

<body>

<footer>
```

### Type

| Type       | When                                                    |
|------------|---------------------------------------------------------|
| `feat`     | a new capability for a user of Ferry                    |
| `fix`      | a bug fix                                               |
| `docs`     | documentation only (`docs/`, README, DESIGN.md)         |
| `refactor` | code changes that neither fix a bug nor add a feature   |
| `perf`     | faster or lighter, same behavior                        |
| `test`     | tests only                                              |
| `build`    | build system, dependencies, Dockerfiles                 |
| `ci`       | `.github/workflows/`                                    |
| `style`    | formatting only, no code change                         |
| `chore`    | anything else that touches no product code              |
| `revert`   | reverts an earlier commit                               |

### Scope

The part of the repo the change lives in. Pick one; leave it out only when the
change really spans the whole repo.

| Scope                                      | Where                         |
|--------------------------------------------|-------------------------------|
| `ferryd`, `cli`                            | `crates/ferryd`, `crates/ferry-cli` |
| `api`, `core`, `engine`, `build`, `docker`, `proxy`, `scm`, `tls` | `crates/ferry-<scope>` |
| `web`                                      | `web/` (the dashboard)        |
| `docs`                                     | `docs/` (the website: landing page and documentation) |
| `examples`                                 | `examples/`                   |
| `ci`                                       | `.github/`                    |
| `deps`                                     | dependency bumps              |
| `claude`                                   | `.claude/` (skills, agents)   |

When the change spans two crates, use the one where the behavior lives (an
API route added in `ferry-api` and called from `web` is `feat(api)`, or two
commits).

### Description (the first line)

- Imperative mood, lowercase first letter, no final period: `add`, `fix`,
  `show` — not `added`, `adds`, `Added`.
- Says what changes for the reader, not which file was touched.
- The whole first line fits in 50 characters when possible, 72 at most.

### Breaking changes

Add `!` after the scope and a `BREAKING CHANGE:` footer when a change breaks
something people or other crates rely on: a public signature in
`crates/*/src/lib.rs` (the DESIGN.md contract), an API route or its JSON, a
CLI flag, the `render.yaml` schema, the data on disk.

### Body

Skip it when the first line says it all. Always write one for a breaking
change, a security fix, a data migration or a revert. Wrapped at 72
columns, caveman-terse:

- what was wrong or missing,
- what changes (a `-` list when there are several parts),
- what stays the same, when a reader could worry about it.

No "this commit", "I", "we", "now": the diff says what.

### Footer

- `Refs: FER-12` when a Linear issue is known: the argument, or the ID in the
  branch name (`feat/fer-12-…` → `FER-12`). Do not write `Closes` in a
  commit: the pull request closes the issue.
- `BREAKING CHANGE: …` when needed.
- Then the attribution trailers the environment asks for (`Co-Authored-By: …`),
  always last.

### Examples

```
feat(api): deploy a service on a matching git tag

Only branch pushes deployed. New optional `deploy_tags` glob: a pushed
tag that matches deploys the commit it points to.

- GitHub and GitLab webhooks read tag pushes.
- API and CLI accept `deploy_tags`; dashboard shows it.

Branch pushes deploy as before.

Refs: FER-12
```

```
fix(proxy): keep the connection when an instance restarts mid-request

Refs: FER-31
```

```
refactor(ferryd)!: move the server options to ferry-core

BREAKING CHANGE: `ferryd::ServerArgs` is now `ferry_core::ServerOptions`.
```

## 4. Commit

- Stage by path, never `git add -A` or `git add .`, so nothing unexpected
  slips in.
- Pass the message through a heredoc so the formatting survives:

  ```bash
  git add path/one path/two
  git commit -F - <<'EOF'
  feat(web): show the deploy status on the service page

  …
  EOF
  ```

- A pre-commit hook fails → fix the cause, then commit again. Never
  `--no-verify`, never `--amend` a commit that is already pushed.

## 5. Check

```bash
git log --oneline -n <number of commits made>
git status --short
```

Report in the `caveman` voice ([../caveman/SKILL.md](../caveman/SKILL.md)):
the commits made (hash + first line) and anything left out on purpose.
