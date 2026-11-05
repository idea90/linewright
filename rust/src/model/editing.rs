//! Geometry behind the editing commands: exploding compound objects, stretching by a window, joining chains,
//! grips, and hatch patterns.

use super::viewport::{frame_corners, reframed};
use std::f64::consts::TAU;

use super::dimension::{DimensionLook, dimension_parts};
use super::entities::{Entity, PatternLine, Shape, Vertex, vertex};
use super::geometry::{Point, arc_through_points, bulge_arc, direction, modulo, pt, sweep_degrees};
use super::index::Bounds;
use super::parts::{Styled, ellipse_point, entity_parts, flatten};
use super::transform::{insert_transform, transform_shape, translation};

const JOIN_TOLERANCE: f64 = 1e-6;

fn styled(entity: &Entity, style: &Styled, shape: Shape) -> Entity {
    Entity {
        layer: style.layer.clone(),
        color: style.color,
        linetype: style.linetype.clone(),
        linetype_scale: style.linetype_scale,
        lineweight: style.lineweight,
        ..entity.derive(shape)
    }
}

/// The simpler objects a compound object is made of, or None for one that cannot be exploded. A polyline gives
/// lines and arcs, a block insert its block's entities where the insert puts them (in the colors they showed in),
/// a dimension its lines, arrowheads and text, a hatch its boundaries.
pub fn explode(entity: &Entity) -> Option<Vec<Entity>> {
    match &entity.shape {
        Shape::Polyline { vertices, closed } => {
            let count = if *closed { vertices.len() } else { vertices.len().saturating_sub(1) };
            Some(
                (0..count)
                    .map(|i| {
                        let (here, next) = (vertices[i], vertices[(i + 1) % vertices.len()]);
                        let shape = if here.bulge == 0.0 || here.point() == next.point() {
                            Shape::Line { start: here.point(), end: next.point() }
                        } else {
                            let arc = bulge_arc(here.point(), next.point(), here.bulge);
                            let (from, to) = (arc.start_angle.to_degrees(), (arc.start_angle + arc.sweep).to_degrees());
                            let (start_angle, end_angle) = if arc.sweep > 0.0 { (from, to) } else { (to, from) };
                            Shape::Arc {
                                center: arc.center,
                                radius: arc.radius,
                                start_angle: modulo(start_angle, 360.0),
                                end_angle: modulo(end_angle, 360.0),
                            }
                        };
                        entity.derive(shape)
                    })
                    .collect(),
            )
        }
        Shape::Insert { block, position, scale_x, scale_y, rotation, attributes } => {
            let placed = insert_transform(block.0.base, *position, *scale_x, *scale_y, *rotation);
            let style = Styled::of(entity);
            let mut pieces = vec![];
            for child in &block.0.entities {
                let child_style = Styled::inside(child, &style);
                if placed.is_uniform() {
                    pieces.push(styled(entity, &child_style, transform_shape(&child.shape, &placed)));
                } else {
                    // A stretched block bends circles into ellipses: its curves come apart into polylines.
                    for (_, parts) in flatten(child, crate::model::index::entity_extent(child).diagonal().max(1e-9) * 1e-3) {
                        for path in parts.paths {
                            let points: Vec<Vertex> = path.iter().map(|p| vertex(placed.point(*p))).collect();
                            pieces.push(styled(entity, &child_style, Shape::Polyline { vertices: points, closed: false }));
                        }
                    }
                }
            }
            for attribute in attributes.iter().filter(|a| !a.invisible) {
                let style = Styled {
                    layer: attribute.layer.clone(),
                    color: attribute.color,
                    linetype: entity.linetype.clone(),
                    linetype_scale: entity.linetype_scale,
                    lineweight: entity.lineweight,
                };
                pieces.push(styled(entity, &style, attribute.text.clone()));
            }
            Some(pieces)
        }
        Shape::Dimension { start, end, line_point, angle, look } => {
            let parts = dimension_parts(*start, *end, *line_point, *angle, look);
            let mut pieces: Vec<Entity> = parts.lines().map(|(a, b)| entity.derive(Shape::Line { start: *a, end: *b })).collect();
            pieces.extend(parts.arrows.iter().map(|corners| entity.derive(Shape::Solid { corners: corners.to_vec() })));
            for text in entity_parts(entity, 1.0).texts {
                pieces.push(entity.derive(Shape::Text {
                    position: text.origin,
                    text: text.text,
                    height: text.height,
                    rotation: text.rotation,
                    font: String::new(),
                    style: String::new(),
                    align: text.align,
                    line_spacing: 1.0,
                    formatted: String::new(),
                    width_factor: 1.0,
                    oblique: 0.0,
                }));
            }
            Some(pieces)
        }
        // A multiline comes apart into a line for each segment of each of its lines, in that line's look.
        Shape::Mline { .. } => Some(
            flatten(entity, 1.0)
                .into_iter()
                .flat_map(|(style, parts)| {
                    let lines: Vec<Entity> = parts
                        .paths
                        .iter()
                        .flat_map(|path| path.windows(2))
                        .map(|w| styled(entity, &style, Shape::Line { start: w[0], end: w[1] }))
                        .collect();
                    lines
                })
                .collect(),
        ),
        Shape::Annotation { .. } | Shape::Table { .. } | Shape::Multileader { .. } => {
            let parts = entity_parts(entity, f64::INFINITY);
            let mut pieces: Vec<Entity> = parts
                .paths
                .iter()
                .map(|path| match path[..] {
                    [start, end] => entity.derive(Shape::Line { start, end }),
                    _ => entity.derive(Shape::Polyline { vertices: path.iter().copied().map(vertex).collect(), closed: false }),
                })
                .collect();
            pieces.extend(parts.fills.iter().map(|corners| entity.derive(Shape::Solid { corners: corners.to_vec() })));
            pieces.extend(parts.texts.into_iter().map(|text| {
                entity.derive(Shape::Text {
                    position: text.origin,
                    text: text.text,
                    height: text.height,
                    rotation: text.rotation,
                    font: String::new(),
                    style: String::new(),
                    align: text.align,
                    line_spacing: 1.0,
                    formatted: String::new(),
                    width_factor: 1.0,
                    oblique: 0.0,
                })
            }));
            Some(pieces)
        }
        Shape::Hatch { loops, .. } => Some(
            loops
                .iter()
                .map(|l| entity.derive(Shape::Polyline { vertices: l.iter().copied().map(vertex).collect(), closed: true }))
                .collect(),
        ),
        _ => None,
    }
}

