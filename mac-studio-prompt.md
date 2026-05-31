# Mac Studio CodeWhale Handoff

Use this when moving the external SSD to the Mac Studio and resuming the
v0.8.48 release-readiness work.

## Source Of Truth

- Checkout: `/Volumes/VIXinSSD/whalebro/codewhale`
- Branch: `harvest/v0.8.48-community`
- Lightweight Desktop migration bundle:
  `/Volumes/VIXinSSD/whalebro/migration-local-only/desktop-codewhale-light-20260530-180216`
- Bundle setup notes:
  `/Volumes/VIXinSSD/whalebro/migration-local-only/desktop-codewhale-light-20260530-180216/MAC_STUDIO_SETUP.md`

The migration bundle intentionally excludes duplicate repos, `target/`,
`node_modules/`, `.git/objects`, `.next/`, `dist/`, and build caches. It keeps
small local notes/configs and git metadata only. Review any copied env/config
files before placing them under `~/.codewhale`.

## First Commands On The Mac Studio

```bash
cd /Volumes/VIXinSSD/whalebro/codewhale
git status --short --branch
git log --oneline --decorate -5

cargo build --release -p codewhale-cli -p codewhale-tui

mkdir -p ~/.npm-global/bin
ln -sfn /Volumes/VIXinSSD/whalebro/codewhale/target/release/codewhale ~/.npm-global/bin/codewhale
ln -sfn /Volumes/VIXinSSD/whalebro/codewhale/target/release/codewhale-tui ~/.npm-global/bin/codewhale-tui
ln -sfn /Volumes/VIXinSSD/whalebro/codewhale/target/release/codew ~/.npm-global/bin/codew
ln -sfn /Volumes/VIXinSSD/whalebro/codewhale/target/release/deepseek ~/.npm-global/bin/deepseek
ln -sfn /Volumes/VIXinSSD/whalebro/codewhale/target/release/deepseek-tui ~/.npm-global/bin/deepseek-tui

codewhale --version
codewhale-tui --version
```

## Smoke Checks

```bash
codewhale doctor

printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
  | codewhale mcp-server \
  | head -n 1
```

## Sanitized Previous Handoff Context

- The last stale-binary symptom was traced to an old local launcher, not a
  current runtime bug. After moving machines, rebuild first and relink launchers
  before judging runtime behavior.
- Xiaomi/MiMo now supports a `providers.xiaomi.cluster` setting for `cn`, `sgp`,
  and `ams`. An explicit provider `base_url` or `MIMO_BASE_URL` still wins over
  the cluster setting.
- MiMo Token Plan keys are cluster-specific. Do not assume a key that works in
  one region will work in another; configure the cluster/base URL to match the
  issued key.
- The generic provider resolver names were cleaned up from old
  `deepseek_*` wording to `active_provider_*`; real DeepSeek identifiers,
  environment variables, and legacy path fallback names intentionally remain.
- Issue #2363 was fixed by correcting `/provider` wording in five locales.
- Wanjie Ark's default documented model moved to `deepseek-v4-pro`; live catalog
  verification still needs a Wanjie key.
- Website facts were regenerated for v0.8.48 and should show Xiaomi MiMo in the
  provider list.
- US-facing web deployment should start with Railway using `web/railway.json`.
  Keep Cloudflare as the edge/cron/KV route until the curator storage path is
  replaced.
- 0.9.0 whale-pods work should stay next-cycle scope, not a late v0.8.48
  release add-on.

The original private handoff file contained machine-local setup details and
credential-adjacent notes. Keep it out of git history.

## Release Readiness Prompt

Paste this into a fresh CodeWhale session after the rebuild:

```text
Use parallel sub-agents. Agent A: inspect this checkout for v0.8.48 release/docs drift around `.codewhale` vs `.deepseek` paths and provider lists. Agent B: inspect release artifact/npm wrapper paths for missing assets, especially Windows `codewhale.bat` and updater hints. Agent C: inspect runtime liveness risks around sub-agent fanout, compaction/status UI, and MCP tool discovery. Do not edit files. Return a release-risk table with exact file references, confidence, and one recommended follow-up test.
```

## Current Release Notes

- v0.8.48 GitHub Release was not live at last check.
- The release workflow body includes direct contributor credits; do not publish
  a release body that only links to `CHANGELOG.md`.
- If the release already exists when resuming, verify:

```bash
/opt/homebrew/bin/gh release view v0.8.48 --repo Hmbown/CodeWhale --json body
```

The body must include a `## Contributors` or `## Credits` section with the
material contributors for v0.8.48.
