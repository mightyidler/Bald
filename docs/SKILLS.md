# Project skill setup

Project skills are installed in `.agents/skills/` and activated by `AGENTS.md`. Exact upstream revisions are in `.agents/skills/sources.json` and `skills-lock.json`. Attention-kind is the default communication style.

Anti-slop lives in `tools/oxlint/anti-slop/`. Oxlint and `@oxlint/plugins` are pinned to `1.86.0`. All 18 generic rules and the native accumulating-spread companion are errors. Run `npm run lint` for web JavaScript and the three root build/release scripts.

No JavaScript/TypeScript typecheck command is configured. Rust compilation and runtime behavior require separate checks.