/// The entity with what lies inside the window moved by `shift`: the ends and vertices inside, or the whole object
/// when the point it hangs on (a circle's center, a text's position) is inside. None when nothing is inside.
pub fn stretch(entity: &Entity, window: &Bounds, shift: Point) -> Option<Entity> {
    let moved = |p: &Point| if window.contains(*p) { *p + shift } else { *p };
    let shape = match &entity.shape {
        Shape::Line { start, end } => Shape::Line { start: moved(start), end: moved(end) },
        Shape::Multileader { leaders, landing, text_position, .. } => {
            let mut shape = entity.shape.clone();
            if let Shape::Multileader { leaders: l, landing: at, text_position: corner, .. } = &mut shape {
                *l = leaders.iter().map(|leader| leader.iter().map(&moved).collect()).collect();
                *at = moved(landing);
                *corner = moved(text_position);
            }
            shape
        }
        Shape::Polyline { vertices, closed } => Shape::Polyline {
            vertices: vertices.iter().map(|v| Vertex { x: moved(&v.point()).x, y: moved(&v.point()).y, ..*v }).collect(),
            closed: *closed,
        },
        Shape::Dimension { start, end, line_point, angle, look } => Shape::Dimension {
            start: moved(start),
            end: moved(end),
            line_point: moved(line_point),
            angle: *angle,
            look: DimensionLook { text_position: look.text_position.as_ref().map(moved), ..look.clone() },
        },
        Shape::XLine { base, direction, ray } => Shape::XLine { base: moved(base), direction: *direction, ray: *ray },
        Shape::Table { position, .. } if window.contains(*position) => transform_shape(&entity.shape, &translation(shift.x, shift.y)),
        Shape::Table { .. } => return None,
        // An image keeps its shape: it moves when its corner is caught.
        Shape::Image { path, position, u, v, pixels } => {
            Shape::Image { path: path.clone(), position: moved(position), u: *u, v: *v, pixels: *pixels }
        }
        Shape::Viewport { center, width, height, .. } => {
            let corners = frame_corners(*center, *width, *height).map(|c| moved(&c));
            reframed(&entity.shape, Bounds::around(corners))?
        }
        Shape::Annotation { kind, points, look } => Shape::Annotation {
            kind: *kind,
            points: points.iter().map(moved).collect(),
            look: DimensionLook { text_position: look.text_position.as_ref().map(moved), ..look.clone() },
        },
        Shape::Spline { degree, control, knots, weights, closed } => Shape::Spline {
            degree: *degree,
            control: control.iter().map(moved).collect(),
            knots: knots.clone(),
            weights: weights.clone(),
            closed: *closed,
        },
        Shape::Solid { corners } => Shape::Solid { corners: corners.iter().map(moved).collect() },
        Shape::Wipeout { corners } => Shape::Wipeout { corners: corners.iter().map(moved).collect() },
        Shape::Mline { vertices, closed, justification, scale, style, gaps } => Shape::Mline {
            vertices: vertices.iter().map(moved).collect(),
            closed: *closed,
            justification: *justification,
            scale: *scale,
            style: style.clone(),
            gaps: gaps.clone(),
        },
        Shape::Hatch { loops, solid, pattern, name, scale, angle, gradient, .. } => Shape::Hatch {
            loops: loops.iter().map(|l| l.iter().map(moved).collect()).collect(),
            solid: *solid,
            pattern: pattern.clone(),
            name: name.clone(),
            scale: *scale,
            angle: *angle,
            gradient: gradient.clone(),
            // Stretched itself, it no longer matches what it followed.
            association: None,
        },
        Shape::Circle { center: anchor, .. }
        | Shape::Arc { center: anchor, .. }
        | Shape::Ellipse { center: anchor, .. }
        | Shape::Text { position: anchor, .. }
        | Shape::Insert { position: anchor, .. }
        | Shape::Point { position: anchor } => {
            if !window.contains(*anchor) {
                return None;
            }
            transform_shape(&entity.shape, &translation(shift.x, shift.y))
        }
    };
    (shape != entity.shape).then(|| entity.with_shape(shape))
}

