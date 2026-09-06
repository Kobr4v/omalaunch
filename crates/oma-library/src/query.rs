// SPDX-License-Identifier: GPL-3.0-or-later
//! Filtered, sorted query shapes over the library.
//! Execution lives in `db.rs` (it owns the connection).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortOrder {
    #[default]
    UpdateFirst,
    Name,
    Recent,
    Played,
}

#[derive(Debug, Clone, Default)]
pub struct Filter {
    pub text: String,
    pub favorites_only: bool,
    pub show_hidden: bool,
    pub collection: Option<i64>,
    pub tag: Option<String>,
    pub updates_only: bool,
    pub sort: SortOrder,
}
