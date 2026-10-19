# Authoring guide

How to write a page of the Ferry docs. The goal: a reader who is new to servers and Docker, and whose first language is possibly not English, finds the task and does it with one page.

The model to copy: `content/docs/guides/deploy.mdx`.

```bash
npm run check-content -- -v content/docs/guides/deploy.mdx   # measures the rules below
```

## 1. The page

One page is one task or one subject, from start to end: "Deploy an app", "Domains and HTTPS", "Troubleshooting". The reader does not jump between pages to finish a task.

| Rule | Limit |
|---|---|
| One task per page | The title names it. A related task with its own steps is a second page. |
| Essential only | Aim for 400 to 1,200 words of sentences. 1,500 at most (the checker counts). |
| Start with the task | After one or two sentences, show the main command or the main steps. |
| Tables | 1,200 words at most. A table replaces sentences that compare things or list values. |
| Details | Rare cases go in a closed `<Accordions>` at the end of a section: 400 words at most for the page. |
| Terms | The first time a technical name shows on a page, write `<Term id="…" />`. |
| Headings | `##` for each part of the task, 3 to 10 per page. `###` under a `##` when it helps. |
| Figures | Most pages have none. One `<Diagram>`, `<Flow>` or mermaid chart at most, and only when a flow is hard to explain in words. |

A page looks like this:

```mdx
---
title: Domains and HTTPS
description: Give a service a URL, add your own domain and get a certificate.
---

One or two short paragraphs: what the reader gets, and what they need first.

## Add a domain           ← a part of the task: the command first, then the text

```bash title="Terminal"
ferry domain add api api.example.com
```

## Get a certificate

…

<Accordions>             ← rare cases, closed
  <Accordion title="If the verification fails">…</Accordion>
</Accordions>
```

- `description` is one sentence of 20 words at most. It says what the page helps to do. Plain text, no Markdown.
- Do not write the title again as a `#` heading.
- Do not end a page with "Next steps" cards: the page has previous / next links. Only the home page has cards.
- When the same task has a CLI, a dashboard and an API version, use `<Tabs>`. Do not write three sections.

## 2. Files and sidebar

Each top-level folder of `content/docs/` is a section of the sidebar: Get started, Guides, Developers, Platform, Help. A page is one file.

```text
guides/
├── meta.json        { "title": "Guides", "icon": "BookOpen", "pages": ["deploy", "github-auto-deploy", …] }
├── deploy.mdx       one task
└── domains.mdx      one task
```

- `pages` in `meta.json` gives the order of the sidebar: from what a new reader needs first to what is rare.
- A file name is one or two words (`domains.mdx`). The title can be longer.
- Links are absolute: `/docs/guides/domains#add-a-domain`. `npm run build` fails on a broken link or anchor.
- Add a page only for a new task. A new option or a new case of a task goes in the page of that task.

## 3. Simplified Technical English

The text follows the writing rules of ASD-STE100. The target is 80% of the sentences of each page with no fault (the checker reports the percentage and each fault).

| Rule | Not this | This |
|---|---|---|
| A description has 25 words at most | | |
| An instruction has 20 words at most and starts with the verb | You can run the command to deploy. | Run the command. |
| One instruction per sentence | Open the page and click **Deploy**. | Open the page. Click **Deploy**. |
| Active voice | The image is built by Ferry. | Ferry builds the image. |
| Simple tenses: present, past, future | The deploy has been started. | The deploy started. |
| No `-ing` verb form | Ferry waits for running requests. | Ferry waits for the requests in progress. |
| One word, one meaning. One thing, one name | instance / replica / copy | instance |
| Full words | don't, it's, e.g., etc. | do not, it is, for example |
| No idiom, no filler | under the hood, simply, just | (say the fact) |
| The condition comes first | Restart the service if the value changed. | If the value changed, restart the service. |
| A paragraph has 6 sentences at most | | |
| A noun group has 3 words at most | the deploy health check timeout setting | the timeout of the health check |
| No `;` between two ideas | Ferry stops it; the proxy forgets it. | Ferry stops it. The proxy forgets it. |

Words to replace:

| Not this | This |
|---|---|
| may, might | can |
| should | must, or the instruction |
| once, upon | when, after (at once → immediately) |
| since | because |
| via | through, with |
| however | but |
| whether | if |
| ensure, verify | make sure |
| perform | do |
| in order to | to |
| as well as | and |
| additional | more |
| obtain, retrieve | get |
| terminate | stop |
| utilize, leverage | use |

Names of things are free: `deploy`, `instance`, `container`, `scaling`, `load balancing`, and each term of the glossary. Text in `code`, in commands and in real output stays as the program writes it.

## 4. Facts

- Keep what a reader needs to do the task and to fix the common problems. Leave out internal details: they are in the code and in `DESIGN.md`.
- Do not invent. A command, a flag, an output, a number, a file name: copy it from the code or from the CLI help. If you are not sure, read the code (`../crates`, `../web`).
- Real output stays word for word. You can remove lines, not change them.
- The generated pages (`reference/cli`, `reference/server-options`, `reference/api/*`) are not written by hand: change the Rust doc comments.

## 5. Components

A figure is rare: most pages are text, commands and tables. When a flow is hard to explain in words, one `<Diagram>`, `<Flow>` or mermaid chart can show it.

All components are available in every page with no import. Their data is plain values (strings, numbers, arrays): no JSX in a prop.

### `<Diagram>`: things and what goes between them

Boxes on a grid, arrows, dots that move along the arrows that carry something. With `steps`, the picture changes in a loop and each step has one sentence.

