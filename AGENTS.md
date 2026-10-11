# Development

- Use idiomatic stable Rust and default rustfmt formatting.
- Keep the design small: direct functions and concrete types before abstractions.
- Do not add speculative features, generic plugin systems, or unused dependencies.
- Keep Windows FFI in small modules. Every unsafe block needs a safety explanation.
- Never infer engine addresses or layouts from a different build. Unknown game
  state or an unverified integration must reject item requests explicitly.
- Do not report a successful grant until the game inventory confirms it.
- Keep game assets, decompiled game code, saves, and local toolchains out of Git.
- Run cargo fmt --check, cargo clippy --workspace --all-targets -- -D warnings,
  cargo test --workspace, and the 32-bit adapter smoke test after relevant changes.
- Document what works separately from planned game integration.
