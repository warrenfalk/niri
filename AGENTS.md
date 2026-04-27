# Repository Guidelines

## Project Structure & Module Organization
`src/` contains the compositor core. Common areas are `backend/`, `input/`, `layout/`, `protocols/`, `render_helpers/`, `ui/`, and `window/`; broader integration-style tests live in `src/tests/`. Workspace crates are split by responsibility: `niri-config/` for config parsing and validation, `niri-ipc/` for IPC types/helpers, and `niri-visual-tests/` for manual rendering checks. Runtime assets and packaged defaults live in `resources/`. User-facing documentation and wiki sources live in `docs/wiki/`.

## Build, Test, and Development Commands
Use `nix develop` if you want the project’s preferred toolchain and system libraries, including nightly `rustfmt`.

- `cargo check` - fast default-feature sanity check.
- `cargo test --all --exclude niri-visual-tests -- --nocapture` - closest match to the main CI test job.
- `env RUN_SLOW_TESTS=1 cargo test --all` - enables slow randomized tests.
- `env RUN_SLOW_TESTS=1 PROPTEST_CASES=200000 RUST_BACKTRACE=1 cargo test --release --all` - heavier pre-push regression sweep.
- `cargo run -p niri-visual-tests` - launch GTK/libadwaita visual test cases.
- `cargo +nightly fmt --all` and `cargo clippy --all --all-targets` - required before review.

For docs work, run `uv sync` and `uv run mkdocs serve` inside `docs/`.

## Coding Style & Naming Conventions
Target Rust 2021 with MSRV 1.85. Follow the existing module shape before inventing new abstractions; similar features should look like neighboring code. Use snake_case for modules/functions, CamelCase for types, and keep files scoped by subsystem (`backend/tty.rs`, `protocols/*.rs`, etc.). Formatting is driven by `rustfmt.toml`; imports are grouped at module granularity, and comments should stay tight and technical.

## Testing Guidelines
Prefer narrow unit tests near the code and broader scenario coverage in `src/tests/`. Snapshot assertions use `insta`; review snapshot diffs carefully when files under `src/tests/snapshots/` change. Property tests use `proptest!`; if you add a new layout operation, register it in the `Op` enum in `src/layout/mod.rs` so randomized tests exercise it. New config options should also gain parsing coverage.

This checkout is maintained as an ongoing fork with local features rebased onto newer upstream niri releases. Any new fork-local feature should include focused tests in the same feature commit whenever practical, so rebases fail loudly instead of silently dropping behavior. Prefer tests that lock the public contract, regression trigger, or pure logic behind the feature; if a full compositor/rendering integration test is impractical, add lower-level unit, serialization, or state tests and record any remaining manual validation.

## Commit & Pull Request Guidelines
Recent history uses short, imperative subjects, often with a scope prefix such as `tty:`, `backend/winit:`, or `wiki:`. Keep commits small, self-contained, and buildable. Rebase instead of merging `main`. PRs should explain the problem, the change, and how to test it; update `docs/wiki/` when config or behavior changes are user-visible, and leave "Allow edits from maintainers" enabled.
