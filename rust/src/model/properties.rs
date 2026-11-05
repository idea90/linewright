//! An object's properties as the Properties palette shows them, and changing one by typing a new value.

use super::annotation::{AnnotationKind, annotation_measurement};
use super::dimension::{DimensionLook, Tolerance, measurement};
use super::entities::{Entity, Shape};
use super::geometry::{Point, direction, direction as direction_of, modulo, pt, sweep_degrees};
use super::measure::measure_entity;
use super::parts::entity_parts;
use super::viewport::viewport_scale;

#[derive(Clone, Debug, PartialEq)]
pub struct Property {
    pub name: String,
    pub value: String,
    pub editable: bool,
}

fn number(value: f64) -> String {
    let text = format!("{value:.4}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" { "0".into() } else { text.into() }
}

fn editable(name: &str, value: f64) -> Property {
    Property { name: name.into(), value: number(value), editable: true }
}

fn text_field(name: &str, value: &str) -> Property {
    Property { name: name.into(), value: value.replace('\n', "\\P"), editable: true }
}

fn fixed(name: &str, value: String) -> Property {
    Property { name: name.into(), value, editable: false }
}

fn point_fields(name: &str, p: Point) -> [Property; 2] {
    [editable(&format!("{name} X"), p.x), editable(&format!("{name} Y"), p.y)]
}

/// The sizes of a dimension or leader, and the text over its measurement when it has one.
fn look_rows(look: &DimensionLook, measures: bool) -> Vec<Property> {
    let mut rows = vec![];
    if measures {
        rows.push(text_field("Text override", &look.text));
    }
    rows.push(editable("Dimension scale", look.scale));
    if measures {
        rows.push(editable("Text height", look.text_height));
    }
    rows.push(editable("Arrow size", look.arrow_size));
    if measures {
        rows.push(Property { name: "Tolerance".into(), value: look.tolerance.name().into(), editable: true });
        rows.push(editable("Tolerance plus", look.tolerance_plus));
        rows.push(editable("Tolerance minus", look.tolerance_minus));
        rows.push(editable("Tolerance height", look.tolerance_height));
    }
    rows
}

/// The geometry rows of an object, editable or for information.
pub fn properties(entity: &Entity) -> Vec<Property> {
    let mut rows = vec![];
    match &entity.shape {
        Shape::Line { start, end } => {
            rows.extend(point_fields("Start", *start));
            rows.extend(point_fields("End", *end));
            rows.push(fixed("Length", number(start.dist(*end))));
            rows.push(fixed("Angle", number(modulo(start.angle_to(*end), 360.0))));
        }
        Shape::Circle { center, radius } => {
            rows.extend(point_fields("Center", *center));
            rows.push(editable("Radius", *radius));
            rows.push(editable("Diameter", radius * 2.0));
        }
        Shape::Arc { center, radius, start_angle, end_angle } => {
            rows.extend(point_fields("Center", *center));
            rows.push(editable("Radius", *radius));
            rows.push(editable("Start angle", *start_angle));
            rows.push(editable("End angle", *end_angle));
            rows.push(fixed("Arc length", number(radius * sweep_degrees(*start_angle, *end_angle).to_radians())));
        }
        Shape::Polyline { vertices, closed } => {
            rows.push(text_field("Closed", if *closed { "Yes" } else { "No" }));
            rows.push(fixed("Vertices", vertices.len().to_string()));
            let width = vertices.first().map_or(0.0, |v| v.start_width);
            if vertices.iter().all(|v| v.start_width == width && v.end_width == width) {
                rows.push(editable("Global width", width));
            } else {
                rows.push(Property { name: "Global width".into(), value: "*VARIES*".into(), editable: true });
            }
            let length: f64 = entity_parts(entity, 1e-6).paths.iter().flat_map(|p| p.windows(2).map(|w| w[0].dist(w[1]))).sum();
            rows.push(fixed("Length", number(length)));
        }
        Shape::Text { position, text, height, rotation, style, width_factor, oblique, .. } => {
            rows.push(text_field("Contents", text));
            rows.push(fixed("Style", if style.is_empty() { "Standard".into() } else { style.clone() }));
            rows.push(editable("Width factor", *width_factor));
            rows.push(editable("Oblique angle", *oblique));
            rows.extend(point_fields("Position", *position));
            rows.push(editable("Height", *height));
            rows.push(editable("Rotation", *rotation));
        }
        Shape::Dimension { start, end, angle, look, .. } => {
            rows.push(fixed("Measurement", number(measurement(*start, *end, *angle) * look.length_factor)));
            rows.extend(look_rows(look, true));
        }
        Shape::Table { position, columns, rows: heights, text_height, .. } => {
            rows.extend(point_fields("Position", *position));
            rows.push(editable("Rows", heights.len() as f64));
            rows.push(editable("Columns", columns.len() as f64));
            rows.push(editable("Row height", heights.last().copied().unwrap_or(0.0)));
            rows.push(editable("Column width", columns.last().copied().unwrap_or(0.0)));
            rows.push(editable("Text height", *text_height));
        }
        Shape::Image { path, position, u, v, pixels } => {
            rows.push(fixed("File", path.clone()));
            rows.push(fixed("Pixels", format!("{} x {}", pixels.0, pixels.1)));
            rows.extend(point_fields("Position", *position));
            rows.push(editable("Width", u.length()));
            rows.push(editable("Height", v.length()));
        }
        Shape::XLine { base, direction, .. } => {
            rows.extend(point_fields("Base point", *base));
            rows.push(editable("Angle", modulo(direction.y.atan2(direction.x).to_degrees(), 360.0)));
        }
        Shape::Viewport { center, width, height, view_center, view_height, .. } => {
            rows.extend(point_fields("Center", *center));
            rows.push(editable("Width", *width));
            rows.push(editable("Height", *height));
            rows.push(editable("Scale", viewport_scale(*height, *view_height)));
            rows.extend(point_fields("View center", *view_center));
            if let Shape::Viewport { locked, .. } = &entity.shape {
                rows.push(Property { name: "Display locked".into(), value: if *locked { "Yes" } else { "No" }.into(), editable: true });
            }
        }
        Shape::Multileader { text, text_height, arrow_size, dogleg, style, leaders, .. } => {
            rows.push(text_field("Contents", text));
            rows.push(fixed("Style", style.clone()));
            rows.push(fixed("Leaders", leaders.len().to_string()));
            rows.push(editable("Text height", *text_height));
            rows.push(editable("Arrow size", *arrow_size));
            rows.push(editable("Dogleg length", dogleg.abs()));
        }
        Shape::Annotation { kind, points, look } => {
            let measured = annotation_measurement(*kind, points);
            if let Some(value) = measured {
                let factor = if *kind == AnnotationKind::Angle { 1.0 } else { look.length_factor };
                rows.push(fixed("Measurement", number(value * factor)));
            }
            rows.extend(look_rows(look, measured.is_some()));
            // Tolerances are lengths: angles do not show them.
            if *kind == AnnotationKind::Angle {
                rows.retain(|row| !row.name.starts_with("Tolerance"));
            }
        }
        Shape::Ellipse { center, major, ratio, .. } => {
            rows.extend(point_fields("Center", *center));
            rows.push(editable("Major radius", major.length()));
            rows.push(editable("Minor radius", major.length() * ratio));
        }
        Shape::Spline { degree, control, .. } => {
            rows.push(fixed("Degree", degree.to_string()));
            rows.push(fixed("Control points", control.len().to_string()));
        }
        Shape::Point { position } => rows.extend(point_fields("Position", *position)),
        Shape::Solid { corners } | Shape::Wipeout { corners } => rows.push(fixed("Corners", corners.len().to_string())),
        Shape::Mline { vertices, closed, justification, scale, style } => {
            rows.push(fixed("Style", style.name.clone()));
            rows.push(fixed("Justification", justification.name().to_string()));
            rows.push(editable("Scale", *scale));
            rows.push(text_field("Closed", if *closed { "Yes" } else { "No" }));
            rows.push(fixed("Vertices", vertices.len().to_string()));
        }
        Shape::Hatch { loops, solid, name, scale, angle, gradient, association, .. } => {
            match gradient {
                Some(gradient) => {
                    rows.push(fixed("Gradient", gradient.name.clone()));
                    rows.push(editable("Gradient angle", gradient.angle));
                }
                None => {
                    rows.push(text_field("Pattern", if *solid { "SOLID" } else { name }));
                    if !*solid {
                        rows.push(editable("Pattern scale", *scale));
                        rows.push(editable("Pattern angle", *angle));
                    }
                }
            }
            rows.push(fixed("Associative", if association.is_some() { "Yes".into() } else { "No".into() }));
            rows.push(fixed("Boundary loops", loops.len().to_string()));
        }
        Shape::Insert { block, position, scale_x, scale_y, rotation, attributes } => {
            rows.push(fixed("Block name", block.0.name.clone()));
            rows.extend(point_fields("Position", *position));
            rows.push(editable("Scale X", *scale_x));
            rows.push(editable("Scale Y", *scale_y));
            rows.push(editable("Rotation", *rotation));
            for attribute in attributes {
                if let Shape::Text { text, .. } = &attribute.text {
                    rows.push(text_field(&format!("Attribute {}", attribute.tag), text));
                }
            }
        }
    }
    if let Some(info) = &entity.attribute {
        rows.push(text_field("Prompt", &info.prompt));
        rows.push(text_field("Default", &info.default));
        rows.push(text_field("Invisible", if info.invisible { "Yes" } else { "No" }));
    }
    if let Some(measured) = measure_entity(entity) {
        rows.push(fixed("Area", number(measured.area)));
    }
    rows
}

/// Shown for a property whose value differs between the selected objects, as AutoCAD does.
pub const VARIES: &str = "*VARIES*";

/// The rows that objects of one kind share, for the Properties palette with several objects selected: each value
/// where they all agree, *VARIES* where they do not. Objects of different kinds share none.
pub fn shared_properties(entities: &[Entity]) -> Vec<Property> {
    let Some((first, rest)) = entities.split_first() else { return vec![] };
    if rest.iter().any(|e| e.shape.kind() != first.shape.kind()) {
        return vec![];
    }
    let others: Vec<Vec<Property>> = rest.iter().map(properties).collect();
    properties(first)
        .into_iter()
        .filter_map(|mut row| {
            for rows in &others {
                let other = rows.iter().find(|r| r.name == row.name)?;
                row.editable &= other.editable;
                if other.value != row.value {
                    row.value = VARIES.into();
                }
            }
            Some(row)
        })
        .collect()
}

/// The object with one property set from typed text. The error is a message for the user.
pub fn set_property(entity: &Entity, name: &str, typed: &str) -> Result<Entity, String> {
    let typed = typed.trim();
    let value = || typed.parse::<f64>().ok().filter(|v| v.is_finite()).ok_or_else(|| format!("{name} needs a number."));
    let positive = || value().and_then(|v| if v > 0.0 { Ok(v) } else { Err(format!("{name} must be more than zero.")) });
    let set_x = |p: &mut Point, v: f64| *p = pt(v, p.y);
    let set_y = |p: &mut Point, v: f64| *p = pt(p.x, v);
    if let Some(info) = &entity.attribute
        && matches!(name, "Prompt" | "Default" | "Invisible")
    {
        let mut info = info.clone();
        match name {
            "Prompt" => info.prompt = typed.to_string(),
            "Default" => info.default = typed.to_string(),
            _ => info.invisible = matches!(typed.to_lowercase().as_str(), "yes" | "y" | "true" | "1"),
        }
        return Ok(Entity { attribute: Some(info), ..entity.clone() });
    }
    let mut shape = entity.shape.clone();
    let mut resize: Option<(usize, usize)> = None;
    let unknown = || Err(format!("{name} cannot be changed here."));
    // Which coordinate a point field sets: "Start X" sets the x of `start`.
    let axis = |p: &mut Point, v: f64| if name.ends_with(" X") { set_x(p, v) } else { set_y(p, v) };
    match &mut shape {
        Shape::Line { start, end } => match name {
            "Start X" | "Start Y" => axis(start, value()?),
            "End X" | "End Y" => axis(end, value()?),
            _ => return unknown(),
        },
        Shape::Circle { center, radius } => match name {
            "Center X" | "Center Y" => axis(center, value()?),
            "Radius" => *radius = positive()?,
            "Diameter" => *radius = positive()? / 2.0,
            _ => return unknown(),
        },
        Shape::Arc { center, radius, start_angle, end_angle } => match name {
            "Center X" | "Center Y" => axis(center, value()?),
            "Radius" => *radius = positive()?,
            "Start angle" => *start_angle = modulo(value()?, 360.0),
            "End angle" => *end_angle = modulo(value()?, 360.0),
            _ => return unknown(),
        },
        Shape::Polyline { closed, vertices } => match name {
            "Global width" => {
                let width = value()?;
                if width < 0.0 {
                    return Err("A width cannot be negative.".to_string());
                }
                for vertex in vertices.iter_mut() {
                    (vertex.start_width, vertex.end_width) = (width, width);
                }
            }
            "Closed" => *closed = matches!(typed.to_lowercase().as_str(), "yes" | "y" | "true" | "1"),
            _ => return unknown(),
        },
        Shape::Text { position, text, height, rotation, width_factor, oblique, .. } => match name {
            "Width factor" => *width_factor = positive()?,
            "Oblique angle" => *oblique = value()?.clamp(-85.0, 85.0),
            "Contents" => *text = typed.replace("\\P", "\n"),
            "Position X" | "Position Y" => axis(position, value()?),
            "Height" => *height = positive()?,
            "Rotation" => *rotation = value()?,
            _ => return unknown(),
        },
        // Rows and columns are added or taken away at the end; a size given is set for every row or column.
        Shape::Table { position, columns, rows, text_height, .. } => {
            let whole = || {
                value()
                    .ok()
                    .filter(|v| v.fract() == 0.0 && (1.0..=500.0).contains(v))
                    .map(|v| v as usize)
                    .ok_or_else(|| format!("{name} needs a whole number from 1 to 500."))
            };
            match name {
                "Position X" | "Position Y" => axis(position, value()?),
                "Row height" => {
                    let height = positive()?;
                    rows.iter_mut().for_each(|r| *r = height);
                }
                "Column width" => {
                    let width = positive()?;
                    columns.iter_mut().for_each(|c| *c = width);
                }
                "Text height" => *text_height = positive()?,
                "Rows" => resize = Some((whole()?, columns.len())),
                "Columns" => resize = Some((rows.len(), whole()?)),
                _ => return unknown(),
            }
        }
        // Width and height scale the image as a whole, keeping its proportions.
        Shape::Image { position, u, v, .. } => match name {
            "Position X" | "Position Y" => axis(position, value()?),
            "Width" => {
                let factor = positive()? / u.length().max(f64::MIN_POSITIVE);
                (*u, *v) = (*u * factor, *v * factor);
            }
            "Height" => {
                let factor = positive()? / v.length().max(f64::MIN_POSITIVE);
                (*u, *v) = (*u * factor, *v * factor);
            }
            _ => return unknown(),
        },
        Shape::XLine { base, direction, .. } => match name {
            "Base point X" | "Base point Y" => axis(base, value()?),
            "Angle" => *direction = direction_of(value()?),
            _ => return unknown(),
        },
        Shape::Viewport { center, width, height, view_center, view_height, locked, .. } => {
            let scale = viewport_scale(*height, *view_height);
            match name {
                "Display locked" => *locked = matches!(typed.to_lowercase().as_str(), "yes" | "y" | "true" | "1" | "on"),
                "Center X" | "Center Y" => axis(center, value()?),
                "View center X" | "View center Y" => axis(view_center, value()?),
                "Width" => *width = positive()?,
                "Height" => (*height, *view_height) = (positive()?, positive()? / scale),
                // Sheet units per model unit: 0.01 shows model space at 1:100 on a sheet in the same units.
                "Scale" => *view_height = *height / positive()?,
                _ => return unknown(),
            }
        }
        Shape::Hatch { name: pattern_name, scale, angle, gradient, .. } => match name {
            "Gradient angle" => {
                if let Some(gradient) = gradient {
                    gradient.angle = value()?;
                }
            }
            "Pattern" | "Pattern scale" | "Pattern angle" => {
                let (mut new_name, mut new_scale, mut new_angle) = (pattern_name.clone(), *scale, *angle);
                match name {
                    "Pattern" => new_name = typed.to_string(),
                    "Pattern scale" => new_scale = positive()?,
                    _ => new_angle = value()?,
                }
                shape = crate::model::editing::rehatched(&entity.shape, &new_name, new_scale, new_angle)
                    .ok_or_else(|| format!("There is no hatch pattern called {new_name}."))?;
            }
            _ => return unknown(),
        },
        Shape::Multileader { text, text_height, arrow_size, dogleg, landing, text_position, .. } => {
            let gap = ((text_position.x - landing.x).abs() - dogleg.abs()).max(0.0);
            match name {
                "Contents" => *text = typed.replace("\\P", "\n"),
                "Text height" => *text_height = positive()?,
                "Arrow size" => *arrow_size = positive()?,
                "Dogleg length" => *dogleg = value()?.max(0.0) * if *dogleg < 0.0 { -1.0 } else { 1.0 },
                _ => return unknown(),
            }
            *text_position = super::multileader::text_corner(*landing, *dogleg, *text_height, gap);
        }
        Shape::Dimension { look, .. } | Shape::Annotation { look, .. } => match name {
            "Text override" => look.text = if typed == "<>" { String::new() } else { typed.to_string() },
            // A new size lays the text out afresh, centered again, as the old place was worked out for the old size.
            "Dimension scale" => (look.scale, look.text_position) = (positive()?, None),
            "Text height" => (look.text_height, look.text_position) = (positive()?, None),
            "Arrow size" => look.arrow_size = positive()?,
            "Tolerance" => {
                look.tolerance = Tolerance::ALL
                    .into_iter()
                    .find(|t| t.name().to_lowercase().starts_with(&typed.to_lowercase()) && !typed.is_empty())
                    .ok_or_else(|| "Tolerance is None, Symmetrical, Deviation or Limits.".to_string())?
            }
            "Tolerance plus" => look.tolerance_plus = value()?,
            "Tolerance minus" => look.tolerance_minus = value()?,
            "Tolerance height" => look.tolerance_height = positive()?,
            _ => return unknown(),
        },
        Shape::Ellipse { center, major, ratio, .. } => match name {
            "Center X" | "Center Y" => axis(center, value()?),
            "Major radius" => {
                let minor = major.length() * *ratio;
                let new = positive()?;
                *major = direction(major.y.atan2(major.x).to_degrees()) * new;
                *ratio = minor / new;
            }
            "Minor radius" => *ratio = positive()? / major.length().max(1e-300),
            _ => return unknown(),
        },
        Shape::Point { position } => match name {
            "Position X" | "Position Y" => axis(position, value()?),
            _ => return unknown(),
        },
        Shape::Mline { scale, closed, .. } => match name {
            "Scale" => *scale = value()?,
            "Closed" => *closed = matches!(typed.to_lowercase().as_str(), "yes" | "y" | "true" | "1"),
            _ => return unknown(),
        },
        Shape::Insert { position, scale_x, scale_y, rotation, attributes, .. } => {
            if let Some(tag) = name.strip_prefix("Attribute ") {
                let attribute = attributes.iter_mut().find(|a| a.tag == tag).ok_or_else(|| format!("There is no attribute {tag}."))?;
                if let Shape::Text { text, .. } = &mut attribute.text {
                    *text = typed.to_string();
                }
            } else {
                // Moving, scaling or turning an insert carries its attributes along.
                let before = super::transform::insert_transform(pt(0.0, 0.0), *position, *scale_x, *scale_y, *rotation);
                match name {
                    "Position X" | "Position Y" => axis(position, value()?),
                    "Scale X" => {
                        *scale_x = value().and_then(|v| if v != 0.0 { Ok(v) } else { Err("A scale cannot be zero.".to_string()) })?
                    }
                    "Scale Y" => {
                        *scale_y = value().and_then(|v| if v != 0.0 { Ok(v) } else { Err("A scale cannot be zero.".to_string()) })?
                    }
                    "Rotation" => *rotation = value()?,
                    _ => return unknown(),
                }
                let after = super::transform::insert_transform(pt(0.0, 0.0), *position, *scale_x, *scale_y, *rotation);
                let inverse = invert(&before);
                if let Some(inverse) = inverse {
                    let carry = after.after(&inverse);
                    if carry.is_uniform() {
                        for attribute in attributes.iter_mut() {
                            attribute.text = super::transform::transform_shape(&attribute.text, &carry);
                        }
                    }
                }
            }
        }
        _ => return unknown(),
    }
    if let Some((rows, columns)) = resize {
        shape = crate::model::table::resized(&shape, rows, columns).ok_or_else(|| format!("{name} cannot be zero."))?;
    }
    Ok(entity.with_shape(shape))
}

fn invert(t: &super::transform::Similarity) -> Option<super::transform::Similarity> {
    let det = t.determinant();
    if det.abs() < 1e-300 {
        return None;
    }
    let (a, b, c, d) = (t.d / det, -t.b / det, -t.c / det, t.a / det);
    Some(super::transform::Similarity { a, b, c, d, tx: -(a * t.tx + b * t.ty), ty: -(c * t.tx + d * t.ty) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::entities::{circle, line, text};

    #[test]
    fn a_line_shows_its_ends_and_length() {
        let rows = properties(&line(pt(0.0, 0.0), pt(3.0, 4.0)));
        assert!(rows.iter().any(|r| r.name == "Length" && r.value == "5" && !r.editable));
        let moved = set_property(&line(pt(0.0, 0.0), pt(3.0, 4.0)), "End X", "6").unwrap();
        assert_eq!(moved.shape, Shape::Line { start: pt(0.0, 0.0), end: pt(6.0, 4.0) });
    }

    #[test]
    fn bad_values_are_explained() {
        assert_eq!(set_property(&circle(pt(0.0, 0.0), 1.0), "Radius", "-2").unwrap_err(), "Radius must be more than zero.");
        assert_eq!(set_property(&circle(pt(0.0, 0.0), 1.0), "Diameter", "x").unwrap_err(), "Diameter needs a number.");
        let bigger = set_property(&circle(pt(0.0, 0.0), 1.0), "Diameter", "10").unwrap();
        assert_eq!(bigger.shape, Shape::Circle { center: pt(0.0, 0.0), radius: 5.0 });
    }

    #[test]
    fn text_contents_take_line_breaks() {
        let changed = set_property(&text(pt(0.0, 0.0), "a", 1.0, 0.0), "Contents", "one\\Ptwo").unwrap();
        let Shape::Text { text, .. } = changed.shape else { panic!() };
        assert_eq!(text, "one\ntwo");
    }

    #[test]
    fn objects_of_one_kind_share_their_rows() {
        let rows = shared_properties(&[circle(pt(0.0, 0.0), 1.0), circle(pt(5.0, 0.0), 1.0)]);
        let value = |name: &str| rows.iter().find(|r| r.name == name).map(|r| r.value.clone());
        assert_eq!(value("Radius").as_deref(), Some("1"));
        assert_eq!(value("Center X").as_deref(), Some(VARIES));
        assert_eq!(value("Center Y").as_deref(), Some("0"));
        assert!(shared_properties(&[circle(pt(0.0, 0.0), 1.0), line(pt(0.0, 0.0), pt(1.0, 0.0))]).is_empty());
    }
}
