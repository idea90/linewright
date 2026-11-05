//! POLYGON, MTEXT, DIVIDE and MEASURE.

use std::f64::consts::PI;

use super::draw::{DEFAULT_TEXT_HEIGHT, line_preview};
use super::format_general;
use super::prompts::{Answer, Io, Prompt, PromptKind, Step};
use crate::model::entities::{Entity, HAlign, Shape, TextAlign, VAlign, point, polyline_from};
use crate::model::geometry::{Point, pt};
use crate::model::index::entity_extent;
use crate::model::parts::entity_parts;

const EDGE: &str = "E";
const INSCRIBED: &str = "I";
const CIRCUMSCRIBED: &str = "C";
const MAX_SIDES: f64 = 1024.0;

/// A regular polygon: `sides` corners around `center`, the first at `first`.
fn polygon(center: Point, first: Point, sides: usize) -> Entity {
    let (radius, start) = (center.dist(first), (first.y - center.y).atan2(first.x - center.x));
    let corners: Vec<Point> = (0..sides)
        .map(|i| {
            let a = start + 2.0 * PI * i as f64 / sides as f64;
            center + pt(a.cos(), a.sin()) * radius
        })
        .collect();
    polyline_from(&corners, true)
}

/// The polygon's first corner for the cursor: on it when inscribed in the circle through it, half a side round
/// from it, further out, when circumscribed about it (the cursor is then the middle of the first side).
fn first_corner(center: Point, cursor: Point, sides: usize, inscribed: bool) -> Point {
    if inscribed {
        return cursor;
    }
    let half = PI / sides as f64;
    let (radius, angle) = (center.dist(cursor) / half.cos(), (cursor.y - center.y).atan2(cursor.x - center.x) - half);
    center + pt(angle.cos(), angle.sin()) * radius
}

/// POLYGON: 3 to 1024 equal sides, inscribed in or circumscribed about a circle, or built on one edge.
pub async fn polygon_command(io: Io) {
    let prompt = Prompt::new(PromptKind::Number, "Enter number of sides <4>:").default(Answer::Number(4.0));
    let Some(Answer::Number(count)) = io.ask(prompt).await else { return };
    if count.fract() != 0.0 || !(3.0..=MAX_SIDES).contains(&count) {
        io.step(Step::Report("Enter a whole number of sides from 3 to 1024.".into()));
        return;
    }
    let sides = count as usize;
    match io.ask(Prompt::point("Specify center of polygon or [Edge]:").options(&[EDGE])).await {
        Some(Answer::Point(center)) => {
            let prompt = Prompt::point("Enter an option [Inscribed in circle/Circumscribed about circle] <I>:")
                .options(&[INSCRIBED, CIRCUMSCRIBED])
                .default(Answer::keyword(INSCRIBED));
            let inscribed = !matches!(io.ask(prompt).await, Some(answer) if answer.is(CIRCUMSCRIBED));
            let shape = move |cursor: Point| polygon(center, first_corner(center, cursor, sides, inscribed), sides);
            let prompt = Prompt::point("Specify radius of circle:").base(center).preview(move |cursor| vec![shape(cursor)]);
            if let Some(Answer::Point(cursor)) = io.ask(prompt).await
                && cursor != center
            {
                io.step(Step::Drawn(vec![polygon(center, first_corner(center, cursor, sides, inscribed), sides)]));
            }
        }
        Some(answer) if answer.is(EDGE) => {
            let Some(Answer::Point(a)) = io.ask(Prompt::point("Specify first endpoint of edge:")).await else { return };
            // The polygon lies to the left of the edge from its first end to its second.
            let on_edge = move |b: Point| {
                let half = PI / sides as f64;
                let middle = a.midpoint(b);
                let inward = pt(-(b.y - a.y), b.x - a.x) * (1.0 / (2.0 * half.tan()) / a.dist(b).max(f64::MIN_POSITIVE));
                let center = middle + inward * a.dist(b);
                polygon(center, a, sides)
            };
            let prompt = Prompt::point("Specify second endpoint of edge:").base(a).preview(move |cursor| vec![on_edge(cursor)]);
            if let Some(Answer::Point(b)) = io.ask(prompt).await
                && b != a
            {
                io.step(Step::Drawn(vec![on_edge(b)]));
            }
        }
        _ => {}
    }
}

