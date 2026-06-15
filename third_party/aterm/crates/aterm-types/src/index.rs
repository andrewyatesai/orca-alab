// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Grid indexing types for terminal coordinates.
//!
//! Provides `Line`, `Column`, `Point`, and related types used to address
//! cells in a terminal grid. Extracted from `aterm-alacritty-bridge` to
//! `aterm-types` so any consumer can use grid coordinates without depending
//! on the bridge crate (#3828).

use std::cmp::{max, min};
use std::fmt;
use std::ops::{Add, Sub};

/// Grid dimensions trait for Point/Line arithmetic.
pub trait Dimensions {
    /// Total lines in the buffer (visible + scrollback).
    fn total_lines(&self) -> usize;
    /// Visible screen lines.
    fn screen_lines(&self) -> usize;
    /// Column count.
    fn columns(&self) -> usize;

    /// Index for the last column.
    #[must_use]
    fn last_column(&self) -> Column {
        Column(self.columns().saturating_sub(1))
    }

    /// Topmost line in history.
    #[must_use]
    fn topmost_line(&self) -> Line {
        // Clamp to i32::MAX before negation to prevent silent truncation.
        Line(-(self.history_size().min(i32::MAX as usize) as i32))
    }

    /// Bottommost line in the viewport.
    #[must_use]
    fn bottommost_line(&self) -> Line {
        // Clamp to i32::MAX to prevent silent truncation on extreme sizes.
        Line(self.screen_lines().saturating_sub(1).min(i32::MAX as usize) as i32)
    }

    /// Number of lines in scrollback history.
    #[must_use]
    fn history_size(&self) -> usize {
        self.total_lines().saturating_sub(self.screen_lines())
    }
}

/// Convenience `Dimensions` for `(lines, columns)` tuples
/// used in tests and lightweight grid simulations.
///
/// All lines are visible (no scrollback): `screen_lines() == total_lines()`.
impl Dimensions for (usize, usize) {
    fn total_lines(&self) -> usize {
        self.0
    }

    fn screen_lines(&self) -> usize {
        self.0
    }

    fn columns(&self) -> usize {
        self.1
    }
}

/// Horizontal direction in the terminal grid.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum Direction {
    /// Left direction.
    Left,
    /// Right direction.
    Right,
}

impl Direction {
    /// Reverse the direction.
    #[must_use]
    pub fn opposite(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }
}

/// Side of a cell for selection anchoring.
pub type Side = Direction;

/// Boundary constraints for cursor/search movement.
#[non_exhaustive]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
pub enum Boundary {
    /// Movement bounded by cursor's range (visible area).
    Cursor,
    /// Movement bounded by entire grid including scrollback.
    #[default]
    Grid,
    /// No boundary constraints.
    None,
}

/// Line index in the terminal grid.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Line(pub i32);

impl fmt::Display for Line {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<usize> for Line {
    fn from(value: usize) -> Self {
        Self(value.min(i32::MAX as usize) as i32)
    }
}

impl From<i32> for Line {
    fn from(value: i32) -> Self {
        Self(value)
    }
}

impl Add<i32> for Line {
    type Output = Self;

    fn add(self, rhs: i32) -> Self::Output {
        Self(self.0.saturating_add(rhs))
    }
}

impl Sub<i32> for Line {
    type Output = Self;

    fn sub(self, rhs: i32) -> Self::Output {
        Self(self.0.saturating_sub(rhs))
    }
}

impl Add<usize> for Line {
    type Output = Self;

    fn add(self, rhs: usize) -> Self::Output {
        let rhs_clamped = rhs.min(i32::MAX as usize) as i32;
        Self(self.0.saturating_add(rhs_clamped))
    }
}

impl Sub<usize> for Line {
    type Output = Self;

    fn sub(self, rhs: usize) -> Self::Output {
        let rhs_clamped = rhs.min(i32::MAX as usize) as i32;
        Self(self.0.saturating_sub(rhs_clamped))
    }
}

impl Add<Line> for Line {
    type Output = Self;

    fn add(self, rhs: Line) -> Self::Output {
        Self(self.0.saturating_add(rhs.0))
    }
}

impl Sub<Line> for Line {
    type Output = i32;

    fn sub(self, rhs: Line) -> Self::Output {
        self.0.saturating_sub(rhs.0)
    }
}

impl std::ops::SubAssign<i32> for Line {
    fn sub_assign(&mut self, rhs: i32) {
        self.0 = self.0.saturating_sub(rhs);
    }
}

impl std::ops::AddAssign<i32> for Line {
    fn add_assign(&mut self, rhs: i32) {
        self.0 = self.0.saturating_add(rhs);
    }
}

impl Line {
    /// Clamp a line to the grid boundary.
    ///
    /// For `Boundary::Cursor`, clamps to visible screen area [0, bottommost_line].
    /// For `Boundary::Grid`, clamps to total grid including scrollback [topmost_line, bottommost_line].
    /// For `Boundary::None`, wraps around the grid cyclically.
    pub fn grid_clamp<D: Dimensions>(self, dimensions: &D, boundary: Boundary) -> Self {
        match boundary {
            Boundary::Cursor => max(Line(0), min(dimensions.bottommost_line(), self)),
            Boundary::Grid => {
                let bottommost_line = dimensions.bottommost_line();
                let topmost_line = dimensions.topmost_line();
                max(topmost_line, min(bottommost_line, self))
            }
            Boundary::None => {
                let screen_lines = dimensions.screen_lines().min(i32::MAX as usize) as i32;
                let total_lines = dimensions.total_lines().min(i32::MAX as usize) as i32;

                if total_lines == 0 {
                    return self;
                }

                if self.0 >= screen_lines {
                    let topmost_line = dimensions.topmost_line();
                    let extra = (self.0 - screen_lines) % total_lines;
                    topmost_line + extra
                } else if self.0 < dimensions.topmost_line().0 {
                    let bottommost_line = dimensions.bottommost_line();
                    let extra = (self.0 - screen_lines + 1) % total_lines;
                    bottommost_line + extra
                } else {
                    self
                }
            }
        }
    }
}

/// Column index in the terminal grid.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Column(pub usize);

impl fmt::Display for Column {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<usize> for Column {
    fn from(value: usize) -> Self {
        Self(value)
    }
}

impl Add<usize> for Column {
    type Output = Self;

