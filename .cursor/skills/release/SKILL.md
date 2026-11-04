---
name: release
description: >-
  Ship a PudduScroll GitHub release. Use when the user asks to release, cut a
  version, bump the app version, publish, or tag a build.
---

# Release

Ship one `x.y.z` GitHub release from a clean tree. Loop each gate until it passes before moving on.

## Version

Single source of truth: `[workspace.package].version` in the root `Cargo.toml`.

Crate manifests use `version.workspace = true`. Path deps omit `version`. Do not write a version number anywhere else.

Semver:

- `x` major: breaking change for users or the library file
- `y` minor: new user-facing capability
- `z` patch: fix or small safe change

Ask which bump if the user did not say. Default to patch when they only said "release" and the diff is fixes.

## Steps

Copy and track:

```
Release:
- [ ] 1. Diff and choose bump
- [ ] 2. Write version and notes
- [ ] 3. Commit until green
- [ ] 4. Tag, push, open release workflow
- [ ] 5. Watch CI until the GitHub release exists
```

### 1. Diff and choose bump

Read `git status`, `git diff`, and recent `git log`. Read the current version from root `Cargo.toml`. Choose `x.y.z` with the rules above.

### 2. Write version and notes

Set `[workspace.package].version` to the new `x.y.z`.

Rewrite `.github/release-notes.md`:

1. Keep the Downloads block and install help as they are, only swapping the version in the opening sentence.
2. Replace the `## New` bullets for this release.
3. Load the `unslop` skill and run it on the opening sentence and every `## New` bullet before saving.

`## New` rules after unslop:

- Plain user language only. No crate names, file paths, API words, or editor implementation detail.
- One short sentence per bullet.
- Dash list under `## New` only. No other headings inside that section.
- Two to five bullets. Skip empty marketing lines.

The update dialog reads only the `## New` bullets and turns them into a spaced pointed list. Bad notes show up there.

### 3. Commit until green

Stage the version bump, `.github/release-notes.md`, and every other file that belongs in this release.

Follow the repo commit-review rules: read the staged diff, run fmt and clippy, mark reviewed, then commit. Do not use `--no-verify`.

If the commit fails (hook, message, staging), fix the cause and create a **new** commit. Do not amend unless the repo commit rules allow it. Loop until `git status` shows a clean tree on the new commit.

Commit message shape: `Ship x.y.z with <short user-facing why>.`

### 4. Tag, push, open release workflow

```bash
git tag "vX.Y.Z"
git push origin HEAD
git push origin "vX.Y.Z"
```

Pushing the `v*` tag starts `.github/workflows/release.yml`.

### 5. Watch CI until the GitHub release exists

Poll until `gh release view "vX.Y.Z"` succeeds and lists the four archives:

- `pudduscroll-windows-x86_64.zip`
- `pudduscroll-linux-x86_64.tar.gz`
- `pudduscroll-macos-aarch64.zip`
- `pudduscroll-macos-x86_64.zip`

While waiting:

```bash
gh run list --workflow=release.yml --limit 5
gh run view <id> --json conclusion,status,jobs
```

On a failed job:

1. Read the failed job log (`gh run view <id> --log-failed`).
2. If the log shows a GitHub Actions outage, runner flake, network blip, or lost artifact with no repo change needed, restart that run: `gh run rerun <id> --failed`.
3. If the log shows a real project error, fix it on the branch, commit again with the same loop as step 3, move the tag to the new commit (`git tag -f "vX.Y.Z"`, `git push -f origin "vX.Y.Z"`), and watch the new run.
4. Loop until the release exists with all four assets.

Done when `gh release view "vX.Y.Z"` shows the four archives and the notes match `.github/release-notes.md`.

## Do not

- Bump versions in crate `Cargo.toml` files.
- Put technical implementation notes under `## New`.
- Skip the commit hook.
- Force-push the branch. Force-moving only the release tag is allowed when a fix lands after a bad tagged build.
