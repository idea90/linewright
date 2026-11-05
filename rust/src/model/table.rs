//! Tables: rows and columns of cells with text, drawn as their borders and centered text.

use super::entities::{HAlign, Shape, TextAlign, VAlign};
use super::geometry::{Point, pt};
use super::index::Bounds;
use super::parts::{Parts, TextSpec};

/// A table's size: its width and height.
pub fn table_size(columns: &[f64], rows: &[f64]) -> (f64, f64) {
    (columns.iter().sum(), rows.iter().sum())
}

pub fn table_bounds(position: Point, columns: &[f64], rows: &[f64]) -> Bounds {
    let (width, height) = table_size(columns, rows);
    Bounds::from_corners(position, position + pt(width, -height))
}

/// The cell holding a point, as row and column, the title row being one cell.
pub fn cell_at(shape: &Shape, point: Point) -> Option<(usize, usize)> {
    let Shape::Table { position, columns, rows, title, .. } = shape else { return None };
    let (x, y) = (point.x - position.x, position.y - point.y);
    let find = |sizes: &[f64], at: f64| {
        let mut edge = 0.0;
        sizes.iter().position(|size| {
            edge += size;
            at < edge
        })
    };
    if x < 0.0 || y < 0.0 {
        return None;
    }
    let (row, column) = (find(rows, y)?, find(columns, x)?);
    Some(if *title && row == 0 { (0, 0) } else { (row, column) })
}

/// What a table draws: its outline, the lines between rows and between columns (not across a title row), and
/// each cell's text centered in it.
pub fn table_parts(position: Point, columns: &[f64], rows: &[f64], cells: &[Vec<String>], text_height: f64, title: bool) -> Parts {
    let (width, height) = table_size(columns, rows);
    let at = |x: f64, y: f64| position + pt(x, -y);
    let mut paths = vec![vec![at(0.0, 0.0), at(width, 0.0), at(width, height), at(0.0, height), at(0.0, 0.0)]];
    let mut top = 0.0;
    for row in &rows[..rows.len().saturating_sub(1)] {
        top += row;
        paths.push(vec![at(0.0, top), at(width, top)]);
    }
    let below_title = if title { rows.first().copied().unwrap_or(0.0) } else { 0.0 };
    let mut left = 0.0;
    for column in &columns[..columns.len().saturating_sub(1)] {
        left += column;
        paths.push(vec![at(left, below_title), at(left, height)]);
    }
    let mut texts = vec![];
    let mut y = 0.0;
    for (r, row_height) in rows.iter().enumerate() {
        let mut x = 0.0;
        for (c, column_width) in columns.iter().enumerate() {
            let spans_all = title && r == 0;
            if spans_all && c > 0 {
                break;
            }
            let text = cells.get(r).and_then(|row| row.get(c)).cloned().unwrap_or_default();
            if !text.is_empty() {
                let middle =
                    if spans_all { at(width / 2.0, y + row_height / 2.0) } else { at(x + column_width / 2.0, y + row_height / 2.0) };
                texts.push(TextSpec {
                    origin: middle,
                    rotation: 0.0,
                    height: if spans_all { text_height * 1.25 } else { text_height },
                    text,
                    align: TextAlign { h: HAlign::Center, v: VAlign::Middle },
                    line_spacing: 1.0,
                    font: String::new(),
                    look: Default::default(),
                    rich: None,
                });
            }
            x += column_width;
        }
        y += row_height;
    }
    Parts { paths, texts, ..Parts::default() }
}

/// The table with its rows and columns made `rows` by `columns`, keeping what fits and the last sizes for new ones.
pub fn resized(shape: &Shape, row_count: usize, column_count: usize) -> Option<Shape> {
    let Shape::Table { position, columns, rows, cells, text_height, title } = shape else { return None };
    if row_count == 0 || column_count == 0 {
        return None;
    }
    let grow = |sizes: &[f64], count: usize| {
        let last = sizes.last().copied().unwrap_or(10.0);
        (0..count).map(|i| sizes.get(i).copied().unwrap_or(last)).collect::<Vec<f64>>()
    };
    let cells = (0..row_count)
        .map(|r| (0..column_count).map(|c| cells.get(r).and_then(|row| row.get(c)).cloned().unwrap_or_default()).collect())
        .collect();
    Some(Shape::Table {
        position: *position,
        columns: grow(columns, column_count),
        rows: grow(rows, row_count),
        cells,
        text_height: *text_height,
        title: *title,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Shape {
        Shape::Table {
            position: pt(0.0, 0.0),
            columns: vec![30.0, 20.0],
            rows: vec![10.0, 8.0, 8.0],
            cells: vec![vec!["Schedule".into(), String::new()], vec!["A".into(), "1".into()], vec!["B".into(), "2".into()]],
            text_height: 2.5,
            title: true,
        }
    }

    #[test]
    fn cells_are_found_by_point_and_the_title_is_one_cell() {
        let table = sample();
        assert_eq!(cell_at(&table, pt(35.0, -12.0)), Some((1, 1)));
        assert_eq!(cell_at(&table, pt(40.0, -5.0)), Some((0, 0)));
        assert_eq!(cell_at(&table, pt(60.0, -5.0)), None);
    }

    #[test]
    fn a_table_draws_its_lines_and_text() {
        let Shape::Table { position, columns, rows, cells, text_height, title } = sample() else { panic!() };
        let parts = table_parts(position, &columns, &rows, &cells, text_height, title);
        assert_eq!(parts.paths.len(), 1 + 2 + 1, "outline, two row lines, one column line");
        assert_eq!(parts.paths[3], vec![pt(30.0, -10.0), pt(30.0, -26.0)], "the column line stops at the title");
        assert_eq!(parts.texts.len(), 5);
        let Some(Shape::Table { rows, cells, .. }) = resized(&sample(), 4, 2) else { panic!() };
        assert_eq!((rows.len(), cells[3].len()), (4, 2));
    }
}
