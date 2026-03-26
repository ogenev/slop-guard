# Repo Notes

- This repo is a single Rust package. Keep `src/main.rs` thin and put behavior in `src/lib.rs` modules.
- Current layout: `src/cli.rs`, `src/github/`, `src/store/`, `src/analyzers/`, `src/features/`, `src/scoring/`.
- Add new top-level modules only when the boundary is real; do not split into a workspace prematurely.
- Add comments to any important structs and methods
- For Rust validation, run:
  - `cargo test`
  - `cargo +nightly fmt --all`
  - `RUSTFLAGS="-D warnings" cargo +nightly clippy --all-features --locked`
