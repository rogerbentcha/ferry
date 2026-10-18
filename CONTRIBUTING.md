# Contributing to Ferry

This guide is for people who change the code, the dashboard or the docs of Ferry. To learn what Ferry does and how to use it, read the [README](README.md).
[DESIGN.md](DESIGN.md) is the contract between the crates. Read the section about your area before you change a behavior.

## What the repository holds

| Path | What it is |
|---|---|
| `crates/ferry-core` | Models, DTOs, config, the `Store` (SQLite), env resolution, the `Engine` trait |
| `crates/ferry-docker` | A typed wrapper of the Docker Engine API |
| `crates/ferry-build` | Git fetch, runtime detection, Dockerfile generation, `docker build` |
| `crates/ferry-scm` | The git providers: GitHub and GitLab |
| `crates/ferry-proxy` | The reverse proxy and its route table |
| `crates/ferry-tls` | ACME certificates |
| `crates/ferry-engine` | Deploys, the reconciler, cron, jobs, datastores, logs |
| `crates/ferry-api` | REST, SSE, webhooks, blueprints, OpenAPI. It embeds the dashboard. |
| `crates/ferry-cli` | The `ferry` CLI |
| `crates/ferryd` | The server binary. It wires the crates together. |
| `web/` | The dashboard: React, TypeScript, Vite ([web/README.md](web/README.md)) |
| `docs/` | The docs site: Fumadocs on Next.js ([docs/README.md](docs/README.md)) |
| `landing/` | The landing page: React, TypeScript, Vite ([landing/README.md](landing/README.md)) |
| `examples/` | Example apps. The end-to-end tests use them as fixtures. |
| `install.sh`, `.github/` | The install script, and the CI and release workflows |

`web/`, `docs/` and `landing/` each have their own `package.json`. The Rust build never runs Node.
The three share the colors and fonts of `web/src/styles/tokens.css`.

## Set up your tools

| Tool | Version | You need it for |
|---|---|---|
| Rust | 1.89 or newer (CI uses 1.95.0) | `ferryd`, `ferry` and each crate |
| Docker | The daemon. Docker Desktop on macOS works. | Running `ferryd` and the end-to-end tests |
| `git` and `docker` CLIs | On your `PATH` | Builds: `ferry-build` runs them |
| Node.js and npm | 20.19 or newer (CI uses 22) | `web/`, `docs/` and `landing/` |
| `openssl` CLI | Optional | The GitHub App tests of `ferry-api`. Without it, they are skipped. |

## Build and run Ferry locally

Build the dashboard first. This step is optional. Then build the server and the CLI:

```bash
cd web && npm ci && npm run build && cd ..
cargo build
```

You get `target/debug/ferryd` and `target/debug/ferry`. `cargo build --release` puts them in `target/release/`.
`ferry-api` embeds `web/dist` when it compiles. Without `web/dist`, the server still works, and its page at `/` explains how to build the dashboard.

Start the server:

```bash
./target/debug/ferryd run --data-dir ./ferry-data
```