/// A piece of a chain: from one point to another, straight or (with a bulge) along an arc.
#[derive(Clone, Copy, Debug)]
struct Link {
    start: Point,
    end: Point,
    bulge: f64,
}

impl Link {
    fn reversed(self) -> Link {
        Link { start: self.end, end: self.start, bulge: -self.bulge }
    }
}

fn links(entity: &Entity) -> Option<Vec<Link>> {
    match &entity.shape {
        Shape::Line { start, end } => Some(vec![Link { start: *start, end: *end, bulge: 0.0 }]),
        Shape::Arc { center, radius, start_angle, end_angle } => {
            let sweep = sweep_degrees(*start_angle, *end_angle).to_radians();
            Some(vec![Link {
                start: *center + direction(*start_angle) * *radius,
                end: *center + direction(*end_angle) * *radius,
                bulge: (sweep / 4.0).tan(),
            }])
        }
        Shape::Polyline { vertices, closed: false } => {
            Some(vertices.windows(2).map(|w| Link { start: w[0].point(), end: w[1].point(), bulge: w[0].bulge }).collect())
        }
        _ => None,
    }
}

/// Lines, arcs and open polylines that meet end to end, joined into polylines (or one line, when the chain is
/// straight). Returns the objects joined and what replaces them; None when nothing joins.
pub fn join(entities: &[Entity]) -> Option<(Vec<Entity>, Vec<Entity>)> {
    let mut pool: Vec<(Entity, Vec<Link>)> = entities.iter().filter_map(|e| Some((e.clone(), links(e)?))).collect();
    let size = entities.iter().map(|e| crate::model::index::entity_extent(e).diagonal()).fold(0.0, f64::max).max(1.0);
    let near = |a: Point, b: Point| a.dist(b) <= JOIN_TOLERANCE * size;
    let (mut removed, mut added) = (vec![], vec![]);
    while let Some((first, mut chain)) = pool.pop() {
        let mut members = vec![first];
        loop {
            let (start, end) = (chain[0].start, chain[chain.len() - 1].end);
            let found = pool.iter().position(|(_, l)| {
                let (s, e) = (l[0].start, l[l.len() - 1].end);
                near(s, end) || near(e, end) || near(s, start) || near(e, start)
            });
            let Some(index) = found else { break };
            let (entity, more) = pool.remove(index);
            let (s, e) = (more[0].start, more[more.len() - 1].end);
            let reversed = || more.iter().rev().map(|l| l.reversed()).collect::<Vec<_>>();
            if near(s, end) {
                chain.extend(more);
            } else if near(e, end) {
                chain.extend(reversed());
            } else if near(e, start) {
                chain.splice(0..0, more);
            } else {
                chain.splice(0..0, reversed());
            }
            members.push(entity);
        }
        if members.len() < 2 {
            continue;
        }
        let straight = chain.iter().all(|l| l.bulge == 0.0);
        let (start, end) = (chain[0].start, chain[chain.len() - 1].end);
        let along = end - start;
        let collinear = straight
            && along.length() > 0.0
            && chain.iter().all(|l| along.cross(l.end - start).abs() <= JOIN_TOLERANCE * size * along.length());
        let shape = if collinear {
            Shape::Line { start, end }
        } else {
            let closed = near(start, end);
            let mut vertices: Vec<Vertex> =
                chain.iter().map(|l| Vertex { x: l.start.x, y: l.start.y, bulge: l.bulge, start_width: 0.0, end_width: 0.0 }).collect();
            if !closed {
                vertices.push(vertex(end));
            }
            Shape::Polyline { vertices, closed }
        };
        added.push(members[0].derive(shape));
        removed.extend(members);
    }
    (!removed.is_empty()).then_some((removed, added))
}

