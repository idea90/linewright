use super::dimension::DimensionLook;
use super::entities::{Association, Attribute, Entity, Gradient, PatternLine, Shape, Vertex};
use super::geometry::{Point, direction, modulo, pt};

/// A move, rotation, uniform scale or mirror: x' = a*x + b*y + tx, y' = c*x + d*y + ty. Block inserts may also scale
/// X and Y differently; `is_uniform` tells the two apart.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Similarity {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub tx: f64,
    pub ty: f64,
}

impl Similarity {
    pub fn point(&self, p: Point) -> Point {
        pt(self.a * p.x + self.b * p.y + self.tx, self.c * p.x + self.d * p.y + self.ty)
    }

    /// Where a direction given in degrees points after the transform, in 0 to 360.
    pub fn angle(&self, degrees: f64) -> f64 {
        let (sin, cos) = degrees.to_radians().sin_cos();
        let x = self.a * cos + self.b * sin;
        let y = self.c * cos + self.d * sin;
        modulo(y.atan2(x).to_degrees(), 360.0)
    }

    pub fn determinant(&self) -> f64 {
        self.a * self.d - self.b * self.c
    }

    pub fn scale(&self) -> f64 {
        self.determinant().abs().sqrt()
    }

    pub fn reflects(&self) -> bool {
        self.determinant() < 0.0
    }

    /// A vector after the transform: the move left out.
    pub fn linear(&self, v: Point) -> Point {
        pt(self.a * v.x + self.b * v.y, self.c * v.x + self.d * v.y)
    }

    /// This transform after `first`.
    pub fn after(&self, first: &Similarity) -> Similarity {
        Similarity {
            a: self.a * first.a + self.b * first.c,
            b: self.a * first.b + self.b * first.d,
            c: self.c * first.a + self.d * first.c,
            d: self.c * first.b + self.d * first.d,
            tx: self.a * first.tx + self.b * first.ty + self.tx,
            ty: self.c * first.tx + self.d * first.ty + self.ty,
        }
    }

    /// Whether it keeps shapes, only moving, turning, mirroring and scaling them the same in every direction.
    pub fn is_uniform(&self) -> bool {
        let (x, y) = (self.linear(pt(1.0, 0.0)), self.linear(pt(0.0, 1.0)));
        let tolerance = 1e-9 * (x.length() + y.length()).max(1e-300);
        (x.length() - y.length()).abs() <= tolerance && x.dot(y).abs() <= tolerance * x.length().max(1.0)
    }
}

pub const IDENTITY: Similarity = Similarity { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: 0.0, ty: 0.0 };

/// Where a block's entities go for an insert: from the block's base point, scaled, turned, then moved to the
/// insertion point.
pub fn insert_transform(base: Point, position: Point, scale_x: f64, scale_y: f64, rotation: f64) -> Similarity {
    let (sin, cos) = rotation.to_radians().sin_cos();
    let (a, b, c, d) = (cos * scale_x, -sin * scale_y, sin * scale_x, cos * scale_y);
    Similarity { a, b, c, d, tx: position.x - (a * base.x + b * base.y), ty: position.y - (c * base.x + d * base.y) }
}

pub fn translation(dx: f64, dy: f64) -> Similarity {
    Similarity { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: dx, ty: dy }
}

/// The linear map with the given matrix that leaves the center point where it is.
fn about(center: Point, a: f64, b: f64, c: f64, d: f64) -> Similarity {
    let (x, y) = (center.x, center.y);
    Similarity { a, b, c, d, tx: x - (a * x + b * y), ty: y - (c * x + d * y) }
}

pub fn rotation(center: Point, degrees: f64) -> Similarity {
    let (sin, cos) = degrees.to_radians().sin_cos();
    about(center, cos, -sin, sin, cos)
}

pub fn scaling(base: Point, factor: f64) -> Similarity {
    about(base, factor, 0.0, 0.0, factor)
}

/// Mirror across the line through two different points.
pub fn reflection(first: Point, second: Point) -> Similarity {
    let twice = 2.0 * (second.y - first.y).atan2(second.x - first.x);
    about(first, twice.cos(), twice.sin(), twice.sin(), -twice.cos())
}

