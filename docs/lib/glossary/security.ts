import type { GlossaryEntry } from './types';

/** Terms of this section. An id must not exist in another file of this folder. */
export const terms = {
  // --- Who you are ---------------------------------------------------------
  'session-cookie': {
    term: 'session cookie',
    definition: 'The cookie that proves that a browser signed in to the dashboard. Its name is `ferry_session_<port>`.',
  },
  origin: {
    term: 'origin',
    definition: 'The scheme, the host and the port of a web page, for example `https://ferry.example.com`. A browser keeps pages of different origins apart.',
  },
  cors: {
    term: 'CORS',
    definition: 'Cross-origin resource sharing: headers that let a page of another origin call an API from a browser. Ferry sends none.',
    see: ['origin'],
  },
  endpoint: {
    term: 'endpoint',
    definition: 'One address of an API, for example `/healthz`.',
    see: ['api'],
  },
  'account-endpoint': {
    term: 'account endpoint',
    definition: 'An API address below `/api/v1/auth`. These addresses control the password, the sessions and the API tokens.',
    see: ['endpoint'],
  },
  'query-string': {
    term: 'query string',
    definition: 'The part of a URL after the `?`, for example `?key=abc`.',
  },

  // --- Hashes and signatures ----------------------------------------------
  hex: {
    term: 'hex character',
    definition: 'One of the 16 characters `0` to `9` and `a` to `f`. Programs write random values and hashes with them.',
  },
  'sha-256': {
    term: 'SHA-256',
    definition: 'A function that calculates a fixed value (a hash) from a text. Nobody can get the text back from the hash.',
    see: ['argon2id'],
  },
  argon2id: {
    term: 'Argon2id',
    definition: 'A hash function for passwords. It is slow on purpose, so an attacker cannot try many passwords quickly.',
    see: ['sha-256'],
  },
  hmac: {
    term: 'HMAC',
    definition: 'A signature that the sender calculates from a message and a shared secret. The receiver calculates it again and compares.',
    see: ['webhook'],
  },
  'private-key': {
    term: 'private key',
    definition: 'The secret half of a pair of keys. The program that has it can prove its identity or read encrypted data.',
  },

  // --- Where data goes -----------------------------------------------------
  'clear-text': {
    term: 'clear text',
    definition: 'Data with no encryption. Each machine on the network path can read it.',
    see: ['https'],
  },
  'dashboard-host': {
    term: 'dashboard host',
    definition: 'The hostname on which the proxy serves the dashboard and the API. It comes from `ferryd --dashboard-host`.',
  },

  // --- Limits --------------------------------------------------------------
  quota: {
    term: 'quota',
    definition: 'A limit on the disk space that one container or one volume can use. Ferry sets none.',
  },
} satisfies Record<string, GlossaryEntry>;
