import type { GlossaryEntry } from './types';

/** Terms of this section. An id must not exist in another file of this folder. */
export const terms = {
  // --- Linux ---------------------------------------------------------------
  root: {
    term: 'root',
    definition: 'The administrator account of a Linux machine. It can read, change and delete all things.',
    see: ['sudo'],
  },
  sudo: {
    term: 'sudo',
    definition: 'A Linux command that runs one other command as root.',
    see: ['root'],
  },
  'system-user': {
    term: 'system user',
    definition: 'A Linux account for a program, not for a person. No person can log in with it.',
  },

  // --- systemd -------------------------------------------------------------
  'systemd-unit': {
    term: 'unit',
    definition: 'A file that tells systemd how to start, stop and supervise one program.',
    see: ['systemd'],
  },
  journal: {
    term: 'journal',
    definition: 'The place where systemd keeps the output of the programs that it runs. The command `journalctl` reads it.',
    see: ['systemd'],
  },

  // --- Network -------------------------------------------------------------
  'reverse-proxy': {
    term: 'reverse proxy',
    definition: 'A program that receives the requests from the internet and sends them to another program. nginx, Caddy and Traefik are reverse proxies.',
    see: ['proxy'],
  },

  // --- Ferry servers -------------------------------------------------------
  'docker-root': {
    term: 'Docker root directory',
    definition: 'The folder where Docker keeps its images, containers and volumes.',
  },
  'marker-volume': {
    term: 'marker volume',
    definition: 'The Docker volume `<prefix>-owner`. It records which data directory owns the name prefix.',
    see: ['instance-id'],
  },
  'instance-id': {
    term: 'instance_id',
    definition: 'The file of the data directory that holds the identity of one Ferry server.',
  },
  'data-dir-lock': {
    term: 'lock',
    definition: 'A mark on a file that one program holds. It tells other programs that the folder is in use.',
  },
  staging: {
    term: 'staging',
    definition: 'A second copy of your setup where you test a change before it goes to production.',
  },
} satisfies Record<string, GlossaryEntry>;