/// How far along a construction line its turning grip sits.
const XLINE_GRIP: f64 = 10.0;

/// The points a selected object shows grips at.
pub fn grip_points(shape: &Shape) -> Vec<Point> {
    match shape {
        Shape::Line { start, end } => vec![*start, start.midpoint(*end), *end],
        Shape::Polyline { vertices, .. } => vertices.iter().map(|v| v.point()).collect(),
        Shape::Circle { center, radius } => {
            std::iter::once(*center).chain([0.0, 90.0, 180.0, 270.0].map(|a| *center + direction(a) * *radius)).collect()
        }
        Shape::Arc { center, radius, start_angle, end_angle } => {
            let middle = start_angle + sweep_degrees(*start_angle, *end_angle) / 2.0;
            [*start_angle, middle, *end_angle].iter().fold(vec![*center], |mut points, a| {
                points.push(*center + direction(*a) * *radius);
                points
            })
        }
        Shape::Text { position, .. } | Shape::Insert { position, .. } | Shape::Point { position } => vec![*position],
        Shape::Dimension { start, end, line_point, angle, look } => {
            let line = dimension_parts(*start, *end, *line_point, *angle, look).dimension_line;
            vec![*start, *end, line.0.midpoint(line.1)]
        }
        Shape::Ellipse { center, major, ratio, .. } => {
            let minor = pt(-major.y, major.x) * *ratio;
            vec![*center, *center + *major, *center - *major, *center + minor, *center - minor]
        }
        Shape::Spline { control, .. } => control.clone(),
        Shape::Annotation { points, .. } => points.clone(),
        // Every leader's points, then the landing.
        Shape::Multileader { leaders, landing, .. } => leaders.iter().flatten().copied().chain([*landing]).collect(),
        Shape::Image { position, u, v, .. } => crate::model::entities::image_corners(*position, *u, *v).to_vec(),
        Shape::Table { position, .. } => vec![*position],
        // The base, and a point a little way along that turns it.
        Shape::XLine { base, direction, .. } => vec![*base, *base + *direction * XLINE_GRIP],
        Shape::Viewport { center, width, height, .. } => {
            let mut points = frame_corners(*center, *width, *height).to_vec();
            points.push(*center);
            points
        }
        Shape::Solid { corners } | Shape::Wipeout { corners } => corners.clone(),
        Shape::Mline { vertices, .. } => vertices.clone(),
        Shape::Hatch { .. } => vec![],
    }
}

