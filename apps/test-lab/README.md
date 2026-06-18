# Test Lab (Experimental Workspace)

This app is intentionally isolated from shipped UI work.

Purpose:

- Try out any local test or benchmarking experiments (UI, sync behavior, parsing
  or data shape checks).
- Run speed and sync experiments without touching `apps/admin-ui`.
- Validate ideas for the eventual data viewer before they are promoted into the
  user-facing UI.

Scope and constraints:

- Not user-facing.
- Not part of the runtime binary.
- Broadly scoped experiment surface: static files, fixtures, notes, and compacted
  experiment records.
- Can be replaced/removed whenever we decide the real UI is ready.

Run:

- Open `index.html` directly in a browser, or serve it from a local static server.
- Use the `Load` button to ingest a local JSON snapshot file.
- Use the benchmark controls to generate data and run small local timing checks.

Supporting files:

- `fixtures/` for example payloads and test input data.
- `experiments/` for active short-lived experiment notes.
