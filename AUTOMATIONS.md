# Automations

This document describes all automations in the project, where they live, and what they enforce.

## Local automations (git hooks)

Git hooks are configured via `core.hooksPath` pointing to `.githooks/`. Run `./setup-hooks.sh` once to enable them.

| Hook file | Trigger | What it does |
|---|---|---|
| `.githooks/commit-msg` | `git commit` | Validates the commit message follows [Conventional Commits](https://www.conventionalcommits.org/). Rejects the commit if the first line doesn't match `type[(scope)][!]: description`. |

## CI (continuous integration)

All CI workflows live in `.github/workflows/`.

The `clippy`, `test`, and `docs` jobs install the native libraries needed to compile the project (PipeWire, ALSA, udev, and `libclang` for `bindgen`) before running.

### `ci.yml` — Core CI

**Trigger:** every push and pull request (any branch).

| Job | Tool | What it checks |
|---|---|---|
| PR title | shell | PR title follows Conventional Commits (PRs only). |
| Commit message | shell | Last commit message follows Conventional Commits (pushes only). |
| Format | `cargo fmt --all -- --check` | All code is formatted with rustfmt. |
| Clippy | `cargo clippy --all-targets --all-features -- -D warnings` | No lint warnings. |
| Test | `cargo test --all` | All unit and integration tests pass. |
| Docs | `RUSTDOCFLAGS="-D warnings" cargo doc --all-features --no-deps` | Documentation builds without warnings. |
| Unused deps | `cargo machete` | No unused dependencies declared in `Cargo.toml`. |

### `security.yml` — Security and license audit

**Trigger:** every push and PR to `main`, plus every Monday at 06:00 UTC.

| Job | Tool | What it checks |
|---|---|---|
| cargo-deny | `cargo-deny` | Advisories (RUSTSEC), duplicate crate bans, license compatibility, and source/registry policy. Configured via `deny.toml`. |

`deny.toml` sets `multiple-versions = "warn"` rather than `deny`: `iced` and its GUI dependency tree legitimately ship multiple versions of several crates, so duplicates are reported without failing the build.

### `binary-check.yml` — Binary size and compilation time

**Trigger:** pull requests targeting `main`.

| Job | Tool | What it checks |
|---|---|---|
| Size & compile time | `cargo bloat`, `cargo build --release` | Compares binary size and wall-clock compilation time against the baseline from `main`. Fails if size grows >10% or compile time grows >20%; the report is retained in the workflow logs. |

### `binary-baseline.yml` — Baseline recording

**Trigger:** push to `main`.

| Job | Tool | What it does |
|---|---|---|
| Update baseline | `cargo build --release` | Records binary sizes (per executable) and compilation time into `baseline.json`, then stores it in the GitHub Actions cache for PR comparisons. |

## Releases

Releases are automated end-to-end from Conventional Commits. Two workflows coordinate via the `chore: release` commit message.

### `release.yml` — Release orchestration

**Trigger:** push to `main` (unless the commit is a release merge).

| Step | Tool | What it does |
|---|---|---|
| Determine bump | shell | Analyzes Conventional Commits since the last git tag to determine the next semver bump level (major/minor/patch). |
| Bump version | `cargo set-version --bump` | Bumps the version in `Cargo.toml`. |
| Update changelog | `git cliff --tag` | Generates `CHANGELOG.md` from unreleased Conventional Commits. |
| Create PR | `peter-evans/create-pull-request` | Creates or updates a PR on branch `release/auto` with the version bump and changelog changes. |

The PR is created with the commit message `chore: release`. Merge it with **squash merge** so the resulting `main` commit message is `chore: release (#N)`; that message triggers the tag workflow.

### `release-tag.yml` — Git tag, GitHub release, and packages

**Trigger:** push to `main` with a commit message starting with `chore: release`.

| Step | Tool | What it does |
|---|---|---|
| Build | `cargo build --release` | Builds the `midi-does` binary and strips debug symbols. |
| DEB package | `cargo deb --no-build` | Builds a `.deb` with the binary, desktop entry, and hicolor icons. |
| RPM package | `cargo generate-rpm` | Builds a `.rpm` with the binary, desktop entry, and hicolor icons. |
| Read version | `cargo metadata` | Reads the version from `Cargo.toml`. |
| Nix recipe | `tar` | Packages `flake.nix` + `flake.lock` as `midi-does-<version>-nix.tar.gz`. |
| Create tag | `git tag` | Creates an annotated tag (`v{version}`) and pushes it. |
| Generate notes | `awk` | Extracts the first version section from `CHANGELOG.md`. |
| Create release | `softprops/action-gh-release` | Creates a GitHub release with the changelog entry as body and attaches the raw binary, `.deb`, `.rpm`, and Nix recipe tarball. |

### Release artifacts

Each GitHub release contains:

| Artifact | Purpose |
|---|---|
| `midi-does` | Raw `x86_64` Linux binary. |
| `*.deb` | Debian/Ubuntu package (installs binary, desktop entry, and icons). |
| `*.rpm` | RPM package for Fedora/openSUSE (same contents as the DEB). |
| `midi-does-<version>-nix.tar.gz` | The Nix recipe: `flake.nix` + `flake.lock`. |

The Nix recipe builds the package from source via `nix build`/`nix run` against the `packages.<system>.default` flake output.

## Configuration files

| File | Purpose |
|---|---|
| `deny.toml` | cargo-deny configuration: allowed licenses, advisory policy, duplicate crate bans, sources. |
| `cliff.toml` | git-cliff configuration: changelog format, commit parsers, tag pattern. |
| `.githooks/commit-msg` | Shell script enforcing Conventional Commits locally. |
| `setup-hooks.sh` | One-time script to enable git hooks via `core.hooksPath`. |
| `Cargo.toml` `[package.metadata.deb]` | cargo-deb configuration: Debian metadata, dependencies, and assets. |
| `Cargo.toml` `[package.metadata.generate-rpm]` | cargo-generate-rpm configuration: RPM metadata and assets. |
| `flake.nix` | Nix package + development shell (used by the release as the Nix recipe). |
