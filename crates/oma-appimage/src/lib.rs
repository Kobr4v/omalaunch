// SPDX-License-Identifier: GPL-3.0-or-later
//! AppImage parsing (todos 5-6 implement).

pub mod extract;
pub mod inspect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppImageType {
    Type1,
    Type2,
}
