/**
 * Where the docs site (../docs, a static export) is served. Set VITE_DOCS_URL
 * when building for a public site; the default is the docs' local dev server.
 */
const docsBase = (import.meta.env.VITE_DOCS_URL ?? 'http://localhost:3000').replace(/\/+$/, '')

/** A docs page, e.g. `docs('guides/production')`. Static-export URLs end with a slash. */
export const docs = (page = '') => `${docsBase}/docs/${page ? `${page}/` : ''}`

export const githubUrl = 'https://github.com/rogerbentcha/ferry'

export const links = {
  quickstart: docs('getting-started/quickstart'),
  installation: docs('getting-started/installation'),
  cli: docs('reference/cli'),
  api: docs('reference/api-overview'),
  blueprint: docs('reference/blueprint'),
  deploys: docs('guides/rollbacks'),
  builds: docs('guides/deploy'),
  services: docs('getting-started/concepts'),
  datastores: docs('guides/databases'),
  networking: docs('guides/domains'),
  jobs: docs('guides/jobs'),
  logs: docs('guides/logs'),
  production: docs('guides/production'),
  githubAutoDeploy: docs('guides/github-auto-deploy'),
  postgresApp: docs('guides/databases'),
  migrate: `${docs('reference/blueprint')}#differences-from-renderyaml`,
  architecture: docs('architecture'),
  contributing: `${githubUrl}/blob/main/CONTRIBUTING.md`,
  license: `${githubUrl}/blob/main/LICENSE`,
}
