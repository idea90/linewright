//! Finds the entities near a spot fast, however many there are.
//!
//! Entities are filed in a grid of cells. A longer object than `MAX_CELLS_PER_ENTITY` cells is checked on every
//! query instead of being filed. The cell size follows the drawing, so a few stray objects far from the rest do not
//! stretch it.

use std::collections::HashMap;

use super::dimension::dimension_parts;
use super::entities::{Entity, EntityId, Shape};
pub use super::geometry::Bounds;
use super::geometry::{Point, bulge_arc, pt};
use super::parts::entity_parts;
use super::transform::insert_transform;
use super::viewport::frame_corners;

pub const DEFAULT_CELL_SIZE: f64 = 1000.0;
const CELLS_ACROSS: f64 = 24.0; // how many cells the middle of a drawing is split into each way
const OUTLIER_FRACTION: f64 = 0.01;
const MIN_CELL_SIZE: f64 = 1e-6;
const MAX_CELLS_PER_ENTITY: i64 = 64;
pub const TEXT_DESCENT: f64 = 0.3; // of the text height, below the baseline

/// A generous box around text. The real width depends on the font, so each character gets a full height.
pub fn text_extent(origin: Point, rotation: f64, height: f64, length: usize) -> Bounds {
    let width = height * length.max(1) as f64;
    let (sin, cos) = rotation.to_radians().sin_cos();
    let corners = [(0.0, -TEXT_DESCENT * height), (width, -TEXT_DESCENT * height), (width, height), (0.0, height)];
    Bounds::around(corners.map(|(u, v)| pt(origin.x + u * cos - v * sin, origin.y + u * sin + v * cos)))
}

