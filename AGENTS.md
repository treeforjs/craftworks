# Craftworks — instructions for agents

Craftworks holds what every craft app shares. Today that is `crates/craft-ui`, the design system. Read [`DESIGN.md`](DESIGN.md) first.

- **Never crash.** No `unwrap`/`expect`/`panic!`/`unreachable!`/`todo!`/`unimplemented!`, no
  indexing (`get()`), no `unsafe` outside tests (enforced by the crate's lints). Every component
  must draw with no fonts installed, no icon painter, an empty list and a zero-sized window
  (`tests::gallery_draws_in_every_mode`).
- **Our own design.** Never measure, sample or copy colours, sizes, icons, layout or wording from
  another vendor's app. Keep `tokens::tests::contrast_meets_wcag_aa` green.
- **No assets here.** No font files (they live in craft-fonts or the apps), no icon files (apps
  register a painter).
- **No app dependencies.** craft-ui depends only on egui and serde, and builds for wasm32.
  craft-update has no UI dependency; its network and installer code is desktop-only, the rest
  builds for wasm32. Never weaken its verification (signed list first, hash every download).
- **Releases:** a breaking change bumps the minor version; each release gets a `<crate>-vX.Y.Z` tag (`craft-ui-v0.1.0`, `craft-update-v0.1.0`), and apps pin tags.
- Gates before every commit: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test`, `cargo check --target wasm32-unknown-unknown`. Look at the gallery screenshots
  for any visual change.