fn frame(first: Point, second: Point) -> Entity {
    polyline_from(&[first, pt(second.x, first.y), second, pt(first.x, second.y)], true)
}

/// MTEXT: a frame for the text, its height, then its lines until an empty one. The text hangs from the frame's
/// top left corner, as AutoCAD's does by default.
/// MTEXT from the window: two corners, then its editor opens.
pub async fn mtext_box_command(io: Io, defaults: std::rc::Rc<std::cell::RefCell<super::defaults::Defaults>>) {
    let Some(Answer::Point(first)) = io.ask(Prompt::point("Specify first corner:")).await else { return };
    let prompt = Prompt::point("Specify opposite corner:").base(first).preview(move |cursor| vec![frame(first, cursor)]);
    let Some(Answer::Point(second)) = io.ask(prompt).await else { return };
    defaults.borrow_mut().mtext_request = Some(pt(first.x.min(second.x), first.y.max(second.y)));
}

pub async fn mtext_command(io: Io) {
    let Some(Answer::Point(first)) = io.ask(Prompt::point("Specify first corner:")).await else { return };
    let prompt = Prompt::point("Specify opposite corner:").base(first).preview(move |cursor| vec![frame(first, cursor)]);
    let Some(Answer::Point(second)) = io.ask(prompt).await else { return };
    let prompt = Prompt::new(PromptKind::Number, format!("Specify height <{}>:", format_general(DEFAULT_TEXT_HEIGHT)))
        .default(Answer::Number(DEFAULT_TEXT_HEIGHT));
    let Some(Answer::Number(height)) = io.ask(prompt).await else { return };
    let mut lines: Vec<String> = vec![];
    loop {
        let message = if lines.is_empty() { "Enter text:" } else { "Enter next line, or press Enter to finish:" };
        match io.ask(Prompt::new(PromptKind::Text, message).default(Answer::Text(String::new()))).await {
            Some(Answer::Text(line)) if !line.is_empty() => lines.push(line),
            Some(_) => break,
            None => return,
        }
    }
    if lines.is_empty() {
        return;
    }
    let top_left = pt(first.x.min(second.x), first.y.max(second.y));
    io.step(Step::Drawn(vec![Entity::new(Shape::Text {
        position: top_left,
        text: lines.join("\n"),
        height,
        rotation: 0.0,
        font: String::new(),
        style: String::new(),
        align: TextAlign { h: HAlign::Left, v: VAlign::Top },
        line_spacing: 1.0,
        formatted: String::new(),
        width_factor: 1.0,
        oblique: 0.0,
    })]));
}

/// The object as one run of points, start to end, and whether it closes on itself. Lines, arcs, circles,
/// polylines, ellipses and splines have one; other objects have none.
fn path_of(entity: &Entity) -> Option<(Vec<Point>, bool)> {
    let closed = match &entity.shape {
        Shape::Line { .. } | Shape::Arc { .. } => false,
        Shape::Circle { .. } => true,
        Shape::Polyline { closed, .. } | Shape::Spline { closed, .. } => *closed,
        Shape::Ellipse { start, end, .. } => (end - start).abs() >= 2.0 * PI - 1e-9,
        _ => return None,
    };
    let tolerance = entity_extent(entity).diagonal() * 1e-5;
    let path = entity_parts(entity, tolerance.max(f64::MIN_POSITIVE)).paths.into_iter().next()?;
    (path.len() >= 2).then_some((path, closed))
}

fn length_of(path: &[Point]) -> f64 {
    path.windows(2).map(|w| w[0].dist(w[1])).sum()
}

/// The points at these distances along the path, in order.
fn points_along(path: &[Point], distances: &[f64]) -> Vec<Point> {
    let mut found = vec![];
    let mut walked = 0.0;
    let mut wanted = distances.iter().peekable();
    for w in path.windows(2) {
        let step = w[0].dist(w[1]);
        while let Some(&&d) = wanted.peek() {
            if d > walked + step || step == 0.0 {
                break;
            }
            found.push(w[0] + (w[1] - w[0]) * ((d - walked) / step));
            wanted.next();
        }
        walked += step;
    }
    found
}

