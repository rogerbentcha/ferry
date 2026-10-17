---
name: implement-linear-issue
description: Implement a Linear issue end to end — read it, branch from dev, write the code and its tests, loop with the code-reviewer agent until it approves (3 rounds at most), commit with the commit skill, push and open a pull request into dev. Run it as /implement-linear-issue <Linear issue URL>.
argument-hint: <linear-issue-url>
disable-model-invocation: true
---

# Implement a Linear issue

Issue: $ARGUMENTS

Take one Linear issue from "to do" to "pull request ready for review". The
human reviews and merges the pull request: you never merge.

## Ground rules

- The base branch is `dev`. Pull requests go into `dev`, never `main`.
- One issue = one branch = one pull request.
- Keep the task list up to date with the steps below.
- Stop and ask the human, instead of guessing, when:
  - the issue is too vague to implement safely, or contradicts the code or
    DESIGN.md;
  - the work needs a change to a public signature in `crates/*/src/lib.rs`
    (the DESIGN.md contract);
  - it needs a secret, an account or access you do not have;
  - the review loop reaches its limit (step 5).

## 1. Check the setup

1. `$ARGUMENTS` is a Linear issue URL
   (`https://linear.app/<workspace>/issue/FER-12/<slug>`) or an ID
   (`FER-12`). Take the ID from it. Nothing given → ask for it and stop.
2. The Linear tools are available (the Linear MCP server or connector). If
   not, stop and tell the human to connect Linear:
   - in a local session: `claude mcp add --transport http linear https://mcp.linear.app/mcp`,
     then `/mcp` to sign in;
   - on claude.ai: the Linear connector in the settings.
3. `gh auth status` succeeds.
4. `git status --porcelain` is empty. If not, stop and ask: their changes
   would end up in the pull request.

## 2. Read the issue

1. Get the issue — title, description, labels, priority, parent and
   sub-issues, attachments — and its comments (`get_issue`, `list_comments`;
   the names can differ a little between versions of the Linear tools).
   Open the documents and images it links to.
2. Write down, for yourself:
   - the goal, in one sentence;
   - the acceptance criteria — when the issue has none, derive them from the
     description; they go in the pull request;
   - what is out of scope.
3. Too vague → ask the human precise questions and stop.
4. Move the issue to the team's "In Progress" status.

## 3. Create the branch

1. `git fetch origin dev`
2. Type: `fix` for a bug (label "Bug", or the issue describes broken
   behavior), otherwise `feat`; `docs`, `refactor` or `chore` when the issue
   is only that.
3. Name: `<type>/<id in lowercase>-<short slug>`, e.g.
   `feat/fer-12-deploy-on-tag`. The issue ID in the branch name is what lets
   Linear link the branch and the pull request to the issue.
4. `git switch -c <branch> origin/dev`.

Branch already there (a second run on the same issue) → switch to it and
continue from where it stands: `git log origin/dev..HEAD`,
`gh pr list --head <branch>`.

## 4. Implement

1. Read before writing: the DESIGN.md sections about the area, the code
   around the change, a similar feature. Follow its patterns, naming and
   comment density.
2. Put a short plan in the task list.
3. Write the code and its tests: new behavior → a test; a bug → a test that
   fails before the fix.
4. A change users can see (API, CLI, configuration, dashboard) → update the
   pages in `docs/` (they follow `docs/AUTHORING.md`) and the generated
   references (`npm run generate` in `docs/`).
5. Run the checks for the areas touched — the same as CI — until they pass:

   | Area touched | Checks |
   |---|---|
   | `crates/` | `cargo fmt --all`, `cargo clippy --workspace --tests -- -D warnings`, `cargo test --workspace` |
   | `web/` | `npm run typecheck`, `npm run lint`, `npm test` (in `web/`) |
   | `docs/` | `npm run generate:check`, `npm run typecheck`, `npm run check-content -- --strict` (in `docs/`) |
   | `landing/` | `npm run typecheck` (in `landing/`) |

   Docker end-to-end tests (`FERRY_E2E=1 cargo test --workspace`) when Docker
   paths changed and Docker is running; otherwise say so in the pull request.

## 5. Review loop — 3 rounds at most

For each round, 1 to 3:

1. Call the Agent tool with `subagent_type: "code-reviewer"`. In the prompt:
   the issue ID and title, the goal, the acceptance criteria, the base ref
   `origin/dev`, the round number and, from round 2, the previous round's
   blocking findings with what you changed for each.
2. Read the last line of its answer.
   - `VERDICT: APPROVED` → go to step 6. Keep its suggestions for the pull
     request.
   - `VERDICT: CHANGES_REQUESTED` → fix every blocking finding, run the
     checks again, start the next round. If you disagree with a finding, do
     not skip it silently: explain why in the next round's prompt, and the
     reviewer decides.

Still not approved after round 3 → stop. Do not commit. Tell the human what is
left (the blocking findings), what you tried and what you think, post the same
as a comment on the Linear issue, and wait for their answer.

## 6. Commit

Use the `commit` skill (Skill tool) with the issue ID as its argument, e.g.
`FER-12`. It writes Conventional Commits messages with `Refs: FER-12`.

## 7. Push and open the pull request

1. `git push -u origin <branch>`
2. Open the pull request into `dev`:

   ```bash
   gh pr create --base dev --head <branch> --title "<title>" --body-file - <<'EOF'
   <body>
   EOF
   ```

   - Title: a Conventional Commits first line for the whole change, e.g.
     `feat(api): deploy a service when its git tag matches a pattern` — the
     commit's own first line when there is a single commit.
   - Body:

     ```markdown
     ## Linear
     Closes FER-12 — <issue title>
     <issue URL>

     ## What changes
     - …

     ## Acceptance criteria
     - [x] …

     ## How it was tested
     - `cargo test --workspace` — passed
     - Not run: … (why)

     ## Review
     Approved by the code-reviewer agent in round <N>.
     Suggestions left for later:
     - … (or "None.")

     🤖 Generated with [Claude Code](https://claude.com/claude-code)
     ```

   `Closes FER-12` is a Linear magic word: with Linear's GitHub integration,
   merging the pull request moves the issue to "Done".
3. Never merge, never turn on auto-merge.

## 8. Report

1. Comment on the Linear issue: the pull request's link and one line on what
   it does.
2. Tell the human, in a few lines: the pull request's link, what changed, and
   what was not verified.
