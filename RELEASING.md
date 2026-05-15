# Releasing grok-search-rs

## The only thing you do

```bash
# 1. (optional) add a section to CHANGELOG.md and push it
$EDITOR CHANGELOG.md
git commit -am "docs: changelog for 0.1.5"
git push

# 2. Tag and push
git tag v0.1.5
git push origin v0.1.5
```

That's all. Pushing the tag triggers `release.yml`, which then:

1. Injects `0.1.5` into `Cargo.toml` (in CI working tree) and builds cross-platform binaries
2. Creates the GitHub Release with archives + `SHA256SUMS`
3. Publishes the 6 npm packages (main + 5 platform sub-packages) with version `0.1.5`
4. **Commits the version bump back to `main`** so `Cargo.toml`, `Cargo.lock`, and all `package.json` files stay in sync with the latest release

## Manual fallback (rarely needed)

If CI is unavailable and you want to bump manifests by hand:

- **Local script**: `scripts/bump-version.sh 0.1.5 --push` (bumps, commits, tags, pushes)
- **GitHub UI**: Actions → Bump Version → Run workflow

Both predate the tag-triggered auto-sync and remain for offline use.

## Where version numbers live

- `Cargo.toml` — auto-synced to `main` by the `sync-main` job
- `Cargo.lock` — refreshed alongside `Cargo.toml`
- `npm/grok-search-rs/package.json` (main + 5 `optionalDependencies`) — auto-synced
- `npm/platforms/*/package.json` (5 files) — auto-synced

## Prerequisites

- `secrets.NPM_TOKEN` configured
- No branch protection rule on `main` blocking `github-actions[bot]`

## Verify after release

- GitHub release page lists 5 archives + `SHA256SUMS`
- `npx grok-search-rs@X.Y.Z --help` works
- `main` has a `chore: sync manifests to X.Y.Z` commit from `github-actions[bot]`
