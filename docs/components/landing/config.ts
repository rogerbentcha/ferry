import { githubUrl } from '@/lib/shared'

export { githubUrl }

/** A docs page of this site, e.g. `docs('guides/production')`. Static-export URLs end with a slash. */
export const docs = (page = '') => `/docs/${page ? `${page}/` : ''}`

/** The one-command install of the server (install.sh, from the latest release). */
export const installCommand = `curl -fsSL ${githubUrl}/releases/latest/download/install.sh | sudo sh`

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
