import type { GlossaryEntry } from './types';

/** Terms of this section. An id must not exist in another file of this folder. */
export const terms = {
  // --- The parts of ferryd -------------------------------------------------
  crate: {
    term: 'crate',
    definition: 'A package of Rust code. A crate is a library that other crates use, or a program.',
  },
  engine: {
    term: 'engine',
    definition: 'The part of `ferryd` that runs the deploys, the reconciler, the cron jobs and the datastores. Its crate is `ferry-engine`.',
    see: ['reconciler', 'deploy-worker'],
  },
  store: {
    term: 'store',
    definition: 'The part of Ferry that reads and writes the SQLite file `ferry.db`. It has the desired state.',
    see: ['sqlite', 'desired-state'],
  },
  'route-table': {
    term: 'route table',
    definition: 'A list in the memory of `ferryd`. For each hostname, it gives the addresses of the instances. The proxy reads it for each request.',
    see: ['proxy'],
  },
  'container-label': {
    term: 'label',
    definition: 'A key and a value that Docker keeps on a container. Ferry reads the labels to find its own containers.',
  },

  // --- The deploy pipeline -------------------------------------------------
  'deploy-worker': {
    term: 'deploy worker',
    definition: 'A background task of the engine. It runs the deploys of one service, one at a time, in order.',
  },
  'service-lock': {
    term: 'service lock',
    definition: 'A lock that each service has. Only the operation that holds it can change the containers of the service.',
    see: ['reconciler'],
  },
  'blue-green': {
    term: 'blue-green deploy',
    definition: 'A deploy that starts the new instances next to the old instances, then moves the traffic. It is the default.',
    see: ['zero-downtime', 'recreate'],
  },
  recreate: {
    term: 'recreate',
    definition: 'A deploy that stops the old instance before it starts the new instance. Ferry uses it for a service with a disk.',
    see: ['disk'],
  },


  // --- The reconciler ------------------------------------------------------
  'oom-watcher': {
    term: 'OOM watcher',
    definition: 'A task of the engine that listens to the `oom` events of Docker. It writes a warning in the server log for each kill.',
    see: ['oom'],
  },
} satisfies Record<string, GlossaryEntry>;