async fn object_with_path(io: &Io, message: &str) -> Option<(Vec<Point>, bool)> {
    loop {
        let Some(Answer::Picked(picked)) = io.ask(Prompt::new(PromptKind::Pick, message)).await else { return None };
        match path_of(&picked.entity) {
            Some(path) => return Some(path),
            None => io.step(Step::Report("Pick a line, arc, circle, polyline, ellipse or spline.".into())),
        }
    }
}

/// DIVIDE: point objects splitting an object into equal parts.
pub async fn divide_command(io: Io) {
    let Some((path, closed)) = object_with_path(&io, "Select object to divide:").await else { return };
    let Some(Answer::Number(count)) = io.ask(Prompt::new(PromptKind::Number, "Enter the number of segments:")).await else { return };
    if count.fract() != 0.0 || !(2.0..=32767.0).contains(&count) {
        io.step(Step::Report("Enter a whole number of segments from 2 to 32767.".into()));
        return;
    }
    let (total, parts) = (length_of(&path), count as usize);
    // A closed object has a point where it starts, too; an open one has its ends already.
    let distances: Vec<f64> = (if closed { 0..parts } else { 1..parts }).map(|i| total * i as f64 / parts as f64).collect();
    io.step(Step::Drawn(points_along(&path, &distances).into_iter().map(point).collect()));
}

/// MEASURE: point objects at a set length along an object, from its start.
pub async fn measure_command(io: Io) {
    let Some((path, _)) = object_with_path(&io, "Select object to measure:").await else { return };
    let Some(Answer::Number(step)) =
        io.ask(Prompt::new(PromptKind::Number, "Specify length of segment:").preview(line_preview(path[0]))).await
    else {
        return;
    };
    let total = length_of(&path);
    let distances: Vec<f64> = (1..).map(|i| step * i as f64).take_while(|d| *d < total - 1e-9 * total).take(32767).collect();
    let points = points_along(&path, &distances);
    let count = points.len();
    io.step(Step::Drawn(points.into_iter().map(point).collect()));
    io.step(Step::Report(format!("{count} point(s) placed.")));
}

const HORIZONTAL: &str = "H";
const VERTICAL: &str = "V";
const ANGLE: &str = "A";

fn xline(base: Point, through: Point, ray: bool) -> Option<Entity> {
    let toward = through - base;
    let length = toward.length();
    (length > 0.0).then(|| Entity::new(Shape::XLine { base, direction: toward * (1.0 / length), ray }))
}

/// Construction lines through one point, as many as wanted, each along the direction asked for.
async fn xlines_through(io: &Io, direction: Point) {
    loop {
        let shown = move |cursor: Point| vec![Entity::new(Shape::XLine { base: cursor, direction, ray: false })];
        let Some(Answer::Point(through)) = io.ask(Prompt::point("Specify through point:").preview(shown)).await else { return };
        io.step(Step::Drawn(vec![Entity::new(Shape::XLine { base: through, direction, ray: false })]));
    }
}

/// XLINE: endless construction lines through a point and each point given after, or Horizontal, Vertical or at
/// an Angle through each point given.
pub async fn xline_command(io: Io) {
    let prompt = Prompt::point("Specify a point or [Hor/Ver/Ang]:").options(&[HORIZONTAL, VERTICAL, ANGLE]);
    match io.ask(prompt).await {
        Some(Answer::Point(base)) => loop {
            let prompt =
                Prompt::point("Specify through point:").base(base).preview(move |cursor| xline(base, cursor, false).into_iter().collect());
            let Some(Answer::Point(through)) = io.ask(prompt).await else { return };
            if let Some(line) = xline(base, through, false) {
                io.step(Step::Drawn(vec![line]));
            }
        },
        Some(answer) if answer.is(HORIZONTAL) => xlines_through(&io, pt(1.0, 0.0)).await,
        Some(answer) if answer.is(VERTICAL) => xlines_through(&io, pt(0.0, 1.0)).await,
        Some(answer) if answer.is(ANGLE) => {
            let Some(Answer::Number(angle)) =
                io.ask(Prompt::new(PromptKind::Angle, "Enter angle of xline (0):").default(Answer::Number(0.0))).await
            else {
                return;
            };
            xlines_through(&io, crate::model::geometry::direction(angle)).await;
        }
        _ => {}
    }
}

