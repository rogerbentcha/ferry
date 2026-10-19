import type { GlossaryEntry } from './types';

/** Terms of this section. An id must not exist in another file of this folder. */
export const terms = {
  // --- Services ------------------------------------------------------------
  source: {
    term: 'source',
    definition: 'Where the code of a service comes from: a git repository, a Docker image or an upload.',
    see: ['repository', 'image'],
  },
  'service-type': {
    term: 'service type',
    definition: 'The kind of a service: web service, private service, background worker, cron job or static site.',
    see: ['service'],
  },
  nginx: {
    term: 'nginx',
    definition: 'A web server program. Ferry uses it to send the files of a static site to the browser.',
    see: ['static-site'],
  },
  archive: {
    term: 'archive',
    definition: 'One file that contains many files and folders. A `.tar.gz` archive is also compressed.',
  },
  symlink: {
    term: 'symlink',
    definition: 'A file that points to another file or folder.',
  },
  submodule: {
    term: 'git submodule',
    definition: 'A git repository that is a folder of another git repository.',
    see: ['repository'],
  },
  'auto-deploy': {
    term: 'auto-deploy',
    definition: 'A setting of a service. When it is on, a push to the branch starts a deploy.',
    see: ['webhook'],
  },
  'image-tag': {
    term: 'tag',
    definition: 'The part of an image name after the `:`. It names one version of the image, for example `alpine` in `nginx:alpine`.',
    see: ['image'],
  },
  'pinned-image': {
    term: 'pinned image',
    definition: 'An image that Ferry keeps with a tag of its own for one deploy. This deploy always runs the same image.',
    see: ['image-tag'],
  },
  'cron-schedule': {
    term: 'schedule',
    definition: 'The times at which a cron job runs. You write it as a cron expression, for example `0 3 * * *`.',
    see: ['cron-job', 'cron-expression'],
  },
  utc: {
    term: 'UTC',
    definition: 'Coordinated Universal Time: the reference time of the world. It has no summer time.',
  },

  // --- Builds --------------------------------------------------------------
  monorepo: {
    term: 'monorepo',
    definition: 'One repository that contains many apps, each in its own folder.',
    see: ['root-directory'],
  },
  'root-directory': {
    term: 'root directory',
    definition: 'The folder of your code where Ferry builds the service. The default is the top of the repository.',
    see: ['monorepo'],
  },
  'start-command': {
    term: 'start command',
    definition: 'The command that starts your app in each container, for example `npm start`.',
    see: ['procfile'],
  },
  procfile: {
    term: 'Procfile',
    definition: 'A text file named `Procfile`. Each line has a name and a start command, for example `web: python app.py`.',
    see: ['start-command'],
  },
  'base-image': {
    term: 'base image',
    definition: 'The image that a Dockerfile starts from. It contains a small system and the tools of one language.',
    see: ['dockerfile'],
  },
  'build-cache': {
    term: 'build cache',
    definition: 'The layers that Docker keeps from earlier builds. Docker uses a layer again when its step did not change.',
  },
  'build-secret': {
    term: 'build secret',
    definition: 'A value that Docker gives to one build step only. Docker does not record it in the image.',
  },
  'image-history': {
    term: 'image history',
    definition: 'The list of the steps that made an image. Each person who has the image can read it.',
    see: ['image'],
  },
} satisfies Record<string, GlossaryEntry>;
