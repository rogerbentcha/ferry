import type { GlossaryEntry } from './types';

/** Terms of this section. An id must not exist in another file of this folder. */
export const terms = {
  // --- Resource limits -----------------------------------------------------
  'memory-limit': {
    term: 'memory limit',
    definition: 'The most memory that one container can use. Above it, the kernel stops the process of the container.',
    see: ['oom', 'resource-limit'],
  },
  'cpu-limit': {
    term: 'CPU limit',
    definition: 'The most CPU time that one container can use. At the limit, the container becomes slower. It does not stop.',
  },
  millicore: {
    term: 'millicore',
    definition: 'One thousandth of a CPU core. `500m` is 500 millicores: half a core.',
  },
  mib: {
    term: 'MiB',
    definition: 'A unit of memory and of disk space. 1 MiB is 1024 × 1024 bytes. 1 GiB is 1024 MiB.',
  },
  kernel: {
    term: 'kernel',
    definition: 'The center of the operating system. It gives memory and CPU time to each process, and it applies the limits.',
  },
  sigkill: {
    term: 'SIGKILL',
    definition: 'The signal that stops a process immediately. The process cannot refuse it.',
    see: ['exit-code'],
  },

  // --- Logs & events -------------------------------------------------------
  'deploy-log': {
    term: 'deploy log',
    definition: 'The log of one deploy: the build output and the steps that Ferry does.',
    see: ['log', 'deploy'],
  },
  'runtime-log': {
    term: 'runtime log',
    definition: 'What the containers of a service write while they run. Docker keeps it as long as the containers exist.',
    see: ['log', 'stdout', 'stderr'],
  },
  'job-log': {
    term: 'job log',
    definition: 'The output of one job run.',
    see: ['log', 'job'],
  },
  'server-log': {
    term: 'server log',
    definition: 'The log that `ferryd` writes about its own work: failed deploys, OOM kills, proxy warnings.',
    see: ['ferryd'],
  },
  timestamp: {
    term: 'timestamp',
    definition: 'The date and time when a line was written.',
  },
  latency: {
    term: 'latency',
    definition: 'The time that a service takes to answer a request. Ferry measures it until the headers of the answer go out.',
  },
  prometheus: {
    term: 'Prometheus',
    definition: 'A monitoring program that you install yourself. It collects the numbers of your programs at regular intervals, keeps them, and sends alerts.',
  },

  // --- Self-healing --------------------------------------------------------
  'actual-state': {
    term: 'actual state',
    definition: 'What runs at this moment: the containers and the ports that Docker reports.',
    see: ['desired-state', 'reconciler'],
  },
  orphan: {
    term: 'orphan',
    definition: 'A container of this server whose service or datastore does not exist. The reconciler removes it.',
  },
  'route-watcher': {
    term: 'route watcher',
    definition: 'The loop in Ferry that lists the instances each second and updates the routes of the proxy.',
  },
  'graceful-shutdown': {
    term: 'graceful shutdown',
    definition: 'A stop in order: the server completes the requests in progress, then closes each part, then exits.',
  },
  'name-prefix': {
    term: 'name prefix',
    definition: 'The start of the name of each Docker resource of one Ferry server. The default is `ferry`.',
  },
} satisfies Record<string, GlossaryEntry>;
