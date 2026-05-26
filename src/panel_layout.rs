// navix - panel layout helpers for navigation and preview split logic.
// Copyright (C) 2026 Peter Buch Hansen
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use std::rc::Rc;

pub(crate) fn split_navigation_preview_cols(
    top_area: Rect,
    nav_fullish_mode: bool,
    preview_fullish_mode: bool,
) -> Rc<[Rect]> {
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints(if nav_fullish_mode {
            vec![Constraint::Min(1), Constraint::Length(12)]
        } else if preview_fullish_mode {
            vec![Constraint::Length(12), Constraint::Min(1)]
        } else {
            vec![Constraint::Percentage(30), Constraint::Percentage(70)]
        })
        .split(top_area)
}
