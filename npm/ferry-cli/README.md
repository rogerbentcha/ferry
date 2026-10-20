# ferry-cli

`ferry`, the command-line client of [Ferry](https://github.com/rogerbentcha/ferry): deploy apps,
read their logs and manage your services on your own Ferry server, from your laptop or a CI job.

```bash
npm install -g ferry-cli
ferry --version
ferry login --server http://127.0.0.1:7878
```

You need Node.js 22 or newer. No Rust, no compiler: npm installs the ready-made `ferry` binary of
your system.

| System | CPU |
|---|---|
| macOS 14 or newer | Apple Silicon, Intel |
| Linux with glibc 2.35 or newer (Ubuntu 22.04+, Debian 12+) | x86_64, arm64 |

Windows and Alpine (musl) are not supported yet: `ferry` says so and names the supported systems.

To upgrade, run `npm install -g ferry-cli@latest`. Your login stays: it is in
`~/.config/ferry/config.json`, not in the package.

The version of this package is the version of the Ferry release, and of `ferry --version`.

- Documentation: [Install Ferry → Set up the CLI](https://github.com/rogerbentcha/ferry/blob/main/docs/content/docs/getting-started/installation.mdx#set-up-the-cli),
  [CLI reference](https://github.com/rogerbentcha/ferry/blob/main/docs/content/docs/reference/cli.mdx)
- Source and issues: [github.com/rogerbentcha/ferry](https://github.com/rogerbentcha/ferry)

The server, `ferryd`, is not on npm: install it on a Linux server with the
[install script](https://github.com/rogerbentcha/ferry#quick-start).

MIT license.