/// A box that holds the whole entity. It may be larger than needed, for arcs and text, never smaller.
pub fn entity_extent(entity: &Entity) -> Bounds {
    match &entity.shape {
        Shape::Line { start, end } => Bounds::around([*start, *end]),
        Shape::Circle { center, radius } | Shape::Arc { center, radius, .. } => Bounds::square_around(*center, *radius),
        Shape::Polyline { vertices, .. } => {
            let mut points: Vec<Point> = vertices.iter().map(|v| v.point()).collect();
            for (index, v) in vertices.iter().enumerate() {
                let following = vertices[(index + 1) % vertices.len()];
                if v.bulge != 0.0 && following.point() != v.point() {
                    let arc = bulge_arc(v.point(), following.point(), v.bulge);
                    points.push(arc.center - pt(arc.radius, arc.radius));
                    points.push(arc.center + pt(arc.radius, arc.radius));
                }
            }
            Bounds::around(points)
        }
        Shape::Text { .. } => {
            // Every character a full height wide: generous, since the real width depends on the font.
            let parts = entity_parts(entity, 1.0);
            Bounds::around(parts.texts.iter().flat_map(|t| t.bounding_box(1.0)))
        }
        Shape::Dimension { start, end, line_point, angle, look } => {
            let parts = dimension_parts(*start, *end, *line_point, *angle, look);
            let mut points: Vec<Point> = parts.lines().flat_map(|(a, b)| [*a, *b]).collect();
            points.extend(parts.arrows.iter().flatten());
            let length = parts.text.chars().count();
            let half = length as f64 * parts.text_height / 2.0;
            let reading = parts.text_angle.to_radians();
            let origin = parts.text_anchor - pt(half * reading.cos(), half * reading.sin());
            let stack = parts.stack.iter().map(|t| {
                let count = t.text.chars().count();
                let half = count as f64 * t.height / 2.0;
                let reading = t.angle.to_radians();
                text_extent(t.anchor - pt(half * reading.cos(), half * reading.sin()), t.angle, t.height, count)
            });
            stack.fold(Bounds::around(points).union(text_extent(origin, parts.text_angle, parts.text_height, length)), Bounds::union)
        }
        Shape::Viewport { center, width, height, .. } => Bounds::around(frame_corners(*center, *width, *height)),
        Shape::Image { position, u, v, .. } => Bounds::around(crate::model::entities::image_corners(*position, *u, *v)),
        Shape::Table { position, columns, rows, .. } => crate::model::table::table_bounds(*position, columns, rows),
        Shape::XLine { base, direction, ray } => {
            let (start, end) = crate::model::xline::far_ends(*base, *direction, *ray);
            Bounds::around([start, end])
        }
        Shape::Annotation { .. } | Shape::Multileader { .. } => {
            // Lines and arrows exactly; text as text, each character a full height wide.
            let parts = entity_parts(entity, f64::INFINITY);
            let points = parts.paths.iter().flatten().chain(parts.fills.iter().flatten()).copied();
            parts.texts.iter().flat_map(|t| t.bounding_box(1.0)).fold(Bounds::around(points), |b, p| b.union(Bounds::around([p])))
        }
        Shape::Ellipse { center, major, ratio, .. } => {
            // The whole ellipse's box: half widths along each axis from both semi-axes.
            let minor = pt(-major.y, major.x) * *ratio;
            let half = pt(major.x.hypot(minor.x), major.y.hypot(minor.y));
            Bounds::around([*center - half, *center + half])
        }
        // A NURBS curve stays inside the hull of its control points.
        Shape::Spline { control, .. } => Bounds::around(control.iter().copied()),
        Shape::Point { position } => Bounds::around([*position]),
        Shape::Solid { corners } | Shape::Wipeout { corners } => Bounds::around(corners.iter().copied()),
        Shape::Mline { vertices, closed, justification, scale, style, gaps } => {
            let paths = crate::model::mline::mline_paths(vertices, *closed, *justification, *scale, style, gaps);
            Bounds::around(paths.into_iter().flat_map(|(_, path)| path).chain(vertices.iter().copied()))
        }
        Shape::Hatch { loops, .. } => Bounds::around(loops.iter().flatten().copied()),
        Shape::Insert { block, position, scale_x, scale_y, rotation, attributes } => {
            let placed = insert_transform(block.0.base, *position, *scale_x, *scale_y, *rotation);
            let mut corners: Vec<Point> = vec![*position];
            for child in &block.0.entities {
                let b = entity_extent(child);
                if b.x0.is_finite() {
                    corners.extend([pt(b.x0, b.y0), pt(b.x1, b.y0), pt(b.x1, b.y1), pt(b.x0, b.y1)].map(|c| placed.point(c)));
                }
            }
            let mut bounds = Bounds::around(corners);
            for attribute in attributes.iter().filter(|a| !a.invisible) {
                bounds = bounds.union(entity_extent(&Entity::new(attribute.text.clone())));
            }
            bounds
        }
    }
}

type Cell = (i64, i64);

struct Record {
    order: u64, // drawing order, lowest first
    bounds: Bounds,
    cells: Option<Vec<Cell>>, // None when it is too long to file
}

pub struct SpatialIndex {
    pub cell_size: f64,
    records: HashMap<EntityId, Record>,
    cells: HashMap<Cell, Vec<EntityId>>,
    oversized: Vec<EntityId>,
    next_order: u64,
}

impl Default for SpatialIndex {
    fn default() -> Self {
        SpatialIndex { cell_size: DEFAULT_CELL_SIZE, records: HashMap::new(), cells: HashMap::new(), oversized: vec![], next_order: 0 }
    }
}

fn middle_span(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    let cut = (values.len() as f64 * OUTLIER_FRACTION) as usize;
    let trimmed = if values.len() > 2 * cut { &values[cut..values.len() - cut] } else { &values[..] };
    trimmed[trimmed.len() - 1] - trimmed[0]
}

/// How many cells lie from the first to the last, both counted. Coordinates far beyond any drawing land in the
/// outermost cells an i64 can name, so the count stops at the largest number rather than wrapping round.
fn cells_spanned(first: i64, last: i64) -> i64 {
    last.saturating_sub(first).saturating_add(1)
}

