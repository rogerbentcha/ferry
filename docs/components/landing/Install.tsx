import { githubUrl, installCommand, links } from '@/components/landing/config'

import { CommandBlock } from './code'
import { Reveal } from './Reveal'
import { Heading, Section } from './ui'

const INSTALL = [installCommand]

const TUNNEL = ['ssh -N -L 7878:127.0.0.1:7878 root@your-server']

const SERVER = [
  `git clone ${githubUrl} && cd ferry`,
  '(cd web && npm ci && npm run build)',
  'cargo build --release',
  './target/release/ferryd',
]

const FIRST_APP = [
  'export PATH="$PWD/target/release:$PATH"',
  'ferry login',
  'cd examples/node-hello && ferry up --follow',
  'curl http://node-hello.localhost:8080',
]

export function Install() {
  return (
    <Section id="install" labelledBy="install-title">
      <div className="grid gap-x-16 gap-y-12 lg:grid-cols-[minmax(0,7fr)_minmax(0,5fr)]">
        <div className="min-w-0">
          <Heading id="install-title" strong="One command on a fresh server" quiet="or build it on your own machine" />
          <Reveal delay={0.12}>
            <p className="mt-5 max-w-[34rem] text-foreground-lighter">
              On Ubuntu 22.04, 24.04 or 26.04, or Debian 12 or 13 (x86_64 or arm64), the install script sets up Docker, git and Ferry
              as a systemd service. Elsewhere you need Docker, and Docker Desktop is fine on a Mac. To build from source
              you also need Rust 1.89 or later and Node.js 20 or later.
            </p>
          </Reveal>

          <ol className="mt-10 space-y-8">
            <li>
              <Reveal>
                <h3 className="text-base font-semibold">Install on your server</h3>
                <p className="mt-1 mb-3 text-sm text-foreground-lighter">
                  Run it as root or with <code className="inline-code">sudo</code>. Run it again to upgrade: your apps,
                  data and settings stay.
                </p>
                <CommandBlock lines={INSTALL} label="Copy the install command" />
              </Reveal>
            </li>
            <li>
              <Reveal>
                <h3 className="text-base font-semibold">Open the dashboard</h3>
                <p className="mt-1 mb-3 text-sm text-foreground-lighter">
                  The script ends with an SSH tunnel and a link. Run the tunnel on your computer, then open the link to
                  create your account.
                </p>
                <CommandBlock lines={TUNNEL} label="Copy the tunnel command" />
              </Reveal>
            </li>
            <li>
              <Reveal>
                <h3 className="text-base font-semibold">Or build and start it on your machine</h3>
                <p className="mt-1 mb-3 text-sm text-foreground-lighter">
                  <code className="inline-code">ferryd</code> starts in the background and prints a link: open it to
                  create your account. <code className="inline-code">ferryd stop</code> stops the server.
                </p>
                <CommandBlock lines={SERVER} label="Copy the server commands" />
              </Reveal>
            </li>
            <li>
              <Reveal>
                <h3 className="text-base font-semibold">Deploy the example app</h3>
                <p className="mt-1 mb-3 text-sm text-foreground-lighter">
                  In the clone, <code className="inline-code">ferry login</code> opens the dashboard, where you approve
                  the terminal. The dashboard is at <code className="inline-code">http://127.0.0.1:7878</code>.
                </p>
                <CommandBlock lines={FIRST_APP} label="Copy the deploy commands" />
              </Reveal>
            </li>
          </ol>
        </div>

        <div className="space-y-3 lg:pt-[7.5rem]">
          <Reveal className="rounded-[var(--ferry-radius-xl)] border border-border bg-surface-100 p-6">
            <h3 className="text-base font-semibold">Put it on a server</h3>
            <p className="mt-2 text-sm text-foreground-lighter">
              Point a wildcard DNS record at the server, then give the install script your domain with{' '}
              <code className="inline-code">--base-domain</code>. Add <code className="inline-code">--acme-email</code>{' '}
              and it gets HTTPS certificates from Let’s Encrypt.
            </p>
            <a className="text-link mt-4 inline-block text-sm" href={links.production}>
              Read the production guide
            </a>
          </Reveal>
          <Reveal delay={0.08} className="rounded-[var(--ferry-radius-xl)] border border-border bg-surface-100 p-6">
            <h3 className="text-base font-semibold">Click, type or script it</h3>
            <p className="mt-2 text-sm text-foreground-lighter">
              Manage everything from the web dashboard, the <code className="inline-code">ferry</code> CLI or the REST
              API. The API is described in OpenAPI 3.1, and each server has its own Swagger UI at{' '}
              <code className="inline-code">/api/docs</code>.
            </p>
            <a className="text-link mt-4 inline-block text-sm" href={links.api}>
              Browse the API reference
            </a>
          </Reveal>
        </div>
      </div>
    </Section>
  )
}
