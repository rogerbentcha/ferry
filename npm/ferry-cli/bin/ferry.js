#!/usr/bin/env node
// The `ferry` command of the npm package ferry-cli. npm installed, next to it,
// the one package of `optionalDependencies` whose `os` and `cpu` match this
// system: this runs the ferry binary in it, with the same arguments, terminal
// and exit code. No download at install time, so --ignore-scripts and npm
// mirrors work.

import { spawn } from 'node:child_process'
import { createRequire } from 'node:module'
import { constants } from 'node:os'
import { dirname, join } from 'node:path'
import { platformPackage, thisSystem } from '../lib/platform.js'

function fail(message) {
  console.error(`ferry: ${message}`)
  process.exit(1)
}

const found = platformPackage(thisSystem())
if (found.error) fail(found.error)

let binary
try {
  const manifest = createRequire(import.meta.url).resolve(`${found.package}/package.json`)
  binary = join(dirname(manifest), 'bin', 'ferry')
} catch {
  fail(
    `the package ${found.package}, which holds the ferry of this system, is not installed. ` +
      'npm leaves it out with --omit=optional or --no-optional. Install again: npm install -g ferry-cli',
  )
}

const child = spawn(binary, process.argv.slice(2), { stdio: 'inherit' })

// A signal to this process goes on to ferry, which decides. Ctrl+C reaches
// ferry twice (from the terminal and from here): its handler takes both.
for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP', 'SIGQUIT']) process.on(signal, () => child.kill(signal))

child.on('error', (err) => fail(`cannot run ${binary}: ${err.message}`))
child.on('exit', (code, signal) => {
  if (signal) {
    // Stop with the same signal, so the shell sees how ferry ended. Node
    // ignores some signals (SIGPIPE): then exit with the shell's 128 + n.
    process.removeAllListeners(signal)
    process.kill(process.pid, signal)
    setTimeout(() => process.exit(128 + (constants.signals[signal] ?? 0)), 100)
    return
  }
  process.exit(code)
})
