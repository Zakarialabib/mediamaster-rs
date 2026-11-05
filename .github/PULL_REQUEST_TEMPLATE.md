# Pull Request Template

## What does this PR do?

<!-- Brief description of the change and the problem it solves. -->

## Which crate(s) does this touch?

- [ ] mediamaster-core
- [ ] mediamaster-bus
- [ ] mediamaster-probe
- [ ] mediamaster-codec
- [ ] mediamaster-audio
- [ ] mediamaster-agent
- [ ] mediamaster-engine
- [ ] mediamaster-rag
- [ ] mediamaster-rf
- [ ] mediamaster-sensors
- [ ] mediamaster-bridge
- [ ] mediamaster-frame
- [ ] mediamaster-integration-tests
- [ ] xtask / CI / repo tooling
- [ ] docs only

## Does this break the build?

- [ ] No — workspace `cargo check --workspace` passes
- [ ] Yes — sub-task of the PR (explain below)

## Does this change the public API?

- [ ] No public API change
- [ ] Yes — SemVer bump applied and `CHANGELOG.md` updated (see [docs/RELEASE.md](../docs/RELEASE.md) §1)

## Verification

Local gate (see [CONTRIBUTING.md](../CONTRIBUTING.md)):

- [ ] `cargo fmt --check` — clean
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` — 0 errors
- [ ] `cargo test --workspace` — all tests green
- [ ] `cargo check --workspace` — clean (includes `xtask`)
- [ ] `#![no_std]` drift checked (`mediamaster-core` target check) — n/a or green
- [ ] New crates/features ship a `README.md` update

## Notes

<!-- Anything the reviewer should know? Native MF/NDK scaffolds, device-validation
pending, follow-up issues, screenshots, etc. -->
