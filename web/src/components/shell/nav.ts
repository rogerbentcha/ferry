import {
  Activity,
  Boxes,
  Braces,
  Database,
  FileCode2,
  Globe,
  Layers,
  Server,
  UserRound,
  type LucideIcon,
} from 'lucide-react'

export interface NavItem {
  label: string
  to: string
  icon: LucideIcon
  /** Short description (command palette). */
  hint: string
}

/** Primary navigation (icon rail, mobile sheet, command palette). */
export const NAV_ITEMS: NavItem[][] = [
  [{ label: 'Services', to: '/services', icon: Boxes, hint: 'Web services, workers, cron jobs, static sites' }],
  [
    { label: 'Environments', to: '/environments', icon: Layers, hint: 'Production, staging: a private network each' },
    { label: 'Datastores', to: '/datastores', icon: Database, hint: 'Managed Postgres and Redis' },
    { label: 'Env groups', to: '/env-groups', icon: Braces, hint: 'Shared environment variables' },
    { label: 'Blueprints', to: '/blueprints', icon: FileCode2, hint: 'Apply ferry.yaml / render.yaml' },
  ],
  [
    { label: 'Monitoring', to: '/monitoring', icon: Activity, hint: 'CPU, memory, disk and network of the server' },
    { label: 'Domains', to: '/domains', icon: Globe, hint: 'Your domains, their DNS records and certificates' },
  ],
  [
    { label: 'Server', to: '/server', icon: Server, hint: 'Versions, server logs, CLI and git connections, API' },
    { label: 'Account', to: '/account', icon: UserRound, hint: 'Password, API tokens and browser sessions' },
  ],
]

export const REPO_URL = 'https://github.com/rogerbentcha/ferry'
export const DOCS_URL = `${REPO_URL}#readme`
export const DESIGN_DOC_URL = `${REPO_URL}/blob/main/DESIGN.md`
export { API_DOCS_URL, OPENAPI_URL } from '@/lib/api/queries/info'
