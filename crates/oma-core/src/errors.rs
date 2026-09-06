// SPDX-License-Identifier: GPL-3.0-or-later
//! Typed errors for the whole workspace.

use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("I/O error at {0}: {1}")]
    Io(PathBuf, #[source] std::io::Error),
    #[error("config error: {0}")]
    Config(String),
    #[error("not an AppImage: {0}")]
    NotAppImage(PathBuf),
    #[error("unsupported AppImage type: {0}")]
    UnsupportedType(String),
    #[error("integration error: {0}")]
    Integration(String),
    #[error("theme unavailable: {0}")]
    Theme(String),
    #[error("update error: {0}")]
    Update(String),
    #[error("not registered: {0}")]
    NotRegistered(PathBuf),
}

pub type Result<T> = std::result::Result<T, Error>;
