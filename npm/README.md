# The npm packages of the ferry CLI

`npm install -g ferry-cli` installs the Rust `ferry` binary of the release. Same pattern as
esbuild and Biome: no download at install time, so `--ignore-scripts` and npm mirrors work.

| Package | What it holds |
|---|---|
| `ferry-cli` ([`ferry-cli/`](ferry-cli)) | The `ferry` command: a Node launcher that runs the binary of this system. Its `optionalDependencies` name the four packages below. |
| `ferry-cli-darwin-arm64`, `ferry-cli-darwin-x64`, `ferry-cli-linux-arm64`, `ferry-cli-linux-x64` | `bin/ferry` from the release archive of that system. Their `os` / `cpu` (and `libc` on Linux) make npm install only the one of the machine. |

The four binary packages are not in the repository: `.github/scripts/npm.mjs stage` makes them
from the release archives, with the table `PLATFORMS` of
[`ferry-cli/lib/platform.js`](ferry-cli/lib/platform.js). Every package has the release version:
`release.mjs set-version` writes it into `ferry-cli/package.json` and pins the four
`optionalDependencies` to it.

## A release

In `.github/workflows/release.yml`:

1. `npm — <system>` (before the GitHub release): stages the packages from the release assets,
   packs them, installs `ferry-cli` with `npm install -g` on Linux x86_64 and arm64 and macOS
   arm64 and Intel, and runs `ferry --version`.
2. `Publish to npm` (after the GitHub release): publishes the four binary packages, then
   `ferry-cli`, with provenance. A package already on npm at this version is skipped, so a
   failed run can be run again.

## One-time setup on npm

An agent cannot do this. Someone with an npm account must:

1. Publish once with a token, because npm configures trusted publishing in the settings of a
   package that exists. Create an npm granular access token with read and write access to all
   packages (the five do not exist yet). If the account requires two-factor authentication to
   publish, allow the token to bypass it, or the publish fails with `EOTP`. Add the token to the
   repository as the Actions secret `NPM_TOKEN`. The next release publishes the five packages,
   and that account owns them. The token expires: npm limits its lifetime.
2. For each of the five packages, on npmjs.com → package → **Settings** → **Trusted publishing**:
   GitHub Actions, repository `rogerbentcha/ferry`, workflow `release.yml`. Then, in the same
   settings, require two-factor authentication and disallow tokens.
3. Delete the `NPM_TOKEN` secret and revoke the token. The workflow then publishes with OIDC
   (`id-token: write`), with no secret.

Without a token or trusted publishing, the GitHub release still ships, and `Publish to npm`
fails. After the setup, re-run that job of the most recent release only: npm moves the `latest`
tag to each version it publishes, so the job of an older release would move `latest` back.
Right after a partial publish, npm can take a few minutes to show the new versions: a re-run that
fails with `E403` ("cannot publish over") works a few minutes later.

`ferry-cli` was published once by someone else and unpublished in 2022: npm never accepts its
version 2.2.1 again. Ferry's versions are 0.x, so this does not matter for years.
