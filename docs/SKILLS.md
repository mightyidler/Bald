# Project skill setup

Four skills are installed in `.agents/skills/` and activated by `AGENTS.md`. Exact upstream revisions are in `.agents/skills/sources.json` and `skills-lock.json`. Attention-kind is the default communication style.

Anti-slop lives in `tools/oxlint/anti-slop/`. Oxlint and `@oxlint/plugins` are pinned to `1.86.0`. All 18 generic rules and the native accumulating-spread companion are errors. Run `npm run lint` for web JavaScript and the three root build/release scripts.

Initial verification on 2026-10-05: lint exited 1 with 135 `require-readable-spacing` errors:

- `build-signed.js`: 4 errors.
- `bump-and-push.js`: 22 errors.
- `publish-update.js`: 14 errors.
- `web/app.js`: 95 errors.

Application source was not reformatted during setup. This historical record does not replace fresh verification. JavaScript syntax checks passed for the four files. No JavaScript/TypeScript typecheck command is configured. Rust build and runtime checks were not run for this guidance/tooling change.