`ferryd run` stays in the foreground: you see the log, and Ctrl+C stops the server.
On the first start, the server prints a link. Open it once to create the account of the dashboard.
git ignores `./ferry-data`. For more detail in the log, set `RUST_LOG=debug`, or `RUST_LOG=ferry_engine=debug` for one crate. The default level is `info`.
The README lists every `ferryd` command in [Running the server](README.md#running-the-server) and every flag in [Server configuration](README.md#server-configuration).

### Deploy an example

Point the CLI at your server with the server token. You then do not need `ferry login`:

```bash
export FERRY_SERVER=http://127.0.0.1:7878
export FERRY_TOKEN="$(cat ferry-data/api_token)"
cd examples/node-hello
../../target/debug/ferry up --follow
curl http://node-hello.localhost:8080
```

The two variables do not change the config that `ferry login` saved for your usual server. `cargo run -p ferry-cli -- <args>` also runs the CLI.

### Keep your real server safe

A Ferry server may already run on your machine. Then give the development server its own data directory, name prefix and ports:

```bash
./target/debug/ferryd run \
  --data-dir ./dev-data \
  --name-prefix ferrydev \
  --api-addr 127.0.0.1:17878 \
  --proxy-addr 127.0.0.1:18080
```

`ferryd` refuses to start on a name prefix that another data directory owns. This protects the containers of your real server.

## Work on the dashboard

Start `ferryd` as above. Then start the Vite dev server:

```bash
cd web
npm install
FERRY_API_URL=http://127.0.0.1:7878 npm run dev    # → http://localhost:5173
```

Open `http://localhost:5173` and sign in with the account of the server. Each change of the code shows in the browser at once.
In development, `/dev/patterns` shows the shared UI patterns on one page.

How the dashboard talks to `ferryd`:

- It uses only the public REST API, like the CLI: JSON calls to `/api/v1/*` on its own origin.
- In development, Vite sends `/api`, `/hooks` and `/healthz` to `FERRY_API_URL`.
- The browser sends the session cookie. The app sets no `Authorization` header and keeps no token.
- `src/lib/api/client.ts` wraps `fetch`. It turns an error into `ApiError { status, code, message }`.
- Live data comes as Server-Sent Events: the log streams and the change feed `/api/v1/events`. `src/lib/api/events.ts` turns each change into TanStack Query invalidations.

Where the code is, in `web/src/`:

| Path | What it holds |
|---|---|
| `pages/` | The pages. `routes.tsx` gives each page its URL. |
| `components/` | `ui/` (the shadcn primitives), `shell/` and `patterns/` |
| `lib/api/` | The API client, the types, the endpoints, the query keys, the hooks (`queries/`), SSE |
| `stores/` | The Zustand stores: `auth.ts` and `ui.ts` |
| `styles/tokens.css` | The design tokens, shared with `docs/` and `landing/` |

When the API changes, update these files in this order:

1. `src/lib/api/types.ts`. It copies `dto.rs`, `models.rs` and `logs.rs` of `ferry-core` by hand.
2. `src/lib/api/endpoints.ts` for the call. Add a key to `src/lib/api/keys.ts` for new data in the cache.
3. The query and mutation hooks in `src/lib/api/queries/`.
4. `src/lib/api/events.ts`, when a change event must refresh the data.

To try a production build without a Rust compile, run `npm run build` in `web/`. Then run `FERRY_UI_DIR=web/dist ./target/debug/ferryd` in the repository root.
[web/README.md](web/README.md) explains the stack, the npm scripts and how `ferryd` serves the app.

## Work on the docs site or the landing page

- The docs site: [docs/README.md](docs/README.md) tells how to run, generate and build it. Read [docs/AUTHORING.md](docs/AUTHORING.md) before you write a page.
- The landing page: read [landing/README.md](landing/README.md).

## Code rules

[DESIGN.md §16](DESIGN.md#16-coding-rules) gives the rules. In short:

- Rust 2024, stable toolchain. `rustfmt.toml` sets a width of 120 columns.
- No `unwrap()` or `expect()` outside tests, except on an invariant that a comment explains. Leave no `todo!()`.
- Return `ferry_core::Error`, with context in the message: `pulling image nginx: …`. Its variant sets the HTTP status (`Invalid` 400, `NotFound` 404, `Conflict` 409).
- Log with `tracing`: `info` for lifecycle events, `debug` for detail. Never log secrets.
- Run tar and heavy filesystem work in `spawn_blocking`.
- Public signatures in `crates/*/src/lib.rs` are the contract between crates. Change one only on purpose, and say so in the pull request.
- Add a new dependency to the `Cargo.toml` of the crate, with an explicit version. Do not add it to the workspace root.
- A crate depends only on the crates that the table of [DESIGN.md §2](DESIGN.md#2-architecture) allows.

## Run the checks before a pull request

CI (`.github/workflows/ci.yml`) runs these checks on Linux for each pull request. Only the `landing/` check is not in CI. Run the checks of each area that you changed, until they pass:

| Area | Run in | Commands |
|---|---|---|
| `crates/` | the repository root | `cargo fmt --all`, `cargo clippy --workspace --tests -- -D warnings`, `cargo test --workspace` |
| `web/` | `web/` | `npm run typecheck`, `npm run lint`, `npm test`, `npm run build` |
| `docs/` | `docs/` | `npm run generate:check`, `npm run typecheck`, `npm run check-content -- --strict`, `npm run build` |
| `landing/` | `landing/` | `npm run typecheck` |
| `install.sh` | the repository root | `shellcheck install.sh .github/scripts/install.test.sh` |
| `.github/scripts/` | the repository root | `node --test .github/scripts/release.test.mjs` |

- `cargo fmt --all` fixes the format. CI runs `cargo fmt --all --check`, which only reports a difference.
- `-D warnings` turns each clippy warning into an error.
- `npm run generate:check` needs `cargo`. Run `npm ci` once in each npm folder first.
- After a change to `web/src/styles/tokens.css`, build `web/`, `docs/` and `landing/` again.

### Docker end-to-end tests

The tests that need Docker run only with `FERRY_E2E=1`. Without it, they print `skipped` and pass. CI runs them on each pull request.
Run them when you change Docker, builds, the proxy or the engine:

```bash
cargo build -p ferryd                  # the CLI suite drives a real ferryd binary
FERRY_E2E=1 cargo test --workspace
FERRY_E2E=1 cargo test -p ferry-engine --test e2e    # one suite: the crate, then the test name
```

The suites are the `*e2e*.rs` files in `crates/*/tests/`, and `crates/ferryd/tests/background.rs`.
The tests create containers, volumes, networks and images. They pull small public images and bind free local ports.
Each test uses its own name prefix, so it never touches the resources of a Ferry server. Still, run them on a Docker daemon where this load is acceptable.
If you cannot run them, say so in the pull request.

A run that stops in the middle can leave containers. Remove them by prefix:

```bash
docker ps -aq --filter label=ferry.instance=<prefix> | xargs docker rm -f
```

A new end-to-end test follows [DESIGN.md §17](DESIGN.md#17-testing):

1. Check `FERRY_E2E=1` first. Without it, print `skipped` and return.
2. Use your own name prefix (`ferrytest-<crate>-<random>`), free ports and a temporary data directory.
3. Remove everything that the test creates, also after a failure.
4. Use small base images only: `python:3.12-alpine`, `node:22-alpine`, `nginx:alpine`, `busybox:stable`, `postgres:16-alpine`, `redis:7-alpine`.
5. Use the apps in `examples/` as fixtures.

## Branches, commits and pull requests

### Branches

`dev` is the base branch. Start each change from `dev`. Open the pull request into `dev`, never into `main`: work reaches `main` through `dev`.

```bash
git fetch origin dev
git switch -c feat/fer-12-deploy-on-tag origin/dev
```

Name the branch `<type>/<short-slug>`, with the commit type of the change (see below).
For a Linear issue, put its ID in lowercase before the slug, as above. Linear then links the branch and the pull request to the issue.
One issue is one branch and one pull request.

### Commit messages

Ferry uses [Conventional Commits 1.0](https://www.conventionalcommits.org/en/v1.0.0/): `<type>(<scope>)!: <description>`, then an optional body and footer.

- **Type:** `feat` (a new capability for users), `fix`, `docs`, `refactor`, `perf`, `test`, `build`, `ci`, `style`, `chore` or `revert`.
- **Scope:** the part of the repository. Use `api`, `core`, `engine`, `build`, `docker`, `proxy`, `scm` or `tls` for `crates/ferry-<scope>`. Use `ferryd`, `cli`, `web`, `docs`, `landing`, `examples`, `ci`, `deps` or `claude` for the rest.
- **Description:** imperative mood, lowercase first letter, no final period. Keep the first line to 72 characters at most, 50 when possible. Example: `fix(proxy): keep the connection when an instance restarts mid-request`.
- **Breaking change:** add `!` after the scope and a `BREAKING CHANGE:` footer. Examples: a public signature in `crates/*/src/lib.rs`, an API route or its JSON, a CLI flag, the `render.yaml` schema, the data on disk.
- **Footer:** `Refs: FER-12` for a Linear issue. Do not write `Closes` in a commit: the pull request closes the issue.

One commit is one logical change that builds and passes its tests. Put the tests in the commit of the code that they test.
Put generated files (the docs references, `Cargo.lock`, `package-lock.json`) in the commit of the change that caused them.
Stage the files by path. Never use `--no-verify`.
The type matters: a push to `main` with a `feat`, `fix` or breaking commit publishes a release. See [Development](README.md#development) in the README.

### In the same change

| If you change | Also do this |
|---|---|
| A CLI command, a `ferryd` flag, an API route or a DTO | Run `npm run generate` in `docs/`. Commit the result. |
| Something that users see: the API, the CLI, the configuration, the dashboard | Update the pages in `docs/`. They follow `docs/AUTHORING.md`. |
| A contract between crates, an API route or a documented behavior | Update `DESIGN.md`. |

### Pull requests

- **Title:** a Conventional Commits first line for the whole change. With one commit, use its first line.
- **Body:** what changes, how you tested it, and each check that you did not run, with the reason.
- **Linear:** write `Closes FER-12` in the body. When the pull request is merged, Linear moves the issue to "Done".

With Claude Code, `/implement-linear-issue <issue URL>` follows these steps, and the `commit` skill writes the commit messages. Both are in `.claude/skills/`.
