/**
 * Raw API calls (DESIGN.md §10). One function per endpoint; the React hooks in
 * `queries/*` wrap these. `ref` = resource id or name; for a service or a
 * datastore, its id or its qualified name (`api`, `api.staging`: see `refs`).
 */

import { api, request, seg } from './client'
import { splitRef } from './refs'
import type {
  ApiTokenView,
  ApplyBlueprint,
  AuthorizeGit,
  AuthStatus,
  BlueprintResult,
  CertificateView,
  ChangePassword,
  CliLoginView,
  ConnectGit,
  CreateApiToken,
  CreatedApiToken,
  CreateDatastore,
  CreateEnvGroup,
  CreateEnvironment,
  CreateService,
  DatastoreView,
  Deploy,
  DomainView,
  EnvGroupView,
  EnvironmentView,
  EnvVar,
  GitAuthorization,
  GitBranches,
  GitCallback,
  GitConnectionView,
  GitRepositoryList,
  JobRun,
  Login,
  MetricsHistory,
  MetricsRange,
  PatchEnv,
  ReplaceEnv,
  RunJobRequest,
  RuntimeStatus,
  ServerInfo,
  ServerMetrics,
  ServiceView,
  SessionView,
  SetupAccount,
  TriggerDeploy,
  UpdateDatastore,
  UpdateService,
} from './types'

const V1 = '/api/v1'

/**
 * `/api/v1/services/<name><sub>` (or `datastores`) for a ref: the name in the
 * path, and the environment of a qualified name outside production in
 * `?environment=` (`buildUrl` adds the other parameters after it).
 */
export function resourcePath(collection: 'services' | 'datastores', ref: string, sub = ''): string {
  const { seg: name, environment } = splitRef(ref)
  const path = `${V1}/${collection}/${seg(name)}${sub}`
  return environment ? `${path}?environment=${encodeURIComponent(environment)}` : path
}

const svc = (ref: string, sub = '') => resourcePath('services', ref, sub)
const dbs = (ref: string, sub = '') => resourcePath('datastores', ref, sub)

