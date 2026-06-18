# Contract Tests

Use this directory for tests that lock down public behavior across crate boundaries:

- CLI command contracts.
- File format compatibility contracts.
- Local store behavior contracts.

Contract tests should describe behavior from the caller's point of view and avoid depending on private implementation details.