```mdx
<Diagram
  nodes={[
    { id: 'browser', at: [0, 0.5], icon: 'browser', label: 'Browser' },
    { id: 'proxy', at: [1.25, 0.5], icon: 'route', label: 'Proxy' },
    { id: 'old', at: [2.5, 0], icon: 'box', label: 'Old instance', note: 'version 1' },
    { id: 'new', at: [2.5, 1], icon: 'box', label: 'New instance', note: 'version 2' },
  ]}
  edges={[
    { from: 'browser', to: 'proxy' },
    { from: 'proxy', to: 'old' },
    { from: 'proxy', to: 'new' },
  ]}
  steps={[
    { caption: 'The old instance answers the requests.', hide: ['new'], flow: ['browser>proxy', 'proxy>old'] },
    { caption: 'The proxy sends the traffic to the new instance.', cut: ['proxy>old'], flow: ['browser>proxy', 'proxy>new'], tones: { new: 'success' }, notes: { new: 'live' } },
  ]}
/>
```

| Prop | Value |
|---|---|
| `nodes[].at` | `[column, row]` from 0. Halves are fine (`[1.25, 0.5]`). Column 3.5 at most; 3 rows read best. |
| `nodes[].label` | 15 characters at most with an icon, 20 without. |
| `nodes[].note` | A second, smaller line: 18 characters at most. |
| `nodes[].icon` | A name from `components/mdx/icons.ts` (`box`, `server`, `database`, `globe`, `route`, `key`, `disk`, `terminal`, `branch`, `hammer`, `health`, …). |
| `nodes[].tone`, `edges[].tone` | `default`, `primary`, `success`, `warning`, `danger`, `muted`. |
| `edges[]` | `from`, `to`, and optional `label` (16 characters), `flow: true` (dots move), `dashed`, `both` (two arrow heads), `id` (default `from>to`). |
| `groups[]` | `{ label, around: ['proxy', 'old', 'new'] }`: a dashed box around the named nodes, for example "Your server". (`from: [col, row], to: [col, row]` also works: a box around those cells.) Keep 14px free around the box: no other node next to it in the same half column. |
| `steps[]` | 2 to 6 steps. `caption` (one sentence), `hide` (node ids not there), `flow` (edge ids that carry something), `cut` (edge ids not there), `tones` and `notes` (per node id, for this step). |
| `caption` | A title under a diagram with no steps. |

Without `steps`, an edge moves when it has `flow: true`. With `steps`, only the edges in the `flow` of the step move.

### `<Flow>`: what happens, in order

2 to 7 steps in a row. One step is lit at a time and its sentence shows below.

```mdx
<Flow
  steps={[
    { icon: 'queue', title: 'Queue', text: 'The deploy waits for its turn.' },
    { icon: 'hammer', title: 'Build', text: 'Ferry gets your code and makes an image.' },
    { icon: 'health', title: 'Health check', text: 'Each new instance must pass a test.', tone: 'warning' },
  ]}
/>
```

`title`: 18 characters at most. `text`: one or two short sentences. `tone`: `warning` for the step that can fail, `success` for the good end, `danger` for a dead end.

### `<Terminal>`: a session that plays

The commands are typed, then the output shows line by line. Use it only in the quickstart, for the first deploy and its real output (16 lines at most). Everywhere else, use a `bash` code block for the command and a `text` code block with `noCopy` for the output.

```mdx
<Terminal
  title="Terminal"
  lines={[
    '$ ferry up hello --follow',
    '==> Build successful 🎉',
    '==> Your service is live 🎉',
    '# a line that starts with # is a remark',
  ]}
/>
```

A line that starts with `$ ` is a command. In a string, write `\\` for a backslash and `\'` for a quote, or use double quotes around the string.

### `<Term>`: a name with its definition

```mdx
A <Term id="deploy" /> is one release of a <Term id="service" />.
Ferry starts new <Term id="container">containers</Term>.
```

The terms are in `lib/glossary/`: `core.ts` and one file per section. To add a term, add it to the file of your section:

```ts
'blue-green': {
  term: 'blue-green deploy',
  definition: 'A deploy that starts the new version next to the old version, then moves the traffic.',
  see: ['deploy'],
},
```

A definition is one or two sentences, 30 words at most, in the same simple English. An id exists in one file only (the checker tells you).

### Other components

| Component | Use |
|---|---|
| ` ```mermaid ` | State machines and sequences between 3 or more actors. `flowchart TB` if more than 4 nodes. For boxes and arrows, prefer `<Diagram>` (it moves, and it has the Ferry look). |
| ` ```bash title="Terminal" ` | Commands to copy. `noCopy` for output. |
| Table | To compare, or to list values. Short cells. |
| `<Steps>` / `<Step>` | A procedure that the reader does. Each step starts with a `###` heading and has one action. |
| `<Tabs>` / `<Tab>` | The same task with the CLI, the dashboard or the API. `groupId="interface" persist`. |
| `<Callout type="warning" title="…">` | One per page at most. For a risk, or for the one thing to remember. |
| `<Accordions>` / `<Accordion title="…">` | Rare cases, at the end of the page. |
| `<Cards>` / `<Card title href description>` | Only on the home page. |
| `<Files>` / `<Folder>` / `<File>` | A file tree. |

MDX traps: write `{'{'}` or use `code` for a brace in text, write `&lt;` or use `code` for `<`, and leave an empty line before and after a component that has Markdown inside.

## 6. Before you are done

```bash
npm run check-content -- -v content/docs/<your page>.mdx  # every page ✓
npm run typecheck                                         # after a change in lib/ or components/
npm run build                                             # the static export and the link check
```

The checker measures the sentence rules and a short list of words. It does not have the STE dictionary: read each page one more time as the reader.
