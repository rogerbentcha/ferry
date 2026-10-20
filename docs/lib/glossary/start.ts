import type { GlossaryEntry } from './types';

/** Terms of this section. An id must not exist in another file of this folder. */
export const terms = {
  // --- Render --------------------------------------------------------------
  render: {
    term: 'Render',
    definition: 'A company that runs apps on its own servers. Ferry does the same work on a server that you control.',
  },
  'self-hosted': {
    term: 'self-hosted',
    definition: 'You run the program on a machine that you control, not on the machines of a company.',
  },
  'secret-file': {
    term: 'secret file',
    definition: 'On Render, a file with secret text that the host puts in the container of a service.',
  },

  // --- Install with one command ---------------------------------------------
  'install-script': {
    term: 'install script',
    definition: 'The file install.sh of each release. With one command, it installs Ferry on a server or upgrades it.',
  },
  'x86-64': {
    term: 'x86_64',
    definition: 'The type of CPU in most PCs and servers, from Intel and AMD. It is also called amd64.',
    see: ['arm64'],
  },
  arm64: {
    term: 'arm64',
    definition: 'A type of CPU. Many low-cost cloud servers and the newer Raspberry Pi boards use it. On Linux, `uname -m` calls it `aarch64`.',
    see: ['x86-64'],
  },

  // --- Download Ferry ------------------------------------------------------
  checksum: {
    term: 'checksum',
    definition: 'A short value calculated from a file. When one byte of the file changes, the checksum changes.',
    see: ['sha-256'],
  },
  glibc: {
    term: 'glibc',
    definition: 'The C library of most Linux systems. A program built with one version runs with that version or a newer one.',
  },

  // --- Install the CLI on a Mac --------------------------------------------
  'apple-silicon': {
    term: 'Apple Silicon',
    definition: 'The arm64 CPUs of Apple (M1 and newer), in the Macs since 2020. On a Mac, `uname -m` calls them `arm64`.',
    see: ['arm64'],
  },
  gatekeeper: {
    term: 'Gatekeeper',
    definition: 'The part of macOS that checks a program from the internet before it opens. It can block a program with no Apple signature.',
  },

  // --- Build Ferry ---------------------------------------------------------
  cargo: {
    term: 'cargo',
    definition: 'The build tool of Rust. It makes the Ferry programs from the source code.',
  },
  npm: {
    term: 'npm',
    definition: 'The package tool of Node.js. It gets the libraries of a project and runs its build.',
  },
  'path-variable': {
    term: 'PATH',
    definition: 'The list of folders where the terminal looks for a program when you type its name.',
  },

  // --- Docker on your machine ----------------------------------------------
  'docker-daemon': {
    term: 'Docker daemon',
    definition: 'The part of Docker that stays in the background. It builds images and runs containers when a program asks.',
    see: ['docker-socket'],
  },
  'docker-socket': {
    term: 'Docker socket',
    definition: 'A special file on the machine. Programs talk to the Docker daemon through it.',
  },
  'docker-context': {
    term: 'Docker context',
    definition: 'A name in Docker for one daemon and its address. Colima and OrbStack each make a context.',
  },
  'docker-desktop': {
    term: 'Docker Desktop',
    definition: 'The Docker app for macOS and Windows. It runs the containers in a virtual machine.',
    see: ['virtual-machine'],
  },
  'virtual-machine': {
    term: 'virtual machine',
    definition: 'A computer that software makes inside a real computer. It has its own system, CPUs and memory.',
  },

  // --- The server ----------------------------------------------------------
  'data-directory': {
    term: 'data directory',
    definition: 'The folder where `ferryd` keeps its state: the database, the logs, the uploads and the certificates. The default is `./ferry-data`.',
  },
  'admin-account': {
    term: 'account',
    definition: 'The one administrator of a Ferry server: an email and a password. You sign in to the dashboard with it.',
  },
  'setup-code': {
    term: 'setup code',
    definition: 'A secret code that `ferryd` makes while the server has no account. You need it to create the account.',
  },
  'server-token': {
    term: 'server token',
    definition: 'An API token that `ferryd` writes in its data directory at the first start. Scripts on the server use it.',
    see: ['api-token'],
  },
  background: {
    term: 'background',
    definition: 'A program in the background does not keep the terminal. You can close the terminal and the program continues.',
    see: ['foreground'],
  },
  foreground: {
    term: 'foreground',
    definition: 'A program in the foreground keeps the terminal and writes its output there until it stops.',
    see: ['background'],
  },
  'service-manager': {
    term: 'service manager',
    definition: 'A system program that starts other programs and starts them again after a crash: systemd on Linux, launchd on macOS.',
    see: ['systemd'],
  },
  'liveness-probe': {
    term: 'liveness probe',
    definition: 'A small request that tells you if a program is alive.',
  },
  'swagger-ui': {
    term: 'Swagger UI',
    definition: 'A web page that lists each endpoint of an API. You can try the endpoints from the page.',
  },
} satisfies Record<string, GlossaryEntry>;
