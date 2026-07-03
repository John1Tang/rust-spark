# Skills

This codebase relies on two Claude Code skills installed in the user's
`~/.claude/skills/` directory. They are **not** committed to the repo
(the repo's `.claude/` is gitignored for the Claude preview tool's
own per-project config).

## `socks-to-http-bridge`

A small Python SOCKS5→HTTP-CONNECT bridge so `docker build` can reach
Docker Hub through the standard SOCKS5 proxy at `127.0.0.1:10808`.
Lives at `~/.claude/skills/socks-to-http-bridge/`. Run `python3
~/.claude/skills/socks-to-http-bridge/socks-to-http.py` to start it
(it listens on `127.0.0.1:8888`).

The bridge alone is not enough — `docker build` runs in a separate
buildkit process inside Docker Desktop, and Docker Desktop's settings
file (`~/Library/Group Containers/group.com.docker/settings.json`)
has to be configured to use the bridge. The exact keys:

```json
{
  "proxyHttpMode": "custom",
  "overrideProxyHttp": "http://127.0.0.1:8888",
  "overrideProxyHttps": "http://127.0.0.1:8888",
  "overrideProxyExclude": "localhost,127.0.0.1,docker.internal,::1",
  "vpnKitTransparentProxy": false
}
```

Then restart Docker Desktop. `docker pull hello-world` should work.

## `post-work`

The end-of-session ship-it loop:

1. Run `cargo test --workspace` (blocker — don't ship broken code).
2. Update `docs/architecture.md` and `docs/deployment.md` if the change
   affected them.
3. Run `./scripts/deploy.sh` (build image, import to k3d, rolling
   update master + worker + operator).
4. Verify the cluster reaches `Ready`.
5. `git add -A && git commit -m "<summary>" && git push origin main`.

Lives at `~/.claude/skills/post-work/`. The skill description triggers
on end-of-work phrases ("ship it", "post-work", "commit and push",
"roll it out", "deploy", "finish up", "ok all done", etc.).

## Running the post-work flow manually

If the `post-work` skill is not installed or Claude isn't auto-triggering
it, run the equivalent shell sequence directly:

```bash
cd /Users/john/projects/rust-spark
cargo test --workspace
./scripts/deploy.sh
kubectl -n rspark get sparkcluster demo
git add -A && git commit -m "<summary>" && git push origin main
```