import type { GlossaryEntry } from './types';

/** Terms of this section. An id must not exist in another file of this folder. */
export const terms = {
  // --- Requests and the proxy ----------------------------------------------
  'ip-address': {
    term: 'IP address',
    definition: 'The number of a machine on a network, for example `203.0.113.10`. Machines use it to find each other.',
    see: ['dns'],
  },
  'public-url': {
    term: 'public URL',
    definition: 'The address that a browser on the internet uses to open your service, for example `https://shop.example.com`.',
    see: ['hostname'],
  },
  'host-header': {
    term: 'Host header',
    definition: 'The header of a request that gives the hostname that the client wants. The proxy reads it to find the service.',
    see: ['hostname'],
  },
  'status-code': {
    term: 'status code',
    definition: 'The number at the start of an HTTP answer. `200` is a success, `404` is "not found", `503` is "not available".',
  },
  redirect: {
    term: 'redirect',
    definition: 'An answer that tells the browser to go to another URL. Ferry uses the status code `308` to send HTTP to HTTPS.',
    see: ['status-code'],
  },
  'round-robin': {
    term: 'round-robin',
    definition: 'A rule of turns: the first request goes to the first instance, the next request to the next instance, then the list starts again.',
  },
  sse: {
    term: 'Server-Sent Events',
    definition: 'SSE: one HTTP answer that stays open. The app sends new lines of data to the browser when it has them.',
  },

  // --- The private network -------------------------------------------------
  'bridge-network': {
    term: 'bridge network',
    definition: 'A network that Docker makes inside one machine. The containers on it can call each other. Other machines cannot reach it.',
    see: ['private-network'],
  },
  alias: {
    term: 'alias',
    definition: 'A name that Docker gives to a container on a network. Other containers use this name, not an IP address.',
    see: ['private-network'],
  },
  environment: {
    term: 'environment',
    definition: 'A group of services and datastores with its own private network, for example `production` or `staging`. Each environment can use the same names. Names do not resolve from one environment to another.',
    see: ['private-network', 'staging'],
  },
  'ssh-tunnel': {
    term: 'SSH tunnel',
    definition: 'A safe connection through SSH that makes a port of the server available as a port of your machine.',
  },

  // --- Domains -------------------------------------------------------------
  'base-domain': {
    term: 'base domain',
    definition: 'The first domain of a Ferry server. It comes from `ferryd --base-domain`. Its default is `localhost`.',
    see: ['server-domain'],
  },
  'server-domain': {
    term: 'domain of the server',
    definition: 'A domain that Ferry serves all public services under. With `example.com`, a service named `shop` answers at `shop.example.com`.',
    see: ['base-domain', 'custom-domain'],
  },
  'default-domain': {
    term: 'default domain',
    definition: 'The domain of the server that Ferry uses when it shows the URL of a service.',
    see: ['server-domain'],
  },
  'custom-domain': {
    term: 'custom domain',
    definition: 'One more hostname that you add to one service, for example `www.example.com`.',
    see: ['server-domain'],
  },
  'wildcard-record': {
    term: 'wildcard DNS record',
    definition: 'A DNS record for `*.example.com`. It answers for each name under the domain, thus a new service needs no new record.',
  },
  'local-name': {
    term: 'local name',
    definition: 'A name that has no public DNS: `localhost`, `*.localhost`, `*.local`, `*.internal` and `*.test`. It gets no certificate.',
    see: ['localhost'],
  },

  // --- HTTPS ---------------------------------------------------------------
  'http-01': {
    term: 'HTTP-01 challenge',
    definition: 'The test of ACME: the certificate authority asks the host for a secret file on port 80. A correct answer proves control of the hostname.',
  },
  'self-signed': {
    term: 'self-signed certificate',
    definition: 'A certificate that a server makes for itself. No certificate authority signed it, thus browsers show a warning.',
    see: ['certificate'],
  },

  // --- Environment ---------------------------------------------------------
  'merged-env': {
    term: 'merged environment',
    definition: 'All the variables that a service gets: the injected variables, then its env groups, then its own variables.',
  },
  shell: {
    term: 'shell',
    definition: 'The program that reads the commands that you type in a terminal, for example bash or zsh.',
  },
  kib: {
    term: 'KiB',
    definition: 'Kibibyte: 1024 bytes. 32 KiB is 32,768 bytes.',
  },
  'connection-string': {
    term: 'connection string',
    definition: 'One URL that contains all that an app needs to connect to a database: user, password, host, port and database name.',
    see: ['datastore'],
  },
} satisfies Record<string, GlossaryEntry>;
