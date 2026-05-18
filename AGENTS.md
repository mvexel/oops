# Repository Guidelines

## Project Structure & Module Organization

This is a Rust 2021 Cargo workspace for OOPS, an OpenStreetMap data quality scanner. Workspace members are declared in root `Cargo.toml`.

- `oops-core/`: shared OSM data types, spatial indexing, analysis traits, streaming utilities, and benchmarks.
- `oops-ingest/`: Overpass API and PBF ingestion code.
- `oops-analyzers/`: built-in quality analyzers and analyzer builders.
- `oops-cli/`: command-line interface and output formatting.
- `docs/`: Rust learning notes and planning documents.

Tests live beside implementation code in `#[cfg(test)] mod tests` blocks. Benchmarks live in `oops-core/benches/`.

## Build, Test, and Development Commands

- `cargo build`: build the workspace in debug mode.
- `cargo build --release`: build optimized binaries.
- `cargo run --bin oops-cli -- list`: list available analyzers.
- `cargo run --bin oops-cli -- analyze --file data.osm.pbf --format summary`: run analysis on a PBF file.
- `cargo test`: run unit tests.
- `cargo test -- --ignored`: run ignored integration tests that may require sample or external OSM data.
- `cargo clippy -- -D warnings`: run CI-equivalent lint checks.
- `cargo fmt --all -- --check`: verify formatting.
- `cd oops-core && cargo bench`: run Criterion spatial performance benchmarks.

## Coding Style & Naming Conventions

Use `rustfmt`; keep code compatible with `cargo fmt --all`. Treat Clippy warnings as errors. Prefer workspace crates and GeoRust ecosystem types over ad hoc spatial logic. Use `snake_case` for functions, modules, and variables, `CamelCase` for types and traits, and clear analyzer IDs such as `missing_maxspeed`.

## Testing Guidelines

Add focused unit tests near the code they exercise. Use `#[tokio::test]` for async ingestion or API paths. Mark tests requiring large files, network access, or local sample datasets with `#[ignore]`. For spatial or performance-sensitive changes, add or update tests in `oops-core` and consider `oops-core/benches/spatial_performance.rs`.

## Commit & Pull Request Guidelines

Recent commit history is informal; prefer short imperative subjects such as `fix sidewalk crossing detection` or `feat: add maxspeed analyzer`. Keep commits scoped to one concern.

Pull requests should describe behavior changes, list test commands run, link related issues when available, and include sample CLI output only when output formatting changes. Ensure CI-equivalent checks pass: `cargo test --all-features`, `cargo clippy -- -D warnings`, and `cargo fmt --all -- --check`.

## Agent-Specific Instructions

Preserve crate boundaries. Put reusable models and spatial primitives in `oops-core`, ingestion in `oops-ingest`, analyzer implementations in `oops-analyzers`, and user-facing command behavior in `oops-cli`.
