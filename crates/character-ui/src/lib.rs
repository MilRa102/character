//! Drawer UI: the layer-shell surface with the character, tooltip and animations.
//!
//! Planned for MVP-0 on top of libcosmic/iced and `zwlr_layer_shell_v1`.
//! libcosmic is not published on crates.io, so it will be added as a git dependency
//! together with the system packages it needs (Wayland, xkbcommon, ...), both in
//! `.github/workflows` and in the AppImage build.
//!
//! The UI should depend only on `character-types` and subscribe to state changes.