/// RAY: construction lines from one start point through each point given after.
pub async fn ray_command(io: Io) {
    let Some(Answer::Point(start)) = io.ask(Prompt::point("Specify start point:")).await else { return };
    loop {
        let prompt =
            Prompt::point("Specify through point:").base(start).preview(move |cursor| xline(start, cursor, true).into_iter().collect());
        let Some(Answer::Point(through)) = io.ask(prompt).await else { return };
        if let Some(ray) = xline(start, through, true) {
            io.step(Step::Drawn(vec![ray]));
        }
    }
}

fn ring(center: Point, diameter: f64) -> Vec<Point> {
    (0..72).map(|i| center + crate::model::geometry::direction(i as f64 * 5.0) * (diameter / 2.0)).collect()
}

/// A filled ring, or a filled dot when there is no hole.
fn donut(center: Point, inside: f64, outside: f64) -> Entity {
    let mut loops = vec![ring(center, outside)];
    if inside > 0.0 {
        loops.push(ring(center, inside));
    }
    Entity::new(Shape::Hatch {
        loops,
        solid: true,
        pattern: vec![],
        name: "SOLID".into(),
        scale: 1.0,
        angle: 0.0,
        gradient: None,
        association: None,
    })
}

/// DONUT: filled rings of a given inside and outside diameter, at as many centers as wanted.
pub async fn donut_command(io: Io) {
    let prompt = Prompt::new(PromptKind::Length, "Specify inside diameter of donut <0.5>:").default(Answer::Number(0.5));
    let Some(Answer::Number(inside)) = io.ask(prompt).await else { return };
    let prompt = Prompt::new(PromptKind::Number, "Specify outside diameter of donut <1>:").default(Answer::Number(1.0));
    let Some(Answer::Number(outside)) = io.ask(prompt).await else { return };
    if inside >= outside {
        io.step(Step::Report("The outside diameter must be larger than the inside one.".into()));
        return;
    }
    loop {
        let prompt = Prompt::point("Specify center of donut or <exit>:").preview(move |cursor| vec![donut(cursor, inside, outside)]);
        let Some(Answer::Point(center)) = io.ask(prompt).await else { return };
        io.step(Step::Drawn(vec![donut(center, inside, outside)]));
    }
}

const ARC_LENGTH: &str = "A";
const OBJECT: &str = "O";
/// How strongly each cloud arc bows out: about 110 degrees of arc, as AutoCAD's do.
const CLOUD_BULGE: f64 = 0.52;

/// A revision cloud along a closed outline: arcs of about `arc` chord each, bowing outward.
pub fn cloud(outline: &[Point], arc: f64) -> Option<Entity> {
    let mut closed: Vec<Point> = outline.to_vec();
    if closed.first() != closed.last() {
        closed.push(closed[0]);
    }
    let total = length_of(&closed);
    let count = ((total / arc).round() as usize).max(3);
    let distances: Vec<f64> = (0..count).map(|i| total * i as f64 / count as f64).collect();
    let points = points_along(&closed, &distances);
    let area: f64 = closed.windows(2).map(|w| w[0].x * w[1].y - w[1].x * w[0].y).sum();
    // A positive bulge turns counter-clockwise, which along a counter-clockwise outline bows out of it.
    let bulge = if area > 0.0 { CLOUD_BULGE } else { -CLOUD_BULGE };
    let vertices =
        points.into_iter().map(|p| crate::model::entities::Vertex { x: p.x, y: p.y, bulge, start_width: 0.0, end_width: 0.0 }).collect();
    Some(Entity::new(Shape::Polyline { vertices, closed: true }))
}

