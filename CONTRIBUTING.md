# Contributing

Contributions are welcome, but the project has one architectural constraint that is not negotiable: ordinary application/UI code must not gain a vehicle-write capability.

Before adding a platform integration, read `docs/SAFETY.md` and `docs/ARCHITECTURE.md`.

For now, keep changes small and desktop-testable. Prefer deterministic mocks and recorded inputs over live vehicle access.

Run before opening a PR:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```
