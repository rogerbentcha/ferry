# Issue voice and template

## Voice

Write like a product owner talking to the team, not like a spec.

- **Person and outcome first, feature second.** "I find out my site is slow
  before my users do" comes before "a CPU chart".
- **Plain words, short sentences.** Explain a technical term the first time,
  or move it to the developer notes.
- **Caveman-compressed.** Follow rules 1–6 of the `caveman` skill
  ([../caveman/SKILL.md](../caveman/SKILL.md)) on every line: no ceremony,
  no hedging, articles optional, one idea per sentence. Names, numbers,
  types, routes and commands stay exact. Never drop not, never, no or only.
  The story line and the Given/when/then examples keep their full shape.
- **Give the reason for every limit.** "24 hours: enough to look into last
  night's problem, small enough to keep Ferry light."
- **Leave the how open** in Why, Examples and the stakeholder's acceptance
  criteria. The developer notes and the "for the agent" criterion carry the
  technical detail.
- **Durable for the agent.** Name types, routes, commands and DESIGN.md
  sections. Never file paths or line numbers: they go stale.

## The story line (Feature Injection)

Ask "Who cares if we don't do this?" until you reach a real person with a
real goal. Then write:

> **In order to** <what the stakeholder gains>, **as** <the stakeholder>,
> **I want** <what the system or someone else does>.

- **The stakeholder isn't always the user.** A captcha isn't for the visitor
  who types it, but for the team whose ads are worthless if bots spam the
  site: "In order to keep bots from making our ads worthless, as the
  commercial team, I want visitors to fill in a captcha."
- **A technical need belongs to whoever pays when it's missing.** "In order
  to stop losing money on code that breaks in production, as the budget
  owner, I want the team to test every change against a production-like
  environment." How (an automated build, a manual run) stays open.
- **Ask the stakeholder for the numbers that mean "good enough".** For
  example: 90% of pages in under a second, or survive the Christmas peak.
  These numbers become acceptance criteria and natural split points (normal
  load first, peak load later).

## Template

```markdown
## Why

**In order to** …, **as** …, **I want** ….

<2–4 sentences in the stakeholder's words: how it is today, and what goes
wrong because of it. A sub-issue starts with "Part of LAB-n.">

## What it looks like when it's done

- **<Short name>.** Given …, when …, then ….
- <3 to 6 examples the stakeholder would recognise, including one edge case
  or failure.>

## Acceptance criteria

- [ ] <Something the stakeholder can see or check.>
- [ ] …
- [ ] For the agent: <routes in OpenAPI and the routes test, the tests to
      write, docs to update, every CI check passes.>

## Not in this issue

- <Thing> (<where it lives instead, if anywhere>).

## Decisions

- <Decision>. (<who>, <date>)

## Notes for the developer

- **Today:** what exists (types, routes, pages) and what is missing.
- **Suggested shape:** DTO, route and trait method names, as suggestions;
  follow the code's conventions.
- **Rules:** the DESIGN.md constraints that apply, and whether adding public
  items to frozen crates is approved.
- **Order:** blocked by / blocks.

## Original request

<The text as first written.>

*Drafted with AI during a review with the product owner.*
```

A **parent issue** adds **The pieces** after Why: a table of its sub-issues
in order, each with what the stakeholder gets from it. Its acceptance
criteria are about the whole outcome, and the rules shared by every
sub-issue go in its developer notes. Sub-issues then say "Rules: see LAB-n".

## Example: the story line

Before: "Add a page to monitor server metrics: cpu, request count, latency,
network, RAM, disk read/write, container cpu, server logs."

After: "**In order to** spot and fix problems on my server before the people
using my apps notice them, **as** the person who runs a Ferry server, **I
want** to see how the machine and every app on it are doing, and how that
changed over the last day, right in the dashboard."