/// REVCLOUD: a revision cloud around a rectangle given by two corners, or along a closed object, which it
/// replaces. The arcs' length can be set first.
pub async fn revcloud_command(io: Io, defaults: std::rc::Rc<std::cell::RefCell<super::defaults::Defaults>>) {
    loop {
        let arc = defaults.borrow().cloud_arc;
        let message = match arc {
            Some(arc) => format!("Specify first corner point or [Arc length/Object] (arc length {}):", format_general(arc)),
            None => "Specify first corner point or [Arc length/Object]:".to_string(),
        };
        match io.ask(Prompt::point(message).options(&[ARC_LENGTH, OBJECT])).await {
            Some(Answer::Point(first)) => {
                let corners = move |second: Point| [first, pt(second.x, first.y), second, pt(first.x, second.y)];
                let length = move |second: Point| arc.unwrap_or_else(|| first.dist(second) / 12.0).max(f64::MIN_POSITIVE);
                let prompt = Prompt::point("Specify opposite corner:").base(first).preview(move |cursor| {
                    if cursor == first { vec![] } else { cloud(&corners(cursor), length(cursor)).into_iter().collect() }
                });
                let Some(Answer::Point(second)) = io.ask(prompt).await else { return };
                if first.x == second.x || first.y == second.y {
                    return;
                }
                defaults.borrow_mut().cloud_arc = Some(length(second));
                io.step(Step::Drawn(cloud(&corners(second), length(second)).into_iter().collect()));
                return;
            }
            Some(answer) if answer.is(ARC_LENGTH) => {
                let prompt = Prompt::new(PromptKind::Number, "Specify length of arc:");
                if let Some(Answer::Number(length)) = io.ask(prompt).await {
                    defaults.borrow_mut().cloud_arc = Some(length);
                }
            }
            Some(answer) if answer.is(OBJECT) => {
                let Some(Answer::Picked(picked)) = io.ask(Prompt::new(PromptKind::Pick, "Select object:")).await else { return };
                let Some(outline) = crate::model::editing::closed_loop(&picked.entity) else {
                    io.step(Step::Report("Pick a closed object: a circle, a closed polyline or a rectangle.".into()));
                    continue;
                };
                let size = entity_extent(&picked.entity).diagonal();
                let arc = arc.unwrap_or(size / 12.0);
                let Some(shape) = cloud(&outline, arc).map(|c| c.shape) else { return };
                io.step(Step::Compound(vec![Step::Remove(vec![picked.entity.clone()]), Step::Commit(vec![picked.entity.derive(shape)])]));
                return;
            }
            _ => return,
        }
    }
}

async fn whole_number(io: &Io, message: &str, default: f64) -> Option<usize> {
    let prompt = Prompt::new(PromptKind::Number, format!("{message} <{}>:", format_general(default))).default(Answer::Number(default));
    match io.ask(prompt).await {
        Some(Answer::Number(n)) if n.fract() == 0.0 && (1.0..=200.0).contains(&n) => Some(n as usize),
        Some(_) => {
            io.step(Step::Report("Enter a whole number from 1 to 200.".into()));
            None
        }
        None => None,
    }
}

pub async fn table_command(io: Io) {
    let Some(columns) = whole_number(&io, "Enter number of columns", 5.0).await else { return };
    let Some(rows) = whole_number(&io, "Enter number of data rows", 3.0).await else { return };
    let prompt = Prompt::new(PromptKind::Number, "Specify column width <30>:").default(Answer::Number(30.0));
    let Some(Answer::Number(width)) = io.ask(prompt).await else { return };
    let prompt = Prompt::new(PromptKind::Number, "Specify row height <9>:").default(Answer::Number(9.0));
    let Some(Answer::Number(height)) = io.ask(prompt).await else { return };
    let table = move |position: Point, cells: Vec<Vec<String>>| {
        Entity::new(Shape::Table {
            position,
            columns: vec![width; columns],
            rows: vec![height; rows + 1],
            cells,
            text_height: height * 0.36,
            title: columns > 1,
        })
    };
    let empty = vec![vec![String::new(); columns]; rows + 1];
    let preview = {
        let (table, empty) = (table, empty.clone());
        move |corner: Point| vec![table(corner, empty.clone())]
    };
    let Some(Answer::Point(corner)) = io.ask(Prompt::point("Specify insertion point:").preview(preview)).await else { return };
    let drawn = table(corner, empty);
    io.step(Step::Drawn(vec![drawn.clone()]));
    fill_cells(&io, drawn, (0, 0)).await;
}

