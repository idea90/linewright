//! Linewright: a 2D CAD program in the style of AutoCAD.
//!
//! Everything but `ui` and the drawing part of `canvas` is free of the GUI, so it can be tested headless.

pub mod canvas;
pub mod commands;
pub mod fonts;
pub mod io;
pub mod mcp;
pub mod model;
pub mod selection;
pub mod shx;
pub mod snap;
pub mod threads;
pub mod ui;
pub mod update;
