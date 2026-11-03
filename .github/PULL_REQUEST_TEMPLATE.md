# Pull Request Template

## What does this PR do?

<!-- Brief description of changes -->

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

## Does this break the build?

- [ ] No — workspace `cargo check --workspace` passes
- [ ] Yes — sub-task of the PR (explain below)

## Verification

- [ ] `cargo fmt --check` — clean
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` — 0 errors
- [ ] `cargo test --workspace` — all tests green

## Notes

<!-- Anything the reviewer should know? Native MF/NDK scaffolds, device-validation pending, etc. -->