/// The shape after the transform. Text keeps its direction when mirrored, so it stays readable.
pub fn transform_shape(shape: &Shape, sim: &Similarity) -> Shape {
    match shape {
        Shape::Line { start, end } => Shape::Line { start: sim.point(*start), end: sim.point(*end) },
        Shape::Region { loops } => Shape::Region {
            loops: crate::model::region::loop_shapes(loops)
                .filter_map(|l| match transform_shape(&l, sim) {
                    Shape::Polyline { vertices, .. } => Some(vertices),
                    _ => None,
                })
                .collect(),
        },
        Shape::Circle { center, radius } => Shape::Circle { center: sim.point(*center), radius: radius * sim.scale() },
        Shape::Arc { center, radius, start_angle, end_angle } => {
            let (mut first, mut last) = (sim.angle(*start_angle), sim.angle(*end_angle));
            if sim.reflects() {
                std::mem::swap(&mut first, &mut last);
            }
            Shape::Arc { center: sim.point(*center), radius: radius * sim.scale(), start_angle: first, end_angle: last }
        }
        Shape::Polyline { vertices, closed } => {
            let flip = if sim.reflects() { -1.0 } else { 1.0 };
            let vertices = vertices
                .iter()
                .map(|v| {
                    let moved = sim.point(v.point());
                    Vertex {
                        x: moved.x,
                        y: moved.y,
                        bulge: v.bulge * flip,
                        start_width: v.start_width * sim.scale(),
                        end_width: v.end_width * sim.scale(),
                    }
                })
                .collect();
            Shape::Polyline { vertices, closed: *closed }
        }
        Shape::Text { position, text, height, rotation, font, style, align, line_spacing, formatted, width_factor, oblique } => {
            Shape::Text {
                formatted: formatted.clone(),
                width_factor: *width_factor,
                oblique: *oblique,
                position: sim.point(*position),
                text: text.clone(),
                font: font.clone(),
                style: style.clone(),
                height: height * sim.scale(),
                rotation: if sim.reflects() { *rotation } else { sim.angle(*rotation) },
                align: *align,
                line_spacing: *line_spacing,
            }
        }
        Shape::Dimension { start, end, line_point, angle, look } => Shape::Dimension {
            start: sim.point(*start),
            end: sim.point(*end),
            line_point: sim.point(*line_point),
            angle: sim.angle(*angle),
            look: DimensionLook { text_position: look.text_position.map(|p| sim.point(p)), ..look.clone() },
        },
        // Tables stay square to the page: they move, and scale with their text.
        Shape::Table { position, columns, rows, cells, text_height, title } => Shape::Table {
            position: sim.point(*position),
            columns: columns.iter().map(|c| c * sim.scale()).collect(),
            rows: rows.iter().map(|r| r * sim.scale()).collect(),
            cells: cells.clone(),
            text_height: text_height * sim.scale(),
            title: *title,
        },
        Shape::Image { path, position, u, v, pixels } => {
            Shape::Image { path: path.clone(), position: sim.point(*position), u: sim.linear(*u), v: sim.linear(*v), pixels: *pixels }
        }
        Shape::XLine { base, direction, ray } => {
            let turned = sim.linear(*direction);
            Shape::XLine { base: sim.point(*base), direction: turned * (1.0 / turned.length()), ray: *ray }
        }
        // Frames stay square to the sheet; scaling one keeps the scale it shows model space at.
        Shape::Viewport { center, width, height, view_center, view_height, frozen, locked } => Shape::Viewport {
            center: sim.point(*center),
            width: width * sim.scale(),
            height: height * sim.scale(),
            view_center: *view_center,
            view_height: view_height * sim.scale(),
            frozen: frozen.clone(),
            locked: *locked,
        },
        Shape::Multileader { leaders, landing, dogleg, text, text_position, text_height, arrow_size, style } => {
            // The dogleg and text stay level; a mirror sends them to the other side.
            let new_landing = sim.point(*landing);
            let new_dogleg = dogleg * sim.scale() * if sim.linear(pt(1.0, 0.0)).x < 0.0 { -1.0 } else { 1.0 };
            let height = text_height * sim.scale();
            let gap = ((text_position.x - landing.x).abs() - dogleg.abs()).max(0.0) * sim.scale();
            Shape::Multileader {
                leaders: leaders.iter().map(|l| l.iter().map(|p| sim.point(*p)).collect()).collect(),
                landing: new_landing,
                dogleg: new_dogleg,
                text: text.clone(),
                text_position: super::multileader::text_corner(new_landing, new_dogleg, height, gap),
                text_height: height,
                arrow_size: arrow_size * sim.scale(),
                style: style.clone(),
            }
        }
        Shape::Annotation { kind, points, look } => Shape::Annotation {
            kind: *kind,
            points: {
                let mut placed: Vec<Point> = points.iter().map(|p| sim.point(*p)).collect();
                // A mirror runs the arc the other way round, and its ends stay counter-clockwise by swapping.
                if *kind == crate::model::annotation::AnnotationKind::ArcLength && sim.reflects() && placed.len() > 2 {
                    placed.swap(1, 2);
                }
                placed
            },
            look: DimensionLook { text_position: look.text_position.map(|p| sim.point(p)), ..look.clone() },
        },
        Shape::Ellipse { center, major, ratio, start, end } => {
            // A mirror runs the ellipse the other way round, so the same points come from the negated parameters.
            let (start, end) = if sim.reflects() { (-end, -start) } else { (*start, *end) };
            Shape::Ellipse { center: sim.point(*center), major: sim.linear(*major), ratio: *ratio, start, end }
        }
        Shape::Spline { degree, control, knots, weights, closed } => Shape::Spline {
            degree: *degree,
            control: control.iter().map(|p| sim.point(*p)).collect(),
            knots: knots.clone(),
            weights: weights.clone(),
            closed: *closed,
        },
        Shape::Point { position } => Shape::Point { position: sim.point(*position) },
        Shape::Solid { corners } => Shape::Solid { corners: corners.iter().map(|p| sim.point(*p)).collect() },
        Shape::Wipeout { corners } => Shape::Wipeout { corners: corners.iter().map(|p| sim.point(*p)).collect() },
        Shape::Mline { vertices, closed, justification, scale, style, gaps } => {
            // A mirror turns left into right: running the path the other way puts each line back on its side, and
            // each gap is then measured from the other end of its segment.
            let mirrored = sim.determinant() < 0.0;
            let gaps = if mirrored {
                crate::model::mline::reversed_gaps(vertices, *closed, *justification, *scale, style, gaps)
            } else {
                gaps.clone()
            };
            let mut vertices: Vec<Point> = vertices.iter().map(|p| sim.point(*p)).collect();
            if mirrored {
                vertices.reverse();
            }
            let gaps =
                gaps.iter().map(|g| crate::model::mline::MlineGap { from: g.from * sim.scale(), to: g.to * sim.scale(), ..*g }).collect();
            Shape::Mline {
                vertices,
                closed: *closed,
                justification: *justification,
                scale: scale * sim.scale(),
                style: style.clone(),
                gaps,
            }
        }
        Shape::Hatch { loops, solid, pattern, name, scale, angle, gradient, association } => Shape::Hatch {
            scale: scale * sim.scale(),
            angle: sim.angle(*angle),
            gradient: gradient.clone().map(|g| {
                let along = sim.linear(direction(g.angle));
                Gradient { angle: along.y.atan2(along.x).to_degrees(), ..g }
            }),
            association: match association {
                Some(Association::Seeds(seeds)) => Some(Association::Seeds(seeds.iter().map(|p| sim.point(*p)).collect())),
                other => other.clone(),
            },
            loops: loops.iter().map(|l| l.iter().map(|p| sim.point(*p)).collect()).collect(),
            solid: *solid,
            pattern: pattern
                .iter()
                .map(|line| PatternLine {
                    angle: sim.angle(line.angle),
                    origin: sim.point(line.origin),
                    offset: sim.linear(line.offset),
                    dashes: line.dashes.iter().map(|d| d * sim.scale()).collect(),
                })
                .collect(),
            name: name.clone(),
        },
        Shape::Insert { block, position, scale_x, scale_y, rotation, attributes } => {
            // The transform turns and scales the insert's direction; a mirror also turns its Y scale over.
            let turned = sim.linear(direction(*rotation));
            let flip = if sim.reflects() { -1.0 } else { 1.0 };
            Shape::Insert {
                block: block.clone(),
                position: sim.point(*position),
                scale_x: scale_x * sim.scale(),
                scale_y: scale_y * sim.scale() * flip,
                rotation: modulo(turned.y.atan2(turned.x).to_degrees(), 360.0),
                attributes: attributes.iter().map(|a| Attribute { text: transform_shape(&a.text, sim), ..a.clone() }).collect(),
            }
        }
    }
}

