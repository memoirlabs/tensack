# Note Taking Playground

Interactive test-lab app for exercising sixpack as a real local database.

This is separate from `note-taking-init`, which stays the minimal untouched
compiler/init example. The playground is intentionally for experiments:

- create notes through a tiny browser UI,
- edit and delete rows through the sixpack runtime API,
- watch read-after-write timing,
- poll for local changes,
- compact the notes table and inspect byte shrinkage.

Run:

```sh
cargo run -p note-taking-playground -- --reset
```

Then open:

```txt
http://127.0.0.1:4766/
```

Options:

```txt
--host <host>    bind host, default 127.0.0.1
--port <port>    bind port, default 4766
--out <path>     database root, default target/test-lab/note-taking-playground
--reset          remove the playground output before startup
```

The database lives under:

```txt
target/test-lab/note-taking-playground/notes-db/
```

This app is not the public admin UI. It is a focused playground for speed,
reactivity, and storage-shape experiments.