/// Ask each cell's text in turn from the one given, as AutoCAD moves on with Tab: Enter keeps a cell's text and
/// goes on, Esc stops. Each cell changed is its own undo step.
async fn fill_cells(io: &Io, mut table: Entity, from: (usize, usize)) {
    let Shape::Table { columns, rows, title, .. } = &table.shape else { return };
    let (column_count, row_count, title) = (columns.len(), rows.len(), *title);
    let order: Vec<(usize, usize)> =
        (0..row_count).flat_map(|r| (0..column_count).map(move |c| (r, c))).filter(|&(r, c)| !(title && r == 0 && c > 0)).collect();
    let start = order.iter().position(|cell| *cell == from).unwrap_or(0);
    for &(r, c) in &order[start..] {
        let Shape::Table { cells, .. } = &table.shape else { return };
        let current = cells.get(r).and_then(|row| row.get(c)).cloned().unwrap_or_default();
        let which =
            if title && r == 0 { "the title".to_string() } else { format!("row {}, column {}", r + if title { 0 } else { 1 }, c + 1) };
        let prompt = Prompt::new(PromptKind::Text, format!("Enter text for {which} <{current}>:")).default(Answer::Text(current.clone()));
        let Some(Answer::Text(text)) = io.ask(prompt).await else { return };
        if text != current {
            let mut shape = table.shape.clone();
            if let Shape::Table { cells, .. } = &mut shape {
                cells[r][c] = text;
            }
            table = table.with_shape(shape);
            io.step(Step::Modify(vec![table.clone()]));
        }
    }
}

/// TABLEDIT: pick inside a table's cell (or double-click it) and type its text, moving on from cell to cell.
pub async fn tabledit_command(io: Io, defaults: std::rc::Rc<std::cell::RefCell<super::defaults::Defaults>>) {
    let picked = defaults.borrow_mut().table_cell.take();
    let (table, point) = match picked {
        Some(found) => found,
        None => {
            let Some(Answer::Picked(picked)) = io.ask(Prompt::point("Select a table cell:").table()).await else {
                io.step(Step::Report("Pick inside a table's cell.".into()));
                return;
            };
            (picked.entity, picked.point)
        }
    };
    let Some(cell) = crate::model::table::cell_at(&table.shape, point) else { return };
    fill_cells(&io, table, cell).await;
}

/// Placing an image IMAGEATTACH chose: its lower left corner, then its width, its height following.
pub async fn place_image(io: Io, defaults: std::rc::Rc<std::cell::RefCell<super::defaults::Defaults>>) {
    let Some((path, pixels)) = defaults.borrow_mut().image.take() else { return };
    let aspect = pixels.1 as f64 / pixels.0.max(1) as f64;
    let shape = {
        let path = path.clone();
        move |corner: Point, width: f64| {
            Entity::new(Shape::Image { path: path.clone(), position: corner, u: pt(width, 0.0), v: pt(0.0, width * aspect), pixels })
        }
    };
    let preview = shape.clone();
    let prompt = Prompt::point("Specify insertion point:").preview(move |corner| vec![preview(corner, 100.0)]);
    let Some(Answer::Point(corner)) = io.ask(prompt).await else { return };
    let preview = shape.clone();
    let prompt = Prompt::new(PromptKind::Number, "Specify width <100>:")
        .base(corner)
        .default(Answer::Number(100.0))
        .preview(move |cursor| vec![preview(corner, (cursor.x - corner.x).abs().max(f64::MIN_POSITIVE))]);
    let Some(Answer::Number(width)) = io.ask(prompt).await else { return };
    io.step(Step::Drawn(vec![shape(corner, width)]));
}

const POLYLINE: &str = "P";
const CLOSE: &str = "C";
const UNDO: &str = "U";

const FRAMES: &str = "F";

