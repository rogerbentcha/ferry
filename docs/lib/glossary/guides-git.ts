import type { GlossaryEntry } from './types';

/** Terms of this section. An id must not exist in another file of this folder. */
export const terms = {
  // --- Git accounts --------------------------------------------------------
  'git-connection': {
    term: 'git connection',
    definition: 'A GitHub or GitLab account that gave your Ferry server the permission to read its repositories.',
    see: ['repository'],
  },
  'github-app': {
    term: 'GitHub App',
    definition: 'An application on GitHub that belongs to your account. Ferry creates one for your server. It reads only the repositories that you choose.',
    see: ['git-connection'],
  },
  'oauth-app': {
    term: 'OAuth application',
    definition: 'An application that you create on GitLab for your server. After you authorize it, Ferry can read your repositories. Ferry never gets your password.',
    see: ['redirect-uri'],
  },
  'redirect-uri': {
    term: 'redirect URI',
    definition: 'The address that the provider sends your browser back to after you authorize.',
  },

  // --- Auto-deploy ---------------------------------------------------------
  'git-push': {
    term: 'push',
    definition: 'The git command that sends your commits to the repository on GitHub or GitLab.',
    see: ['commit'],
  },
  'webhook-secret': {
    term: 'webhook secret',
    definition: 'A secret text that GitHub and `ferryd` share. GitHub signs each webhook with it.',
    see: ['webhook'],
  },
  ci: {
    term: 'CI',
    definition: 'Continuous integration: a system that runs your tests after each push. GitHub Actions is a CI system.',
  },

  // --- Languages and Postgres ----------------------------------------------
  binary: {
    term: 'binary',
    definition: 'The program file that a compiler makes from Go or Rust code. It runs without the compiler.',
  },
  migration: {
    term: 'migration',
    definition: 'A script that changes the structure of a database, for example to create a table.',
  },
} satisfies Record<string, GlossaryEntry>;