/// The shape with a grip dragged to a new point: an end or vertex moves, a center or position moves the whole
/// object, a circle's quadrant sets its radius, an arc's points reshape it through the other two.
pub fn apply_grip(shape: &Shape, index: usize, to: Point) -> Option<Shape> {
    let grips = grip_points(shape);
    let from = *grips.get(index)?;
    let moved = || transform_shape(shape, &translation(to.x - from.x, to.y - from.y));
    Some(match shape {
        Shape::Line { start, end } => match index {
            0 => Shape::Line { start: to, end: *end },
            2 => Shape::Line { start: *start, end: to },
            _ => moved(),
        },
        Shape::Polyline { vertices, closed } => {
            let mut vertices = vertices.clone();
            vertices[index] = Vertex { x: to.x, y: to.y, ..vertices[index] };
            Shape::Polyline { vertices, closed: *closed }
        }
        Shape::Circle { center, .. } if index > 0 => Shape::Circle { center: *center, radius: center.dist(to) },
        Shape::Arc { .. } if index > 0 => {
            let mut points = [grips[1], grips[2], grips[3]];
            points[index - 1] = to;
            let arc = arc_through_points(points[0], points[1], points[2])?;
            Shape::Arc { center: arc.center, radius: arc.radius, start_angle: arc.start_angle, end_angle: arc.end_angle }
        }
        Shape::Dimension { start, end, line_point, angle, look } => {
            // Moving a grip lays the dimension out afresh, text centered again.
            let look = DimensionLook { text_position: None, ..look.clone() };
            match index {
                0 => Shape::Dimension { start: to, end: *end, line_point: *line_point, angle: *angle, look },
                1 => Shape::Dimension { start: *start, end: to, line_point: *line_point, angle: *angle, look },
                _ => Shape::Dimension { start: *start, end: *end, line_point: to, angle: *angle, look },
            }
        }
        Shape::XLine { base, ray, .. } if index == 1 && to != *base => {
            let toward = to - *base;
            Shape::XLine { base: *base, direction: toward * (1.0 / toward.length()), ray: *ray }
        }
        Shape::Viewport { center, width, height, .. } if index < 4 => {
            let opposite = frame_corners(*center, *width, *height)[(index + 2) % 4];
            reframed(shape, Bounds::from_corners(opposite, to))?
        }
        Shape::Multileader { .. } => {
            let mut shape = shape.clone();
            let Shape::Multileader { leaders, landing, text_position, .. } = &mut shape else { return None };
            let mut left = index;
            let target = leaders.iter_mut().flatten().find(|_| {
                let here = left == 0;
                left = left.wrapping_sub(1);
                here
            });
            match target {
                Some(point) => *point = to,
                None => {
                    // The landing takes the dogleg and text along.
                    *text_position = *text_position + (to - *landing);
                    *landing = to;
                }
            }
            shape
        }
        Shape::Annotation { kind, points, look } => {
            let mut points = points.clone();
            points[index] = to;
            Shape::Annotation { kind: *kind, points, look: DimensionLook { text_position: None, ..look.clone() } }
        }
        Shape::Ellipse { center, major, ratio, start, end } if index > 0 => {
            let minor_length = major.length() * ratio;
            let (major, ratio) = if index <= 2 {
                let major = (to - *center) * if index == 1 { 1.0 } else { -1.0 };
                (major, minor_length / major.length().max(1e-300))
            } else {
                (*major, center.dist(to) / major.length().max(1e-300))
            };
            Shape::Ellipse { center: *center, major, ratio, start: *start, end: *end }
        }
        Shape::Spline { degree, control, knots, weights, closed } => {
            let mut control = control.clone();
            control[index] = to;
            Shape::Spline { degree: *degree, control, knots: knots.clone(), weights: weights.clone(), closed: *closed }
        }
        Shape::Solid { corners } => {
            let mut corners = corners.clone();
            corners[index] = to;
            Shape::Solid { corners }
        }
        Shape::Wipeout { corners } => {
            let mut corners = corners.clone();
            corners[index] = to;
            Shape::Wipeout { corners }
        }
        Shape::Mline { vertices, .. } => {
            let mut shape = shape.clone();
            if let Shape::Mline { vertices: moved, .. } = &mut shape {
                *moved = vertices.clone();
                moved[index] = to;
            }
            shape
        }
        _ => moved(),
    })
}

