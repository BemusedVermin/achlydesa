//! The simulation **substrate**: a cylindrical hex world carrying the
//! geological, climate, and ecosystem fields described in
//! `docs/simulation_details.md`.
//!
//! This is the *skeleton* pass. The topology, double-buffered field storage,
//! minimal world generation, and the `Substrate` trait impl are complete; the
//! per-tick update `Φ` ([`World::evolve`]) currently wires one representative
//! field end to end — `insolation → temperature` — to establish the pattern
//! (time→season, static-field read, the **diffuse** spatial operator, polar
//! neighbour renormalisation, buffer swap). The remaining climate, ecosystem,
//! and disturbance fields slot into the same shape.
//!
//! Module layout (separation of concerns):
//! - [`grid`]   — addressing & adjacency ([`Topology`]) and the [`Buffered`]
//!   double-buffer; no physics.
//! - [`fields`] — the categorical tile qualities (crust, lithology).
//! - [`worldgen`] — minimal initial world (plates → elevation → rock/ore).
//! - [`world`]  — the [`World`] substrate that owns the fields and runs `Φ`.
//! - [`rng`]    — a seedable [`SplitMix64`] so runs are reproducible.

pub mod fields;
pub mod grid;
pub mod rng;
pub mod world;
pub mod worldgen;

pub use config::Params;
pub use grid::{Buffered, Coord, Topology};
pub use rng::SplitMix64;
pub use world::{Interaction, StigConfig, TileView, World};
