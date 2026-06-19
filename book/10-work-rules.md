# Work Rules

These rules are for agents and contributors.

## Before Changing Code

Read:

1. `book/README.md`
2. relevant chapter in `book/`
3. `book/07-implementation-status.md`
4. code in the package you are changing

Do not assume old architecture drafts are current.

## When Updating Behavior

- Update tests.
- Update implementation status.
- Update the relevant book chapter.
- Keep public names boring and descriptive.
- Do not claim planned behavior is implemented.

## When Adding Docs

Decide whether the doc is:

- internal design book
- user-facing docs
- implementation status
- old background reference

Do not scatter source-of-truth decisions across random drafts.

## Required Checks

Run from repo root:

```sh
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
```

## Dirty Worktree Rule

Do not revert changes you did not make.

If files have moved or changed concurrently, work with the current tree and
mention it in the final summary.
