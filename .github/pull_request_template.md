# Pull request template — feel free to delete sections that don't apply.
#
# This template is opinionated on purpose: the questions below are the same
# ones a reviewer will ask anyway, so answering them up-front unblocks
# the review faster.

## What does this change?

<!-- One or two sentences. Mention the issue number if it exists. -->

Fixes #

## How was it tested?

<!-- What did you actually run? Be specific — `cargo test --workspace`
     is good, but paste any sample SQL / curl / output that helps. -->

- [ ] `cargo fmt --all` clean
- [ ] `cargo clippy --workspace --lib --bins -- -D warnings` clean
- [ ] `cargo test --workspace` passes
- [ ] I added a test (or a new entry in `examples/demo.sh` / dashboard)
- [ ] I read `CONTRIBUTING.md`

## Backwards compatibility

<!-- Does this change any public API, CLI flag, JSON endpoint, or the SQL
     surface? If so, what's the migration path? -->

- [ ] No breaking change
- [ ] Breaking change documented in `CHANGELOG.md`

## Screenshots / output

<!-- If the change is visible in the dashboard, paste a screenshot or a
     curl response. For SQL: paste the query and a few result rows. -->

```text

```
