//! Library surface of the `app` crate — currently just the shared UI [`theme`], exposed so the
//! `ui_gallery` example (and tests) can build widgets with the exact same tokens the live HUD
//! uses. The playable binary lives in `main.rs`; everything game-specific stays there.

pub mod theme;
