import type { GlossaryEntry } from './types';

/** Terms of this section. An id must not exist in another file of this folder. */
export const terms = {
  // --- The blueprint file --------------------------------------------------
  yaml: {
    term: 'YAML',
    definition: 'A text format for settings. Each line is `key: value`. The indentation shows what is inside what.',
    see: ['blueprint'],
  },
  'yaml-anchor': {
    term: 'YAML anchor',
    definition: 'A name for a block of YAML, written `&defaults`. `<<: *defaults` copies the block to another place of the file.',
    see: ['yaml'],
  },
  apply: {
    term: 'apply',
    definition: 'The operation that reads a blueprint, then creates what is not there and updates what changed.',
    see: ['blueprint', 'dry-run'],
  },
  'ferry-extension': {
    term: 'Ferry key',
    definition: 'A blueprint key that Ferry adds to the `render.yaml` format: `port`, `memoryLimit` and `cpuLimit`. Render has no such key.',
    see: ['blueprint'],
  },
  'generated-secret': {
    term: 'generated secret',
    definition: 'A random value of 64 hex characters (256 bits). Ferry makes it for a variable that has `generateValue: true`.',
  },
  'camel-case': {
    term: 'camelCase',
    definition: 'A way to write a name of many words with no space. Each word after the first starts with a capital letter: `healthCheckPath`.',
    see: ['snake-case'],
  },
  // --- The API -------------------------------------------------------------
  'rest-api': {
    term: 'REST API',
    definition: 'An API that uses plain HTTP requests. The URL names a resource. The method (`GET`, `POST`, `DELETE`) names the action.',
    see: ['api'],
  },
  'base-url': {
    term: 'base URL',
    definition: 'The start of each URL of the API: the address of the server, then `/api/v1`.',
  },
  curl: {
    term: 'curl',
    definition: 'A command that sends one HTTP request and prints the answer.',
  },
  'bearer-token': {
    term: 'bearer token',
    definition: 'A token that a client sends in the header `Authorization: Bearer <token>`.',
    see: ['api-token'],
  },
  'request-body': {
    term: 'body',
    definition: 'The data of an HTTP request or answer. It comes after the headers.',
  },
  'snake-case': {
    term: 'snake_case',
    definition: 'A way to write a name of many words: lower case, with `_` between the words, for example `base_domain`.',
    see: ['camel-case'],
  },
  'rfc-3339': {
    term: 'RFC 3339',
    definition: 'A standard text format for a date and a time, for example `2026-09-27T07:38:34.231568Z`.',
    see: ['timestamp'],
  },
  pagination: {
    term: 'pagination',
    definition: 'An API with pagination gives a long list in parts, one page for each request. The Ferry API gives the full list.',
  },
  'event-source': {
    term: 'EventSource',
    definition: 'The object of a browser that reads a Server-Sent Events stream. It connects again by itself. It cannot set headers.',
    see: ['sse'],
  },
  'resource-id': {
    term: 'id',
    definition: 'The identifier that Ferry gives to a resource: a prefix and 20 hex characters, for example `srv-60f5973b037e4478ae7b`.',
  },
  delivery: {
    term: 'delivery',
    definition: 'One webhook request from GitHub. The header `X-GitHub-Delivery` gives its id.',
    see: ['webhook'],
  },
  polling: {
    term: 'polling',
    definition: 'To ask the server again at a fixed interval, to see if something changed.',
  },
} satisfies Record<string, GlossaryEntry>;
