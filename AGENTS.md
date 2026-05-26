# AGENTS.md

## Build & Dev Commands

```bash
cargo build              # Compile (debug)
cargo build --release    # Compile (release)
cargo run                # Build and run
cargo check              # Fast compile check (no binary)
cargo clean              # Remove build artifacts
```

### Lint / Format

```bash
cargo clippy             # Run linter (install: rustup component add clippy)
cargo fmt                # Auto-format (install: rustup component add rustfmt)
cargo fmt --check        # Check formatting in CI
```

### Testing

```bash
cargo test               # Run all tests
cargo test -- --nocapture           # Show stdout
cargo test <name_filter>            # Run tests matching name (e.g. cargo test my_test)
cargo test <module>::<test_name>    # Fully qualified single test
cargo test --test <file>            # Integration test in tests/<file>.rs
```

Test at the bottom of the source file, or in `tests/` directory for integration tests.

## Code Style

### Imports
- Group: `std` imports first, then external crates, then `crate`/`super` — each separated by a blank line.
- Use brace grouping: `use std::collections::{HashMap, HashSet};`
- Prefer `use crate::foo::Bar` over `super::` where possible.

### Types & Traits
- `UpperCamelCase` for types, enums, traits.
- Derive common traits: `#[derive(Debug, Clone, PartialEq)]` where appropriate.
- Prefer `impl` blocks over freestanding functions for type-associated logic.
- Use `pub` / `pub(crate)` / private visibility deliberately; default to private.

### Naming
- `snake_case` for functions, variables, modules, file names.
- `SCREAMING_SNAKE_CASE` for constants and statics.
- `UpperCamelCase` for type/struct/enum/trait names.
- Module file names match module name: `mod foo;` → `foo.rs`.
- Avoid abbreviations unless universally understood (e.g. `msg`, `idx`).

### Error Handling
- Return `Result<T, E>` from fallible functions; use `?` to propagate.
- Define domain error types with `thiserror` (if added) or `#[derive(Debug)]` enums.
- In `main`, either return `Result<(), Box<dyn std::error::Error>>` or handle fatal errors with `.expect()` / `eprintln!`.
- Avoid bare `.unwrap()` in production code; prefer `.expect("context")`.

### Formatting
- 4-space indentation, not tabs.
- 100-char max line width (Rust 2024 edition default).
- Opening brace on same line (Rust default).
- Spaces around binary operators, after `:` in type annotations.
- Trailing commas in multi-line struct/tuple/enum literals.
- Run `cargo fmt` before committing.

### General
- No wildcard imports (`use foo::*`) except in test modules or prelude patterns.
- Prefer `String` / `Vec` owned types for function returns; accept `&str` / `&[T]` for borrowing.
- Use `#[cfg(test)] mod tests { ... }` for inline unit tests.
- Use `///` doc comments on public API items.
- Keep functions small and single-purpose.
- Avoid `unsafe` unless absolutely necessary, with safety comments.
