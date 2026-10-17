# Craftworks

Shared pieces of the craft apps: PdfCraft, PhotoCraft, VectorCraft, FilmCraft, EffectCraft,
DesignCraft and LightCraft.

- [`DESIGN.md`](DESIGN.md): the design direction every craft app follows. One design of our own,
  consistent across apps; feature parity with the professional apps they target, without copying
  their look.
- [`crates/craft-ui`](crates/craft-ui): that design as code, for egui: tokens, themes and the
  shared components (command palette, Undo toasts, inspector sections, empty states, buttons).

![The shared shell](docs/images/shell-light.png)

MIT or Apache-2.0, at your option.
