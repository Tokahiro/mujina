//! Moving between a page's rows with the controller. Pure, so the rules are tested here rather
//! than on a device.

use slint::Model;

use crate::ui::{RowData, RowKind, RowSection};

/// The row `delta` steps from `current` among the rows that are `enabled`. Rows that are
/// switched off are skipped, and the ends are ends: there is nothing past the first or the last
/// row. Without an enabled row in that direction the answer is `current`.
pub fn step(enabled: &[bool], current: i32, delta: i32) -> i32 {
    let len = i32::try_from(enabled.len()).unwrap_or(i32::MAX);
    let usable = |row: i32| {
        usize::try_from(row)
            .ok()
            .and_then(|row| enabled.get(row))
            .copied()
            .unwrap_or(false)
    };
    let direction = delta.signum();
    if direction == 0 {
        return current;
    }
    let mut row = current;
    let mut found = current;
    let mut remaining = delta.abs();
    while remaining > 0 {
        row += direction;
        if row < 0 || row >= len {
            break;
        }
        if usable(row) {
            found = row;
            remaining -= 1;
        }
    }
    found
}

/// Each row of `sections`, in order, and whether the controller can land on it: not in a hidden
/// section, not switched off, and with something to do. A row only to read has nothing, and nor
/// has a button without a label (the agent's while it runs).
pub fn enabled(sections: impl Iterator<Item = RowSection>) -> Vec<bool> {
    sections
        .flat_map(|section| {
            let hidden = section.hidden;
            section
                .rows
                .iter()
                .map(move |row| !hidden && navigable(&row))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn navigable(row: &RowData) -> bool {
    !row.disabled
        && match row.kind {
            RowKind::Info => false,
            RowKind::Button => !row.label.is_empty(),
            _ => true,
        }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use slint::{ModelRc, VecModel};

    use super::*;

    #[test]
    fn moves_one_row_and_stops_at_the_ends() {
        let rows = [true, true, true];
        assert_eq!(step(&rows, 0, 1), 1);
        assert_eq!(step(&rows, 1, -1), 0);
        assert_eq!(step(&rows, 2, 1), 2);
        assert_eq!(step(&rows, 0, -1), 0);
    }

    #[test]
    fn skips_rows_that_are_switched_off() {
        let rows = [true, false, false, true, false];
        assert_eq!(step(&rows, 0, 1), 3);
        assert_eq!(step(&rows, 3, -1), 0);
        // Nothing enabled below the last usable row: stay.
        assert_eq!(step(&rows, 3, 1), 3);
    }

    #[test]
    fn entering_a_page_lands_on_its_first_enabled_row() {
        assert_eq!(step(&[false, true, true], -1, 1), 1);
        assert_eq!(step(&[false, false], -1, 1), -1);
    }

    fn section(rows: Vec<RowData>, hidden: bool) -> RowSection {
        RowSection {
            rows: ModelRc::from(Rc::new(VecModel::from(rows))),
            hidden,
        }
    }

    fn row(kind: RowKind) -> RowData {
        RowData {
            kind,
            ..RowData::default()
        }
    }

    #[test]
    fn rows_to_read_off_or_hidden_are_passed_over() {
        let off = RowData {
            disabled: true,
            ..row(RowKind::Toggle)
        };
        let button = |label: &str| RowData {
            label: label.into(),
            ..row(RowKind::Button)
        };
        let sections = [
            section(vec![row(RowKind::Own), row(RowKind::Info)], false),
            section(vec![off, button("Start it"), button("")], false),
            section(vec![row(RowKind::Copy)], true),
            section(vec![row(RowKind::Text)], false),
        ];
        assert_eq!(
            enabled(sections.into_iter()),
            [true, false, false, true, false, false, true]
        );
    }
}