/// WIPEOUT: an area that hides what is drawn under it, from corners clicked in turn or from a closed polyline. Its
/// Frames option sets whether wipeout frames show and plot, for the whole drawing.
pub async fn wipeout_command(io: Io, defaults: std::rc::Rc<std::cell::RefCell<super::defaults::Defaults>>) {
    let prompt = Prompt::point("Specify first point or [Frames/Polyline] <Polyline>:")
        .options(&[FRAMES, POLYLINE])
        .default(Answer::keyword(POLYLINE));
    let corners = match io.ask(prompt).await {
        Some(Answer::Point(first)) => wipeout_corners(&io, first).await,
        Some(answer) if answer.is(FRAMES) => {
            let current = defaults.borrow().wipeout_frame;
            wipeout_frames(&io, current).await;
            return;
        }
        Some(answer) if answer.is(POLYLINE) => {
            let Some(Answer::Picked(picked)) = io.ask(Prompt::new(PromptKind::Pick, "Select a closed polyline:")).await else { return };
            let corners = match &picked.entity.shape {
                Shape::Polyline { vertices, .. } if vertices.iter().any(|v| v.bulge != 0.0) => {
                    io.step(Step::Report("The polyline must have straight segments only.".into()));
                    return;
                }
                Shape::Polyline { .. } => crate::model::editing::closed_loop(&picked.entity),
                _ => None,
            };
            let Some(corners) = corners else {
                io.step(Step::Report("The object must be a closed polyline.".into()));
                return;
            };
            let prompt = Prompt::point("Erase polyline? [Yes/No] <No>:").options(&["Y", "N"]).default(Answer::keyword("N"));
            if io.ask(prompt).await.is_some_and(|answer| answer.is("Y")) {
                io.step(Step::Remove(vec![picked.entity.clone()]));
            }
            Some(corners)
        }
        _ => None,
    };
    if let Some(corners) = corners {
        io.step(Step::Drawn(vec![Entity::new(Shape::Wipeout { corners })]));
    }
}

/// The corners after the first, until Enter or Close; None when cancelled or with fewer than three.
async fn wipeout_corners(io: &Io, first: Point) -> Option<Vec<Point>> {
    let mut corners = vec![first];
    loop {
        let shown = corners.clone();
        let preview = move |cursor: Point| {
            let mut points = shown.clone();
            points.push(cursor);
            vec![polyline_from(&points, points.len() > 2)]
        };
        let last = *corners.last()?;
        let prompt = match corners.len() {
            1 => Prompt::point("Specify next point:"),
            2 => Prompt::point("Specify next point or [Undo]:").options(&[UNDO]),
            _ => Prompt::point("Specify next point or [Close/Undo]:").options(&[CLOSE, UNDO]),
        };
        match io.ask(prompt.base(last).preview(preview)).await {
            Some(Answer::Point(point)) => corners.push(point),
            Some(answer) if answer.is(UNDO) => {
                corners.pop();
                if corners.is_empty() {
                    return None;
                }
            }
            Some(answer) if answer.is(CLOSE) => break,
            None if corners.len() >= 3 => break,
            _ => return None,
        }
    }
    Some(corners)
}

/// FRAMES: asks whether wipeout frames show and plot, as WIPEOUTFRAME does, and sets the answer. The command ends.
async fn wipeout_frames(io: &Io, current: u8) {
    let (word, name) = match current {
        0 => ("OFF", "OFF"),
        2 => ("D", "Display but not plot"),
        _ => ("ON", "ON"),
    };
    let prompt = Prompt::point(format!("Enter mode [ON/OFF/Display but not plot] <{name}>:"))
        .options(&["ON", "OFF", "D"])
        .default(Answer::keyword(word));
    let mode = match io.ask(prompt).await {
        Some(answer) if answer.is("ON") => 1,
        Some(answer) if answer.is("OFF") => 0,
        Some(answer) if answer.is("D") => 2,
        _ => return,
    };
    io.step(Step::WipeoutFrame(mode));
}

const JUSTIFICATION: &str = "J";
const SCALE: &str = "S";
const STYLE: &str = "ST";