impl SpatialIndex {
    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn contains(&self, id: EntityId) -> bool {
        self.records.contains_key(&id)
    }

    pub fn bounds_of(&self, id: EntityId) -> Option<Bounds> {
        self.records.get(&id).map(|r| r.bounds)
    }

    pub fn order_of(&self, id: EntityId) -> u64 {
        self.records.get(&id).map_or(u64::MAX, |r| r.order)
    }

    pub fn reload<'a>(&mut self, entities: impl IntoIterator<Item = &'a Entity>) {
        let measured: Vec<(EntityId, Bounds)> = entities.into_iter().map(|e| (e.id, entity_extent(e).finite())).collect();
        *self = SpatialIndex::default();
        self.cell_size = Self::cell_size_for(&measured);
        for (id, bounds) in measured {
            self.file(id, bounds);
        }
    }

    pub fn add(&mut self, entity: &Entity) {
        self.file(entity.id, entity_extent(entity).finite());
    }

    pub fn remove(&mut self, id: EntityId) {
        if let Some(record) = self.records.remove(&id) {
            self.unfile(id, &record);
        }
    }

    /// Pick up changes made to an entity in place. It keeps its place in the drawing order.
    pub fn refresh(&mut self, entity: &Entity) {
        let Some(mut record) = self.records.remove(&entity.id) else { return };
        self.unfile(entity.id, &record);
        record.bounds = entity_extent(entity).finite();
        record.cells = self.cells_for(&record.bounds);
        self.place(entity.id, &record);
        self.records.insert(entity.id, record);
    }

    /// The entities whose box touches the area, in drawing order.
    pub fn query(&self, area: &Bounds) -> Vec<EntityId> {
        let size = self.cell_size;
        let columns = (area.x0 / size).floor() as i64..=(area.x1 / size).floor() as i64;
        let rows = (area.y0 / size).floor() as i64..=(area.y1 / size).floor() as i64;
        let cell_count = cells_spanned(*columns.start(), *columns.end()).saturating_mul(cells_spanned(*rows.start(), *rows.end()));
        let mut found: Vec<EntityId> = if cell_count > self.cells.len() as i64 {
            self.records.keys().copied().collect()
        } else {
            let mut ids: Vec<EntityId> = Vec::new();
            for column in columns {
                for row in rows.clone() {
                    if let Some(members) = self.cells.get(&(column, row)) {
                        ids.extend(members);
                    }
                }
            }
            ids.extend(&self.oversized);
            ids.sort_unstable();
            ids.dedup();
            ids
        };
        found.retain(|id| self.records[id].bounds.touches(area));
        found.sort_by_key(|id| self.records[id].order);
        found
    }

    /// All ids with their boxes, in drawing order.
    pub fn all(&self) -> Vec<(EntityId, Bounds)> {
        let mut all: Vec<(u64, EntityId, Bounds)> = self.records.iter().map(|(id, r)| (r.order, *id, r.bounds)).collect();
        all.sort_by_key(|(order, _, _)| *order);
        all.into_iter().map(|(_, id, bounds)| (id, bounds)).collect()
    }

    fn cell_size_for(boxes: &[(EntityId, Bounds)]) -> f64 {
        if boxes.is_empty() {
            return DEFAULT_CELL_SIZE;
        }
        let span = middle_span(boxes.iter().map(|(_, b)| (b.x0 + b.x1) / 2.0).collect())
            .max(middle_span(boxes.iter().map(|(_, b)| (b.y0 + b.y1) / 2.0).collect()));
        if span > 0.0 { (span / CELLS_ACROSS).max(MIN_CELL_SIZE) } else { DEFAULT_CELL_SIZE }
    }

    fn cells_for(&self, bounds: &Bounds) -> Option<Vec<Cell>> {
        let size = self.cell_size;
        let (c0, c1) = ((bounds.x0 / size).floor() as i64, (bounds.x1 / size).floor() as i64);
        let (r0, r1) = ((bounds.y0 / size).floor() as i64, (bounds.y1 / size).floor() as i64);
        if cells_spanned(c0, c1).saturating_mul(cells_spanned(r0, r1)) > MAX_CELLS_PER_ENTITY {
            return None;
        }
        Some((c0..=c1).flat_map(|c| (r0..=r1).map(move |r| (c, r))).collect())
    }

    fn file(&mut self, id: EntityId, bounds: Bounds) {
        let record = Record { order: self.next_order, cells: self.cells_for(&bounds), bounds };
        self.next_order += 1;
        if let Some(old) = self.records.remove(&id) {
            self.unfile(id, &old);
        }
        self.place(id, &record);
        self.records.insert(id, record);
    }

    fn place(&mut self, id: EntityId, record: &Record) {
        match &record.cells {
            None => self.oversized.push(id),
            Some(cells) => {
                for cell in cells {
                    self.cells.entry(*cell).or_default().push(id);
                }
            }
        }
    }

    fn unfile(&mut self, id: EntityId, record: &Record) {
        match &record.cells {
            None => self.oversized.retain(|other| *other != id),
            Some(cells) => {
                for cell in cells {
                    if let Some(members) = self.cells.get_mut(cell) {
                        members.retain(|other| *other != id);
                        if members.is_empty() {
                            self.cells.remove(cell);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::entities::{circle, line};

    #[test]
    fn finds_entities_near_an_area_in_drawing_order() {
        let entities: Vec<Entity> = (0..1000).map(|i| line(pt(i as f64 * 10.0, 0.0), pt(i as f64 * 10.0 + 5.0, 5.0))).collect();
        let mut index = SpatialIndex::default();
        index.reload(&entities);
        let found = index.query(&Bounds { x0: 99.0, y0: 0.0, x1: 121.0, y1: 1.0 });
        assert_eq!(found, vec![entities[10].id, entities[11].id, entities[12].id]);
    }

    #[test]
    fn long_objects_are_found_everywhere_along_them() {
        let mut entities: Vec<Entity> = (0..100).map(|i| circle(pt(i as f64, 0.0), 0.5)).collect();
        entities.push(line(pt(-1e6, 0.0), pt(1e6, 0.0)));
        let mut index = SpatialIndex::default();
        index.reload(&entities);
        let far = index.query(&Bounds::square_around(pt(5e5, 0.0), 1.0));
        assert_eq!(far, vec![entities[100].id]);
    }

    #[test]
    fn coordinates_beyond_the_grid_neither_wrap_nor_stall() {
        let mut entities: Vec<Entity> = (0..10).map(|i| circle(pt(i as f64, 0.0), 0.5)).collect();
        entities.push(line(pt(-1e9, 1e9), pt(1e308, 1e308)));
        let mut index = SpatialIndex::default();
        index.reload(&entities);
        let everywhere = Bounds { x0: -1e300, y0: -1e300, x1: 1e300, y1: 1e300 };
        assert_eq!(index.query(&everywhere).len(), entities.len());
        let near = index.query(&Bounds::square_around(pt(3.0, 0.0), 0.1));
        assert!(near.contains(&entities[3].id) && !near.contains(&entities[7].id), "{near:?}");
    }

    #[test]
    fn refresh_moves_an_entity_and_keeps_its_order() {
        let mut first = line(pt(0.0, 0.0), pt(1.0, 0.0));
        let second = line(pt(0.0, 1.0), pt(1.0, 1.0));
        let mut index = SpatialIndex::default();
        index.reload([&first, &second]);
        first.shape = Shape::Line { start: pt(50.0, 50.0), end: pt(51.0, 50.0) };
        index.refresh(&first);
        assert!(index.query(&Bounds::square_around(pt(0.0, 0.0), 0.5)).is_empty());
        assert_eq!(index.query(&Bounds::square_around(pt(50.0, 50.0), 1.0)), vec![first.id]);
        assert!(index.order_of(first.id) < index.order_of(second.id));
    }
}