    fn add(self, rhs: usize) -> Self::Output {
        Self(self.0.saturating_add(rhs))
    }
}

impl Sub<usize> for Column {
    type Output = Self;

    fn sub(self, rhs: usize) -> Self::Output {
        Self(self.0.saturating_sub(rhs))
    }
}

impl Sub<Column> for Column {
    type Output = usize;

    fn sub(self, rhs: Column) -> Self::Output {
        self.0.saturating_sub(rhs.0)
    }
}

impl std::ops::AddAssign<usize> for Column {
    fn add_assign(&mut self, rhs: usize) {
        self.0 = self.0.saturating_add(rhs);
    }
}

impl std::ops::SubAssign<usize> for Column {
    fn sub_assign(&mut self, rhs: usize) {
        self.0 = self.0.saturating_sub(rhs);
    }
}

/// Grid coordinate expressed as line/column.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Point<L = Line, C = Column> {
    /// Line position.
    pub line: L,
    /// Column position.
    pub column: C,
}

impl<L, C> Point<L, C> {
    /// Create a new point.
    #[must_use]
    pub fn new(line: L, column: C) -> Self {
        Self { line, column }
    }
}

impl<L: Ord, C: Ord> Ord for Point<L, C> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self.line.cmp(&other.line) {
            std::cmp::Ordering::Equal => self.column.cmp(&other.column),
            ord => ord,
        }
    }
}

impl<L: Ord, C: Ord> PartialOrd for Point<L, C> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<L: fmt::Display, C: fmt::Display> fmt::Display for Point<L, C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {})", self.line, self.column)
    }
}

impl Point<Line, Column> {
    /// Subtract a number of columns from a point, wrapping to previous lines as needed.
    ///
    /// The result is clamped according to the boundary constraints.
    pub fn sub<D>(mut self, dimensions: &D, boundary: Boundary, rhs: usize) -> Self
    where
        D: Dimensions,
    {
        let cols = dimensions.columns();
        if cols == 0 {
            return self;
        }

        let line_changes = rhs.saturating_add(cols - 1).saturating_sub(self.column.0) / cols;
        self.line -= line_changes.min(i32::MAX as usize) as i32;
        self.column = Column((cols + self.column.0 - rhs % cols) % cols);
        self.grid_clamp(dimensions, boundary)
    }

    /// Add a number of columns to a point, wrapping to next lines as needed.
    ///
    /// The result is clamped according to the boundary constraints.
    pub fn add<D>(mut self, dimensions: &D, boundary: Boundary, rhs: usize) -> Self
    where
        D: Dimensions,
    {
        let cols = dimensions.columns();
        if cols == 0 {
            return self;
        }

        self.line += (rhs.saturating_add(self.column.0) / cols).min(i32::MAX as usize) as i32;
        self.column = Column((self.column.0 + rhs % cols) % cols);
        self.grid_clamp(dimensions, boundary)
    }

    /// Clamp a point to a grid boundary.
    ///
    /// Ensures the point stays within valid grid coordinates according to the
    /// specified boundary constraints.
    pub fn grid_clamp<D>(mut self, dimensions: &D, boundary: Boundary) -> Self
    where
        D: Dimensions,
    {
        let last_column = dimensions.last_column();
        self.column = min(self.column, last_column);

        let topmost_line = dimensions.topmost_line();
        let bottommost_line = dimensions.bottommost_line();

        match boundary {
            Boundary::Cursor if self.line.0 < 0 => Point::new(Line(0), Column(0)),
            Boundary::Grid if self.line < topmost_line => Point::new(topmost_line, Column(0)),
            Boundary::Cursor | Boundary::Grid if self.line > bottommost_line => {
                Point::new(bottommost_line, last_column)
            }
            Boundary::None => {
                self.line = self.line.grid_clamp(dimensions, boundary);
                self
            }
            _ => self,
        }
    }
}

/// Alacritty-style scroll requests.
#[non_exhaustive]
#[derive(Debug, Copy, Clone)]
pub enum Scroll {
    /// Scroll by a delta in lines.
    Delta(i32),
    /// Scroll up by one page.
    PageUp,
    /// Scroll down by one page.
    PageDown,
    /// Scroll to top of history.
    Top,
    /// Scroll to bottom (current output).
    Bottom,
}