/// MLINE: parallel lines along a path, in a multiline style, with the justification and scale it remembers.
pub async fn mline_command(io: Io, defaults: std::rc::Rc<std::cell::RefCell<super::defaults::Defaults>>) {
    use crate::model::mline::Justification;
    let first = loop {
        let (justification, scale, style) = {
            let d = defaults.borrow();
            (d.mline_justification, d.mline_scale, d.mline_style.clone())
        };
        io.step(Step::Report(format!(
            "Current settings: Justification = {}, Scale = {:.2}, Style = {}",
            justification.name(),
            scale,
            style.name.to_uppercase()
        )));
        let prompt = Prompt::point("Specify start point or [Justification/Scale/STyle]:").options(&[JUSTIFICATION, SCALE, STYLE]);
        match io.ask(prompt).await {
            Some(Answer::Point(first)) => break first,
            Some(answer) if answer.is(JUSTIFICATION) => {
                let prompt =
                    Prompt::point(format!("Enter justification type [Top/Zero/Bottom] <{}>:", justification.name().to_lowercase()))
                        .options(&["T", "Z", "B"])
                        .default(Answer::keyword(&justification.name()[..1].to_uppercase()));
                if let Some(Answer::Keyword(choice)) = io.ask(prompt).await {
                    defaults.borrow_mut().mline_justification = match choice.as_str() {
                        "Z" => Justification::Zero,
                        "B" => Justification::Bottom,
                        _ => Justification::Top,
                    };
                }
            }
            Some(answer) if answer.is(SCALE) => {
                let prompt = Prompt::new(PromptKind::Length, format!("Enter mline scale <{scale:.2}>:")).default(Answer::Number(scale));
                if let Some(Answer::Number(scale)) = io.ask(prompt).await {
                    defaults.borrow_mut().mline_scale = scale;
                }
            }
            Some(answer) if answer.is(STYLE) => {
                let styles = defaults.borrow().mline_styles.clone();
                let names = styles.iter().map(|s| s.name.clone()).collect::<Vec<_>>().join(", ");
                let Some(Answer::Text(name)) = io.ask(Prompt::new(PromptKind::Text, "Enter mline style name or [?]:")).await else {
                    continue;
                };
                let name = name.trim();
                if name == "?" {
                    io.step(Step::Report(format!("Loaded mline styles: {names}")));
                } else if let Some(found) = styles.iter().find(|s| s.name.eq_ignore_ascii_case(name)) {
                    defaults.borrow_mut().mline_style = found.clone();
                } else {
                    io.step(Step::Report(format!("Cannot find mline style \"{name}\". Loaded styles: {names}")));
                }
            }
            _ => return,
        }
    };
    let (justification, scale, style) = {
        let d = defaults.borrow();
        (d.mline_justification, d.mline_scale, d.mline_style.clone())
    };
    let make = move |vertices: Vec<Point>, closed: bool| {
        Entity::new(Shape::Mline { vertices, closed, justification, scale, style: style.clone() })
    };
    let mut points = vec![first];
    let closed = loop {
        let (shown, make_shown) = (points.clone(), make.clone());
        let preview = move |cursor: Point| {
            let mut through = shown.clone();
            through.push(cursor);
            vec![make_shown(through, false)]
        };
        let prompt = match points.len() {
            1 => Prompt::point("Specify next point:"),
            2 => Prompt::point("Specify next point or [Undo]:").options(&[UNDO]),
            _ => Prompt::point("Specify next point or [Close/Undo]:").options(&[CLOSE, UNDO]),
        };
        match io.ask(prompt.base(*points.last().unwrap_or(&first)).preview(preview)).await {
            Some(Answer::Point(point)) => points.push(point),
            Some(answer) if answer.is(UNDO) => {
                if points.len() > 1 {
                    points.pop();
                }
            }
            Some(answer) if answer.is(CLOSE) => break true,
            _ => break false,
        }
    };
    if points.len() >= 2 {
        io.step(Step::Drawn(vec![make(points, closed)]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_circumscribed_square_has_its_sides_touching_the_circle() {
        let Shape::Polyline { vertices, closed: true } = polygon(pt(0.0, 0.0), first_corner(pt(0.0, 0.0), pt(1.0, 0.0), 4, false), 4).shape
        else {
            panic!()
        };
        let corner = vertices[0].point();
        assert!((corner.dist(pt(0.0, 0.0)) - 2f64.sqrt()).abs() < 1e-9);
        assert!((corner.x - 1.0).abs() < 1e-9 && (corner.y + 1.0).abs() < 1e-9);
    }

    #[test]
    fn points_fall_at_even_lengths() {
        let path = [pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)];
        let points = points_along(&path, &[5.0, 10.0, 15.0]);
        assert_eq!(points, vec![pt(5.0, 0.0), pt(10.0, 0.0), pt(10.0, 5.0)]);
    }
}
