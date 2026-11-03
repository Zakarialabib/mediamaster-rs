# Contributing to mediamaster-rs

## Branching

- Create a feature branch from `main`: `git checkout -b feat/your-feature`
- Do **not** commit directly to `main`. Open a Pull Request against `main`.
- GitHub branch protection is not yet configured on this repo — see
  [Branch protection](#branch-protection) below for the recommended settings.

## Local verification (required)

Before opening a PR, run the portable gate locally:

```bash
cargo fmt --check          # must be clean
cargo clippy --workspace --all-targets -- -D warnings   # 0 errors
cargo test --workspace                            # all tests green
```

## PR checklist

- [ ] `cargo check --workspace` passes
- [ ] `cargo fmt --check` — clean
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` — 0 errors
- [ ] `cargo test --workspace` — all tests green
- [ ] New crates include a `README.md`
- [ ] Breaking changes documented in `CHANGELOG.md`

## Naming conventions

- All crates use the `mediamaster-<name>` package name (required for crates.io).
- Directory names match the package name: `mediamaster-core`, `mediamaster-bus`, …

## License

See each crate's `Cargo.toml` for the license header. New files must carry the
standard authors + license header.