/// A hatch pattern by name, at a scale and angle: (solid fill, pattern lines). The lines start from the origin.
/// None for a name Linewright does not know.
pub fn hatch_pattern(name: &str, scale: f64, angle: f64) -> Option<(bool, Vec<PatternLine>)> {
    // SOLID is a fill, whatever a pattern file has under that name.
    if name.trim().eq_ignore_ascii_case("SOLID") {
        return Some((true, vec![]));
    }
    // AutoCAD's own patterns where they are on this computer.
    if let Some(families) = crate::io::pat::library().get(&name.trim().to_uppercase()) {
        let lines = families
            .iter()
            .map(|family| {
                let turned = family.angle + angle;
                let (u, v) = (direction(turned), direction(turned + 90.0));
                let (x, y) = (direction(angle), direction(angle + 90.0));
                PatternLine {
                    angle: modulo(turned, 360.0),
                    origin: (x * family.x + y * family.y) * scale,
                    offset: (u * family.along + v * family.across) * scale,
                    dashes: family.dashes.iter().map(|d| d * scale).collect(),
                }
            })
            .collect();
        return Some((false, lines));
    }
    const STEP: f64 = 3.175; // an eighth of an inch, the spacing of AutoCAD's ANSI patterns
    // Each family: angle, then the offset to the next line along the line and across it, then its dashes.
    let families: Vec<(f64, f64, f64, Vec<f64>)> = match name.to_uppercase().as_str() {
        "SOLID" => return Some((true, vec![])),
        "ANSI31" => vec![(45.0, 0.0, STEP, vec![])],
        "ANSI37" => vec![(45.0, 0.0, STEP, vec![]), (135.0, 0.0, STEP, vec![])],
        "NET" => vec![(0.0, 0.0, STEP, vec![]), (90.0, 0.0, STEP, vec![])],
        "LINE" => vec![(0.0, 0.0, STEP, vec![])],
        "DOTS" => vec![(0.0, STEP / 2.0, STEP, vec![0.0, -STEP])],
        "DASH" => vec![(0.0, STEP, STEP, vec![STEP, -STEP])],
        _ => return None,
    };
    let lines = families
        .into_iter()
        .map(|(family_angle, along, across, dashes)| {
            let turned = family_angle + angle;
            let (u, v) = (direction(turned), direction(turned + 90.0));
            PatternLine {
                angle: modulo(turned, 360.0),
                origin: pt(0.0, 0.0),
                offset: (u * along + v * across) * scale,
                dashes: dashes.iter().map(|d| d * scale).collect(),
            }
        })
        .collect();
    Some((false, lines))
}

