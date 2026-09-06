// SPDX-License-Identifier: GPL-3.0-or-later
//! SQLite app library: apps, collections, tags, play state.
//! Covers live only as filesystem paths — never blobs, never network.

pub mod covers;
pub mod db;
pub mod import;
pub mod query;

pub use db::{App, Library};
pub use query::{Filter, SortOrder};
