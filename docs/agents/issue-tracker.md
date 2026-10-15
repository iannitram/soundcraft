# Issue tracker: Software Depot reports

Issues for this fork are not GitHub Issues. They are markdown files in the Software Depot, a shared folder outside this repo, at `apps/<App>/reports/`. If you don't know where the depot folder is on this machine, ask the person.

This repo is a public fork, so nothing private goes into it: no report text, no file or machine names from the person's own projects.

## Conventions

- **One file per report:** `apps/<App>/reports/YYYY-MM-DD-<short-slug>.md`, copied from the depot's `report-template.md`. Never one shared list.
- **Fill in** the reporter (ask if you don't know), the OS and machine, and the app version.
- **Status line** near the top. Flow: `new` → `triaged` → `in-progress` → `fixed` (name the version or commit) → `verified` by the reporter. Off-ramps: `wontfix`, `upstream` (filed with storytold, waiting on them), `duplicate` (link the original).
- **Append, don't rewrite:** add dated lines under the report's Work Log. Never delete someone else's text.
- **Branches:** one per report, `fix/<report-slug>` or `feat/<report-slug>`, with the report file name in the commit message.
- **Upstream's GitHub Issues** (`storytold/<app>`) are upstream's tracker. Read them freely; filing or commenting there publishes under the owner's name, so ask first.

## When a skill says "publish to the issue tracker"

Create a new report file as above, status `new`.

## When a skill says "fetch the relevant ticket"

Read the report file. The person normally gives the slug or the path.

## Wayfinding operations

Used by `/wayfinder`.

- **Map:** `apps/<App>/reports/YYYY-MM-DD-<effort>-map.md`, holding the Notes / Decisions-so-far / Fog body.
- **Child ticket:** one report file per ticket, named `YYYY-MM-DD-<effort>-NN-<slug>.md` (numbered from `01`), with a `Type:` line (`research`/`prototype`/`grilling`/`task`) and a `Part of:` line naming the map file.
- **Blocking:** a `Blocked by: NN, NN` line near the top. A ticket is unblocked when every ticket it lists is `fixed` or `verified`.
- **Frontier:** the effort's open, unblocked tickets with no `Claimed by:` line; lowest number wins.
- **Claim:** add `Claimed by: <session or person>` and set status `in-progress`, before any work.
- **Resolve:** append the answer under an `## Answer` heading, set the status, then add a one-line pointer to the map's Decisions-so-far.