/// A transformed copy: a new object with the same layer and look.
pub fn transform_entity(entity: &Entity, sim: &Similarity) -> Entity {
    entity.derive(transform_shape(&entity.shape, sim))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::entities::{arc, line};

    fn near(a: Point, b: Point) -> bool {
        a.dist(b) < 1e-9
    }

    #[test]
    fn rotate_a_line_about_a_point() {
        let moved = transform_entity(&line(pt(1.0, 0.0), pt(2.0, 0.0)), &rotation(pt(1.0, 0.0), 90.0));
        let Shape::Line { start, end } = moved.shape else { panic!() };
        assert!(near(start, pt(1.0, 0.0)) && near(end, pt(1.0, 1.0)));
    }

    #[test]
    fn mirroring_an_arc_keeps_it_counter_clockwise() {
        let mirrored = transform_entity(&arc(pt(0.0, 0.0), 1.0, 0.0, 90.0), &reflection(pt(0.0, 0.0), pt(0.0, 1.0)));
        let Shape::Arc { start_angle, end_angle, .. } = mirrored.shape else { panic!() };
        assert!((start_angle - 90.0).abs() < 1e-9 && (end_angle - 180.0).abs() < 1e-9);
    }

    #[test]
    fn transforms_compose_and_inserts_keep_their_placement() {
        let insert = insert_transform(pt(1.0, 1.0), pt(10.0, 0.0), 2.0, 3.0, 90.0);
        assert!(insert.point(pt(1.0, 1.0)).dist(pt(10.0, 0.0)) < 1e-12);
        assert!(insert.point(pt(2.0, 1.0)).dist(pt(10.0, 2.0)) < 1e-12);
        assert!(!insert.is_uniform());
        let both = translation(1.0, 0.0).after(&rotation(pt(0.0, 0.0), 90.0));
        assert!(both.point(pt(1.0, 0.0)).dist(pt(1.0, 1.0)) < 1e-12);
        assert!(rotation(pt(3.0, 4.0), 33.0).after(&scaling(pt(0.0, 0.0), 2.0)).is_uniform());
    }

    #[test]
    fn moving_an_insert_keeps_where_its_block_lands() {
        use crate::model::entities::{BlockDef, BlockRef};
        let block = BlockRef(std::sync::Arc::new(BlockDef {
            name: "B".into(),
            base: pt(0.0, 0.0),
            entities: vec![],
            attribute_defs: vec![],
            xref: None,
        }));
        let shape = Shape::Insert { block, position: pt(5.0, 5.0), scale_x: 2.0, scale_y: 1.0, rotation: 30.0, attributes: vec![] };
        for sim in [rotation(pt(1.0, 2.0), 50.0), reflection(pt(0.0, 0.0), pt(1.0, 3.0)), scaling(pt(2.0, 2.0), 0.5)] {
            let Shape::Insert { position, scale_x, scale_y, rotation, .. } = transform_shape(&shape, &sim) else { panic!() };
            let before = insert_transform(pt(0.0, 0.0), pt(5.0, 5.0), 2.0, 1.0, 30.0);
            let after = insert_transform(pt(0.0, 0.0), position, scale_x, scale_y, rotation);
            for p in [pt(1.0, 0.0), pt(0.0, 1.0), pt(2.0, -3.0)] {
                assert!(after.point(p).dist(sim.point(before.point(p))) < 1e-9, "{sim:?} {p:?}");
            }
        }
    }

    #[test]
    fn scaling_grows_radius() {
        let sim = scaling(pt(0.0, 0.0), 3.0);
        assert!((sim.scale() - 3.0).abs() < 1e-12);
        assert!(!sim.reflects());
        assert!(reflection(pt(0.0, 0.0), pt(1.0, 1.0)).reflects());
    }
}