export const endpoints = {
  // the account, its sessions and API tokens (the answers of the first three
  // say whether there is a session: a 401 of theirs ends none)
  authStatus: (signal?: AbortSignal) =>
    request<AuthStatus>(`${V1}/auth/status`, { signal, skipAuthRedirect: true }),
  setupAccount: (body: SetupAccount) =>
    request<AuthStatus>(`${V1}/auth/setup`, { method: 'POST', body, skipAuthRedirect: true }),
  login: (body: Login) => request<AuthStatus>(`${V1}/auth/login`, { method: 'POST', body, skipAuthRedirect: true }),
  logout: () => request<void>(`${V1}/auth/logout`, { method: 'POST', skipAuthRedirect: true }),
  changePassword: (body: ChangePassword) => api.post<void>(`${V1}/auth/password`, body),
  listSessions: (signal?: AbortSignal) => api.get<SessionView[]>(`${V1}/auth/sessions`, undefined, signal),
  endSession: (id: string) => api.delete(`${V1}/auth/sessions/${seg(id)}`),
  listApiTokens: (signal?: AbortSignal) => api.get<ApiTokenView[]>(`${V1}/auth/tokens`, undefined, signal),
  createApiToken: (body: CreateApiToken) => api.post<CreatedApiToken>(`${V1}/auth/tokens`, body),
  revokeApiToken: (id: string) => api.delete(`${V1}/auth/tokens/${seg(id)}`),
  getCliLogin: (id: string, signal?: AbortSignal) =>
    api.get<CliLoginView>(`${V1}/auth/cli/${seg(id)}`, undefined, signal),
  approveCliLogin: (id: string) => api.post<CliLoginView>(`${V1}/auth/cli/${seg(id)}/approve`),
  denyCliLogin: (id: string) => api.post<CliLoginView>(`${V1}/auth/cli/${seg(id)}/deny`),

  // info
  info: (signal?: AbortSignal) => api.get<ServerInfo>(`${V1}/info`, undefined, signal),
  serverMetrics: (signal?: AbortSignal) => api.get<ServerMetrics>(`${V1}/server/metrics`, undefined, signal),
  metricsHistory: (range: MetricsRange, signal?: AbortSignal) =>
    api.get<MetricsHistory>(`${V1}/server/metrics/history`, { range }, signal),

  // services
  listServices: (signal?: AbortSignal) => api.get<ServiceView[]>(`${V1}/services`, undefined, signal),
  getService: (ref: string, signal?: AbortSignal) => api.get<ServiceView>(svc(ref), undefined, signal),
  createService: (body: CreateService) => api.post<ServiceView>(`${V1}/services`, body),
  updateService: (ref: string, body: UpdateService) => api.patch<ServiceView>(svc(ref), body),
  deleteService: (ref: string, force = false) => api.delete(svc(ref), force ? { force: true } : undefined),
  serviceStatus: (ref: string, signal?: AbortSignal) => api.get<RuntimeStatus>(svc(ref, '/status'), undefined, signal),
  restartService: (ref: string) => api.post<Deploy>(svc(ref, '/restart')),
  suspendService: (ref: string) => api.post<ServiceView>(svc(ref, '/suspend')),
  resumeService: (ref: string) => api.post<ServiceView>(svc(ref, '/resume')),
  scaleService: (ref: string, instances: number) => api.post<ServiceView>(svc(ref, '/scale'), { instances }),
  rollbackService: (ref: string, deployId: string) => api.post<Deploy>(svc(ref, '/rollback'), { deploy_id: deployId }),
  rotateDeployHook: (ref: string) => api.post<ServiceView>(svc(ref, '/deploy-hook/rotate')),

  // deploys
  listDeploys: (ref: string, limit = 20, signal?: AbortSignal) =>
    api.get<Deploy[]>(svc(ref, '/deploys'), { limit }, signal),
  triggerDeploy: (ref: string, body: TriggerDeploy = {}) => api.post<Deploy>(svc(ref, '/deploys'), body),
  getDeploy: (id: string, signal?: AbortSignal) => api.get<Deploy>(`${V1}/deploys/${seg(id)}`, undefined, signal),
  cancelDeploy: (id: string) => api.post<Deploy>(`${V1}/deploys/${seg(id)}/cancel`),

  // env
  getServiceEnv: (ref: string, signal?: AbortSignal) => api.get<EnvVar[]>(svc(ref, '/env'), undefined, signal),
  replaceServiceEnv: (ref: string, body: ReplaceEnv, restart = false) =>
    api.put<EnvVar[]>(svc(ref, '/env'), body, restart ? { restart: true } : undefined),
  patchServiceEnv: (ref: string, body: PatchEnv, restart = false) =>
    api.patch<EnvVar[]>(svc(ref, '/env'), body, restart ? { restart: true } : undefined),
  linkEnvGroup: (ref: string, group: string) => api.post<ServiceView>(svc(ref, '/env-groups'), { group }),
  unlinkEnvGroup: (ref: string, group: string) => api.delete<ServiceView>(svc(ref, `/env-groups/${seg(group)}`)),

  // custom domains of a service
  listDomains: (ref: string, signal?: AbortSignal) => api.get<string[]>(svc(ref, '/domains'), undefined, signal),
  addDomain: (ref: string, domain: string) => api.post<string[]>(svc(ref, '/domains'), { domain }),
  removeDomain: (ref: string, domain: string) => api.delete<string[]>(svc(ref, `/domains/${seg(domain)}`)),

  // jobs
  listJobs: (ref: string, limit = 20, signal?: AbortSignal) => api.get<JobRun[]>(svc(ref, '/jobs'), { limit }, signal),
  runJob: (ref: string, body: RunJobRequest = {}) => api.post<JobRun>(svc(ref, '/jobs'), body),
  getJob: (id: string, signal?: AbortSignal) => api.get<JobRun>(`${V1}/jobs/${seg(id)}`, undefined, signal),
  cancelJob: (id: string) => api.post<JobRun>(`${V1}/jobs/${seg(id)}/cancel`),

  // datastores
  listDatastores: (signal?: AbortSignal) => api.get<DatastoreView[]>(`${V1}/datastores`, undefined, signal),
  getDatastore: (ref: string, signal?: AbortSignal) => api.get<DatastoreView>(dbs(ref), undefined, signal),
  createDatastore: (body: CreateDatastore) => api.post<DatastoreView>(`${V1}/datastores`, body),
  updateDatastore: (ref: string, body: UpdateDatastore) => api.patch<DatastoreView>(dbs(ref), body),
  deleteDatastore: (ref: string, force = false) => api.delete(dbs(ref), force ? { force: true } : undefined),

  // env groups
  listEnvGroups: (signal?: AbortSignal) => api.get<EnvGroupView[]>(`${V1}/env-groups`, undefined, signal),
  getEnvGroup: (ref: string, signal?: AbortSignal) =>
    api.get<EnvGroupView>(`${V1}/env-groups/${seg(ref)}`, undefined, signal),
  createEnvGroup: (body: CreateEnvGroup) => api.post<EnvGroupView>(`${V1}/env-groups`, body),
  deleteEnvGroup: (ref: string, opts: { force?: boolean; restart?: boolean } = {}) =>
    api.delete(`${V1}/env-groups/${seg(ref)}`, {
      force: opts.force ? true : undefined,
      restart: opts.restart ? true : undefined,
    }),
  replaceEnvGroupEnv: (ref: string, body: ReplaceEnv, restart = false) =>
    api.put<EnvGroupView>(`${V1}/env-groups/${seg(ref)}/env`, body, restart ? { restart: true } : undefined),
  patchEnvGroupEnv: (ref: string, body: PatchEnv, restart = false) =>
    api.patch<EnvGroupView>(`${V1}/env-groups/${seg(ref)}/env`, body, restart ? { restart: true } : undefined),

  // environments (production first; a service or datastore picks one when it is created)
  listEnvironments: (signal?: AbortSignal) => api.get<EnvironmentView[]>(`${V1}/environments`, undefined, signal),
  getEnvironment: (ref: string, signal?: AbortSignal) =>
    api.get<EnvironmentView>(`${V1}/environments/${seg(ref)}`, undefined, signal),
  createEnvironment: (body: CreateEnvironment) => api.post<EnvironmentView>(`${V1}/environments`, body),
  deleteEnvironment: (ref: string) => api.delete(`${V1}/environments/${seg(ref)}`),

  // git connections
  listGitConnections: (signal?: AbortSignal) =>
    api.get<GitConnectionView[]>(`${V1}/git/connections`, undefined, signal),
  authorizeGit: (body: AuthorizeGit) => api.post<GitAuthorization>(`${V1}/git/authorize`, body),
  gitCallback: (body: GitCallback) => api.post<GitAuthorization>(`${V1}/git/callback`, body),
  connectGit: (body: ConnectGit) => api.post<GitConnectionView>(`${V1}/git/connections`, body),
  deleteGitConnection: (id: string, force = false) =>
    api.delete(`${V1}/git/connections/${seg(id)}`, force ? { force: true } : undefined),
  listGitRepositories: (id: string, signal?: AbortSignal) =>
    api.get<GitRepositoryList>(`${V1}/git/connections/${seg(id)}/repositories`, undefined, signal),
  listGitBranches: (repoUrl: string, signal?: AbortSignal) =>
    api.get<GitBranches>(`${V1}/git/branches`, { repo_url: repoUrl }, signal),

  // the server's domains (services are served under them) and certificates
  listServerDomains: (signal?: AbortSignal) => api.get<DomainView[]>(`${V1}/domains`, undefined, signal),
  connectDomain: (name: string) => api.post<DomainView>(`${V1}/domains`, { name }),
  verifyDomain: (ref: string) => api.post<DomainView>(`${V1}/domains/${seg(ref)}/verify`),
  setDefaultDomain: (ref: string) => api.patch<DomainView>(`${V1}/domains/${seg(ref)}`, { is_default: true }),
  disconnectDomain: (ref: string) => api.delete(`${V1}/domains/${seg(ref)}`),
  listCertificates: (signal?: AbortSignal) => api.get<CertificateView[]>(`${V1}/certificates`, undefined, signal),

  // blueprints
  applyBlueprint: (body: ApplyBlueprint) => api.post<BlueprintResult>(`${V1}/blueprints/apply`, body),
}

/** SSE paths (consumed by useLogStream / events.ts). */
export const streamPaths = {
  events: `${V1}/events`,
  deployLogs: (deployId: string) => `${V1}/deploys/${seg(deployId)}/logs`,
  jobLogs: (jobId: string) => `${V1}/jobs/${seg(jobId)}/logs`,
  /** With `?environment=` for a qualified name outside production (`buildUrl` keeps it). */
  serviceLogs: (ref: string) => svc(ref, '/logs'),
  serverLogs: `${V1}/server/logs`,
}