pub const HATCH_PATTERNS: [&str; 7] = ["SOLID", "ANSI31", "ANSI37", "NET", "LINE", "DOTS", "DASH"];

/// Every pattern name there is to choose from: the built-in ones and those in AutoCAD's pattern file.
pub fn hatch_pattern_names() -> Vec<String> {
    let mut names: Vec<String> = HATCH_PATTERNS.iter().map(|n| n.to_string()).collect();
    names.extend(crate::io::pat::library().keys().filter(|k| !HATCH_PATTERNS.contains(&k.as_str())).cloned());
    names
}

/// A hatch with a new pattern, scale and angle, on the same boundary. None for a pattern that is not known.
pub fn rehatched(shape: &Shape, new_name: &str, new_scale: f64, new_angle: f64) -> Option<Shape> {
    let (new_solid, lines) = hatch_pattern(new_name, new_scale, new_angle)?;
    let mut shape = shape.clone();
    if let Shape::Hatch { solid, pattern, name, scale, angle, gradient, .. } = &mut shape {
        (*solid, *pattern, *name, *scale, *angle) = (new_solid, lines, new_name.trim().to_uppercase(), new_scale, new_angle);
        *gradient = None;
    }
    Some(shape)
}

/// The outline of a closed object, as a loop of points for a hatch boundary. None for an open one.
pub fn closed_loop(entity: &Entity) -> Option<Vec<Point>> {
    let tolerance = crate::model::index::entity_extent(entity).diagonal() * 1e-3;
    let closed_path = |points: Vec<Point>| -> Option<Vec<Point>> {
        let mut points = points;
        if points.len() > 2 && points.first() == points.last() {
            points.pop();
        }
        (points.len() >= 3).then_some(points)
    };
    match &entity.shape {
        Shape::Polyline { vertices, closed } if *closed || vertices.first().map(|v| v.point()) == vertices.last().map(|v| v.point()) => {
            closed_path(entity_parts(entity, tolerance).paths.into_iter().next()?)
        }
        Shape::Circle { .. } => closed_path(entity_parts(entity, tolerance).paths.into_iter().next()?),
        Shape::Ellipse { center, major, ratio, start, end } if (end - start).abs() >= TAU - 1e-9 || (end - start).abs() < 1e-12 => {
            closed_path((0..=96).map(|i| ellipse_point(*center, *major, *ratio, TAU * i as f64 / 96.0)).collect())
        }
        Shape::Spline { closed: true, .. } => closed_path(entity_parts(entity, tolerance).paths.into_iter().next()?),
        Shape::Solid { corners } | Shape::Wipeout { corners } => Some(corners.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::model::entities::{BlockDef, BlockRef, arc, circle, line, polyline};

    #[test]
    fn a_polyline_explodes_into_lines_and_arcs() {
        let shape = polyline(
            vec![
                Vertex { x: 0.0, y: 0.0, bulge: 0.0, start_width: 0.0, end_width: 0.0 },
                Vertex { x: 2.0, y: 0.0, bulge: 1.0, start_width: 0.0, end_width: 0.0 },
                vertex(pt(4.0, 0.0)),
            ],
            false,
        );
        let pieces = explode(&shape).unwrap();
        assert!(matches!(pieces[0].shape, Shape::Line { .. }));
        let Shape::Arc { center, radius, .. } = pieces[1].shape else { panic!() };
        assert!(center.dist(pt(3.0, 0.0)) < 1e-9 && (radius - 1.0).abs() < 1e-9);
    }

    #[test]
    fn an_insert_explodes_into_its_block_where_it_was() {
        let block = BlockRef(Arc::new(BlockDef {
            name: "B".into(),
            base: pt(0.0, 0.0),
            entities: vec![circle(pt(1.0, 0.0), 1.0)],
            attribute_defs: vec![],
            xref: None,
        }));
        let insert =
            Entity::new(Shape::Insert { block, position: pt(10.0, 0.0), scale_x: 2.0, scale_y: 2.0, rotation: 90.0, attributes: vec![] });
        let pieces = explode(&insert).unwrap();
        let Shape::Circle { center, radius } = pieces[0].shape else { panic!() };
        assert!(center.dist(pt(10.0, 2.0)) < 1e-9 && (radius - 2.0).abs() < 1e-12);
    }

    #[test]
    fn stretch_moves_only_what_is_inside_the_window() {
        let window = Bounds { x0: 9.0, y0: -1.0, x1: 11.0, y1: 1.0 };
        let stretched = stretch(&line(pt(0.0, 0.0), pt(10.0, 0.0)), &window, pt(5.0, 0.0)).unwrap();
        assert_eq!(stretched.shape, Shape::Line { start: pt(0.0, 0.0), end: pt(15.0, 0.0) });
        assert!(stretch(&circle(pt(0.0, 0.0), 1.0), &window, pt(5.0, 0.0)).is_none());
    }

    #[test]
    fn touching_lines_and_arcs_join_into_one_polyline() {
        let pieces = [line(pt(0.0, 0.0), pt(10.0, 0.0)), arc(pt(10.0, 5.0), 5.0, 270.0, 90.0), line(pt(0.0, 10.0), pt(10.0, 10.0))];
        let (removed, added) = join(&pieces).unwrap();
        assert_eq!((removed.len(), added.len()), (3, 1));
        let Shape::Polyline { vertices, closed } = &added[0].shape else { panic!() };
        assert!(!closed && vertices.len() == 4);
        assert!(vertices.iter().any(|v| (v.bulge.abs() - 1.0).abs() < 1e-9), "the half circle becomes a bulge of one");
        let (_, straight) = join(&[line(pt(0.0, 0.0), pt(5.0, 0.0)), line(pt(5.0, 0.0), pt(9.0, 0.0))]).unwrap();
        assert_eq!(straight[0].shape, Shape::Line { start: pt(0.0, 0.0), end: pt(9.0, 0.0) });
    }

    #[test]
    fn grips_reshape() {
        let reshaped = apply_grip(&Shape::Circle { center: pt(0.0, 0.0), radius: 1.0 }, 1, pt(3.0, 4.0)).unwrap();
        assert_eq!(reshaped, Shape::Circle { center: pt(0.0, 0.0), radius: 5.0 });
        let moved = apply_grip(&Shape::Line { start: pt(0.0, 0.0), end: pt(2.0, 0.0) }, 1, pt(1.0, 1.0)).unwrap();
        assert_eq!(moved, Shape::Line { start: pt(0.0, 1.0), end: pt(2.0, 1.0) });
    }

    #[test]
    fn patterns_scale_and_turn() {
        let (solid, lines) = hatch_pattern("ansi31", 2.0, 0.0).unwrap();
        assert!(!solid && lines.len() == 1 && (lines[0].angle - 45.0).abs() < 1e-9);
        assert!((lines[0].offset.length() - 6.35).abs() < 1e-9);
        assert!(hatch_pattern("NOSUCH", 1.0, 0.0).is_none());
        assert_eq!(closed_loop(&circle(pt(0.0, 0.0), 1.0)).map(|l| l.len() > 8), Some(true));
    }
}
