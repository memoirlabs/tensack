# sixpack Commands

This document tracks the public CLI command surface, exit codes, output
stability rules, and scripting guarantees.

The CLI is intentionally small while the runtime API and storage engine settle.

## Implemented Surface

- `sixpack --version` (or `sixpack -V`) - print the CLI version.
- `sixpack help` (or `-h`, `--help`) - print usage.

## Planned Surface

Future commands should focus on inspectable local development workflows:

- initialize a database directory
- inspect schema and storage status
- rebuild generated indexes
- repair or truncate incomplete final writes
- run focused benchmarks or diagnostics
