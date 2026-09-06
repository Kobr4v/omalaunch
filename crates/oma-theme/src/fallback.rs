// SPDX-License-Identifier: GPL-3.0-or-later
//! Neutral fallback values used when staged theme keys are missing.
//! This is the ONLY non-fixture location allowed to contain color literals.

/// Fallback accent. Only used when the staged theme omits `accent`.
pub const FALLBACK_ACCENT: &str = "#888888";
/// Fallback background. Only used when the staged theme omits `background`.
pub const FALLBACK_BACKGROUND: &str = "#1e1e1e";
/// Fallback foreground. Only used when the staged theme omits `foreground`.
pub const FALLBACK_FOREGROUND: &str = "#e0e0e0";
/// Fallback selection. Only used when the staged theme omits `selection`.
pub const FALLBACK_SELECTION: &str = "#3a3a3a";
/// Fallback muted. Only used when the staged theme omits `muted`.
pub const FALLBACK_MUTED: &str = "#6e6e6e";
/// Fallback danger. Only used when the staged theme omits `red`.
pub const FALLBACK_RED: &str = "#c00000";
/// Fallback warning. Only used when the staged theme omits `yellow`.
pub const FALLBACK_YELLOW: &str = "#c0a000";
/// Fallback success. Only used when the staged theme omits `green`.
pub const FALLBACK_GREEN: &str = "#00a000";
