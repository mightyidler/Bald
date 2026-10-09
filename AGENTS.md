# Project skills

Use the project-local skills below for work in this repository. Read the relevant
`SKILL.md` before applying it. Source repositories and pinned commits are recorded
in `.agents/skills/sources.json` and `skills-lock.json`.

- **Attention-kind**: Read `.agents/skills/attention-kind/SKILL.md` at the start of
  a conversation and use it as the default response style. Respond in Korean
  unless the user requests another language. Lead with the answer, keep points
  concise, and use spaced paragraphs with bold lead-ins and arrows. Preserve
  correctness-critical details. This style governs communication, not work scope.
- **Karpathy guidelines**: Read `.agents/skills/karpathy-guidelines/SKILL.md` before
  writing, reviewing, or refactoring code. Surface material assumptions, prefer
  simple solutions, make focused changes, and define verifiable success criteria.
- **Anti-slop**: Read `.agents/skills/install-anti-slop/SKILL.md` when installing,
  updating, or configuring the vendored rules. For JavaScript and TypeScript
  changes, use the rules in `.oxlintrc.json` and run `npm run lint`. Keep existing
  findings visible; do not weaken rules or perform unrelated cleanup to pass lint.
  Plugin source and provenance live in `tools/oxlint/anti-slop/`.
- **Verification before completion**: Read
  `.agents/skills/verification-before-completion/SKILL.md` before claiming work
  is complete, fixed, or passing. Run fresh checks appropriate to the changed
  behavior, inspect their exit codes and output, and report any remaining failures.
  Lint does not prove that a Rust build or runtime behavior works.

Continue using the existing `transitions-dev` and `transitions-polish` skills for
relevant motion work. Preserve unrelated user changes. User instructions and
higher-priority agent instructions take precedence over skill guidance.
