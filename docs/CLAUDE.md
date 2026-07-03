# rspark docs

This directory holds the project wiki. Each file is one topic; the table below
points at the right one. The files are small and meant to be read end-to-end
by someone new to the codebase.

| If you want to…                                  | Read                                              |
| ------------------------------------------------ | ------------------------------------------------- |
| Get a cluster running on k3d                      | [deployment.md](deployment.md)                   |
| Understand the SQL surface and add a feature      | [sql-surface.md](sql-surface.md)                 |
| Use the local CLI                                | [cli.md](cli.md)                                 |
| Call the HTTP API                                | [api.md](api.md)                                 |
| Drive the dashboard from a browser                | [dashboard.md](dashboard.md)                     |
| Use the k8s operator                              | [operator.md](operator.md)                       |
| Read the architecture end-to-end                  | [architecture.md](architecture.md)               |
| Ship a change (commit + push + rolling update)    | [skills.md](skills.md) — run the `post-work` skill |
| Make a change to the codebase                     | [CONTRIBUTING.md](../CONTRIBUTING.md) + [CLAUDE.md](../CLAUDE.md) |

## Top-level index (the "wiki")

The README and CLAUDE.md at the repo root are the onboarding entry points.
This directory is the deeper reference:

```
docs/
├── CLAUDE.md         (this file)        — index, "where do I look?"
├── architecture.md                       — crate-by-crate tour
├── cli.md                                — local CLI commands
├── api.md                                — HTTP API endpoints
├── dashboard.md                          — dashboard behaviour and JS hooks
├── deployment.md                         — k3s/k3d/kubectl handbook
├── operator.md                           — SparkCluster CRD and the operator
└── sql-surface.md                        — what SQL works, what doesn't
```

## Conventions

- **Each file is small and stand-alone** — don't be afraid to add a new
  topic file when one doesn't fit the existing categories.
- **Code blocks use shell commands** that you can copy-paste. They assume
  you're at the repo root unless stated otherwise.
- **Path conventions**: `crates/rspark-X/src/...` is the canonical
  reference; relative paths are from the repo root.
- **Out of date?** Open a PR — the wiki is meant to drift toward the
  truth, not be a frozen snapshot.

## When to update the wiki

| Change in source                      | Update file                                       |
| ------------------------------------- | ------------------------------------------------- |
| New CLI subcommand                    | `cli.md`                                          |
| New HTTP endpoint or changed payload  | `api.md`                                          |
| New SQL feature                      | `sql-surface.md` (and `CLAUDE.md` if it changes the pipeline) |
| New k8s manifest or deploy procedure  | `deployment.md`                                    |
| New operator behaviour                | `operator.md`                                      |
| New crate or major refactor            | `architecture.md`                                  |