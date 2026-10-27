# Rules for agents in this repo (our pipeline)

This is a fork. `AGENTS.md` and `CLAUDE.md` are upstream's: read and follow them too (upstream's first rule: the app never crashes). This file adds our pipeline's rules.

Everything you need to work here is in this repo. The full pipeline and rules live in the pipeline repo (private: `PIPELINE.md`, `RULES.md`, `RUBRIC.md`, `roles/`); this is the short copy.

1. **Find the stage** in `PIPELINE.md`. The spec is `SPEC.md` (upstream's roadmap plus our delta), the plan upstream's `ROADMAP.md` and `docs/gaps.md`, the code map `docs/code-map.md`.
2. **Work from an issue.** Take one that is not `in-progress` and not blocked; add `in-progress` before you start. Its body is your brief; an Easy issue has a "Qwen brief" section with the expected results and the code map snippet.
3. **Your own platform's issues first.** Mac, Windows and Linux all matter: say which you built on.
4. **Check that it builds**, then open the PR. CI and a review by another satellite with another model do the rest.
5. **Label the PR** `by:<machine>` and `model:<model>`, and write `Closes #N`.
6. **A failed job** gets a comment on the issue saying what happened, and the issue moves up one size (Easy -> Medium -> Hard).
7. **Use the project's words** from `GLOSSARY.md` and the decisions in `docs/adr/`.
8. **No private details** in code, commits or PR text: no names, e-mail addresses, home paths or machine names. Commit as the no-reply identity.

## This project

- **Integration branch is `main`.** PRs go into main; the daily fork sync merges upstream into it.
- **Check upstream first.** Before starting an issue, search storytold/soundcraft's open PRs for the same work; if upstream has it, comment on our issue and take another.
- **Never edit upstream's files for our process** (ROADMAP.md, AGENTS.md, CLAUDE.md): it conflicts on every sync.
- **Upstream PRs need the owner's yes, one at a time**; no credit beyond the username (pinned issue #33).
- **A bug is real when a test fails without the fix.** Write the failing test first.
- **Licence wall:** MIT/Apache only; GPL code is reference only. Real show audio never enters the repo.
- **Quality gate:** `cargo xtask ci`.
