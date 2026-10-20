// Which npm package holds the ferry binary of this system, or why there is none.
// The release workflow builds the binaries (.github/workflows/release.yml), and
// .github/scripts/npm.mjs makes one package of each from this table.

import { release } from 'node:os'

/**
 * The systems ferry runs on: Node's `process.platform` / `process.arch`, the
 * npm package, the Rust target of its release archive, and a name for people.
 */
export const PLATFORMS = [
  {
    os: 'darwin',
    cpu: 'arm64',
    package: 'ferry-cli-darwin-arm64',
    target: 'aarch64-apple-darwin',
    label: 'macOS on Apple Silicon',
  },
  {
    os: 'darwin',
    cpu: 'x64',
    package: 'ferry-cli-darwin-x64',
    target: 'x86_64-apple-darwin',
    label: 'macOS on Intel',
  },
  {
    os: 'linux',
    cpu: 'arm64',
    package: 'ferry-cli-linux-arm64',
    target: 'aarch64-unknown-linux-gnu',
    label: 'Linux arm64 (glibc)',
  },
  {
    os: 'linux',
    cpu: 'x64',
    package: 'ferry-cli-linux-x64',
    target: 'x86_64-unknown-linux-gnu',
    label: 'Linux x86_64 (glibc)',
  },
]

/** The oldest glibc of the Linux binaries (Ubuntu 22.04), as release.yml checks it. */
export const MIN_GLIBC = '2.35'
/** macOS 14, the oldest the macOS binaries run on, has Darwin 23. */
export const MIN_DARWIN = 23

const SUPPORTED =
  'ferry runs on macOS 14 or newer (Apple Silicon or Intel), and on Linux x86_64 or arm64 ' +
  'with glibc 2.35 or newer (Ubuntu 22.04 or newer, Debian 12 or newer).'

/**
 * `{ package }`: the npm package with the binary of this system. `{ error }`:
 * the system is not supported, and the message names the supported ones.
 * `glibc` is the version of a Linux's glibc (none on musl); `darwin` is the
 * major version of the macOS kernel.
 */
export function platformPackage({ os, cpu, glibc, darwin }) {
  const found = PLATFORMS.find((p) => p.os === os && p.cpu === cpu)
  if (!found) return { error: `${systemName(os, cpu)} is not supported yet. ${SUPPORTED}` }
  if (os === 'linux' && !glibc) {
    return { error: `this Linux has no glibc (Alpine and other musl systems are not supported yet). ${SUPPORTED}` }
  }
  if (os === 'linux' && olderThan(glibc, MIN_GLIBC)) {
    return { error: `this Linux has glibc ${glibc}, older than ${MIN_GLIBC}. ${SUPPORTED}` }
  }
  if (os === 'darwin' && darwin < MIN_DARWIN) {
    return { error: `this macOS is older than macOS 14. ${SUPPORTED}` }
  }
  return { package: found.package }
}

/** The facts of this system that platformPackage reads. */
export function thisSystem() {
  const os = process.platform
  let glibc
  if (os === 'linux') {
    // Node reports the glibc it runs with, and nothing on musl.
    process.report.excludeNetwork = true
    glibc = process.report.getReport().header.glibcVersionRuntime
  }
  const darwin = os === 'darwin' ? Number(release().split('.')[0]) : undefined
  return { os, cpu: process.arch, glibc, darwin }
}

/** `Windows x86_64`, `Linux s390x`, … for a message. */
function systemName(os, cpu) {
  const names = { win32: 'Windows', darwin: 'macOS', linux: 'Linux' }
  const cpus = { x64: 'x86_64', ia32: 'x86' }
  return `${names[os] ?? os} ${cpus[cpu] ?? cpu}`
}

/** True when version `a` (`X.Y`) is older than `b`. */
function olderThan(a, b) {
  const [x, y] = [a, b].map((v) => v.split('.').map(Number))
  for (let i = 0; i < Math.max(x.length, y.length); i++) {
    if ((x[i] ?? 0) !== (y[i] ?? 0)) return (x[i] ?? 0) < (y[i] ?? 0)
  }
  return false
}
