---
name: refine-linear-issue
description: Reviews a Linear issue with the human and rewrites it in a product owner's voice, using Feature Injection (who cares if we don't do this, what it is worth to them, examples of done), so an AI dev agent can implement it with /implement-linear-issue. Finds the missing information, asks the human only what they alone can answer, proposes a split when the issue is too big, and updates Linear once the human approves the draft. Use when the user wants to review, refine, rewrite, clarify or complete an issue, prepare it for an agent, or says "update this issue for missing information", "make this issue ready", "améliore / complète cette issue".
argument-hint: <linear-issue-url>
---

# Refine a Linear issue with the human

Issue: $ARGUMENTS

Turn a rough issue into one a person reads in a minute and an AI agent can
implement without guessing. You draft; the human decides. Nothing is written
to Linear before the human approves the draft.

## Ground rules

- **The human owns the why.** Never invent a stakeholder, a benefit, a number
  or a decision. If it is not in the issue, the code or the conversation, ask.
- **Talk caveman.** Load the `caveman` skill
  ([../caveman/SKILL.md](../caveman/SKILL.md)) before your first message and
  follow it for every message to the human: answer first, no fluff, every
  fact kept, a recommended answer first. Questions and options stay full
  sentences, as caveman says.
- **Write the issue in a product owner's voice, compressed with caveman**:
  see [TEMPLATE.md](TEMPLATE.md). Here caveman applies to the issue too,
  even though caveman alone keeps text saved outside the chat in plain
  prose.
- Keep your own context small: send code exploration to an Explore agent and
  keep only its conclusions.

## 1. Read

1. Get the issue with the Linear tools: description, comments, labels,
   parent, sub-issues, relations, attachments. No issue given → ask for it
   and stop.
2. Note the decisions already made (description, comments) so you don't ask
   them again.
3. Ask an Explore agent what exists today in the area: what users can already
   do, the types, routes and pages involved, and the DESIGN.md rules that
   apply (frozen signatures in `crates/*/src/lib.rs`, the crate dependency
   table, API and web conventions).

## 2. Review

Check the issue against each question and note every gap:

| Question | Gap when… |
|---|---|
| Who cares if we don't do this? | no stakeholder, or the stakeholder is "the developer" |
| What is it worth to them? | no benefit, or the benefit just restates the feature |
| What will they see when it's done? | no concrete example |
| How do we know it's done? | criteria nobody can test ("works well", "fast") |
| What is not included? | no boundary |
| Which words are ambiguous? | a term two people would read differently |
| Is it one piece of work? | it holds several things a stakeholder could use on their own |
| Does it fit the code and DESIGN.md? | it changes a frozen signature, adds a service to install, or breaks a design rule |

Technical work that nobody outside the team cares about is not a story:
propose folding it into the story that needs it, or dropping it.

## 3. Ask

1. Tell the human in a few lines: what is clear, what is missing, and your
   recommendation (keep as is, split, fold into another issue, drop).
2. Ask what only they can answer, at most 4 questions at a time with
   AskUserQuestion, recommended option first. Usual ones: who the stakeholder
   is and what they gain; what an ambiguous term means; limits and numbers
   ("how long to keep it", "how fast is fast enough"); whether to split;
   whether agents may add new public items to frozen crates.
3. Fill everything else from the code and sensible defaults, and list those
   defaults in the draft so the human can overrule them.

## 4. Draft and review together

1. Write the new description with [TEMPLATE.md](TEMPLATE.md). Too big →
   also draft sub-issues: thin vertical slices, each one giving the
   stakeholder something they can use, with their order.
2. Show the human, for each issue: the story line, the examples, what's not
   included and the defaults you picked. Show the developer notes only if
   they ask.
3. Change it until the human approves. Their answers go under **Decisions**
   with the date.

## 5. Update Linear

Only after approval:

1. Create the sub-issues first (same team, project, labels, priority; parent
   set; "blocked by" relations for the order), then update the parent so it
   can link to them.
2. Replace the description. Keep the original text at the bottom under
   **Original request**.
3. Leave status, assignee and priority alone unless the human asks.

## 6. Report

A link to every issue touched with one line each, and the defaults the human
has not confirmed yet.
