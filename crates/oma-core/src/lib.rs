// SPDX-License-Identifier: GPL-3.0-or-later
//! Shared foundation: paths, config, typed errors, run decisions.
//! Full implementations land in plan todos 2 and 14.

pub mod config;
pub mod errors;
pub mod paths;
pub mod run;

pub use errors::{Error, Result};
