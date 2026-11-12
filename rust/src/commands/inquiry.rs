//! Asking about the drawing and changing the view: LIST, ID and ZOOM; editing text; and grip editing.

use super::edit::objects_to_edit;
use super::prompts::{Answer, Io, Prompt, PromptKind, Step, ViewRequest};
use crate::model::annotation::{AnnotationKind, annotation_measurement};
use crate::model::dimension::measurement;
use crate::model::editing::{apply_grip, grip_points};
use crate::model::entities::{Entity, Shape};
use crate::model::geometry::{Point, sweep_degrees};
use crate::model::measure::measure_entity;
use crate::model::viewport::viewport_scale;

const ALL: &str = "A";
const EXTENTS: &str = "E";
const PREVIOUS: &str = "P";
const WINDOW: &str = "W";

fn xy(p: Point) -> String {
    format!("X = {:.4}  Y = {:.4}", p.x, p.y)
}

/// What LIST says about an object: what it is, its layer and look, and its geometry.
pub fn describe(entity: &Entity) -> Vec<String> {
    let color = crate::canvas::aci::color_name(entity.color).to_uppercase();
    let mut head =
        format!("{}  Layer: \"{}\"  Color: {color}  Linetype: {}", entity.shape.kind().to_uppercase(), entity.layer, entity.linetype);
    // AutoCAD lists a lineweight only when the object has its own.
    if entity.lineweight != crate::model::lineweight::BYLAYER {
        head.push_str(&format!("  Lineweight: {}", crate::model::lineweight::name(entity.lineweight)));
    }
    let mut lines = vec![head];
    let mut add = |line: String| lines.push(format!("    {line}"));
    match &entity.shape {
        Shape::Line { start, end } => {
            add(format!("from point, {}", xy(*start)));
            add(format!("to point, {}", xy(*end)));
            add(format!(
                "Length = {:.4}, Angle in XY Plane = {:.0}",
                start.dist(*end),
                crate::model::geometry::modulo(start.angle_to(*end), 360.0)
            ));
        }
        Shape::Circle { center, radius } => {
            add(format!("center point, {}", xy(*center)));
            add(format!("radius {radius:.4}"));
        }
        Shape::Arc { center, radius, start_angle, end_angle } => {
            add(format!("center point, {}", xy(*center)));
            add(format!("radius {radius:.4}  start angle {start_angle:.0}  end angle {end_angle:.0}"));
            add(format!("length {:.4}", radius * sweep_degrees(*start_angle, *end_angle).to_radians()));
        }
        Shape::Polyline { vertices, closed } => {
            add(format!("{} vertices, {}", vertices.len(), if *closed { "closed" } else { "open" }));
        }
        Shape::Region { loops } => {
            let measured = crate::model::region::measure(loops);
            add(format!("{} loop(s)  area {:.4}  perimeter {:.4}", loops.len(), measured.area, measured.perimeter));
        }
        Shape::Text { position, text, height, rotation, .. } => {
            add(format!("text \"{}\"", text.replace('\n', "\\P")));
            add(format!("position {}  height {height:.4}  rotation {rotation:.0}", xy(*position)));
        }
        Shape::Dimension { start, end, angle, .. } => add(format!("measurement {:.4}", measurement(*start, *end, *angle))),
        Shape::Table { position, columns, rows, .. } => {
            add(format!("{} rows by {} columns, top left corner {}", rows.len(), columns.len(), xy(*position)));
        }
        Shape::Image { path, position, u, v, pixels } => {
            add(format!("file \"{path}\"  {} x {} pixels", pixels.0, pixels.1));
            add(format!("insertion point, {}  width {:.4}  height {:.4}", xy(*position), u.length(), v.length()));
        }
        Shape::XLine { base, direction, .. } => {
            add(format!("base point, {}", xy(*base)));
            add(format!("direction, {}", xy(*direction)));
        }
        Shape::Viewport { center, width, height, view_center, view_height, .. } => {
            add(format!("center {}  width {width:.4}  height {height:.4}", xy(*center)));
            add(format!("view center {}  scale {:.6}", xy(*view_center), viewport_scale(*height, *view_height)));
        }
        Shape::Multileader { leaders, landing, text, style, .. } => {
            add(format!("style \"{style}\"  {} leader(s), landing {}", leaders.len(), xy(*landing)));
            add(format!("text \"{}\"", text.replace('\n', " ")));
        }
        Shape::Annotation { kind, points, .. } => match annotation_measurement(*kind, points) {
            Some(value) => add(format!("measurement {value:.4}")),
            None => add(format!("{} vertices, from {}", points.len(), points.first().map_or(String::new(), |p| xy(*p)))),
        },
        Shape::Ellipse { center, major, ratio, .. } => {
            add(format!("center {}", xy(*center)));
            add(format!("major radius {:.4}  minor radius {:.4}", major.length(), major.length() * ratio));
        }
        Shape::Spline { degree, control, .. } => add(format!("degree {degree}, {} control points", control.len())),
        Shape::Point { position } => add(format!("at point, {}", xy(*position))),
        Shape::Solid { corners } | Shape::Wipeout { corners } => add(format!("{} corners", corners.len())),
        Shape::Mline { vertices, justification, scale, style, .. } => add(format!(
            "style {}, justification {}, scale {}, {} vertices",
            style.name,
            justification.name(),
            crate::commands::format_general(*scale),
            vertices.len()
        )),
        Shape::Hatch { loops, solid, name, .. } => {
            add(format!("pattern {}, {} boundary loop(s)", if *solid { "SOLID" } else { name }, loops.len()))
        }
        Shape::Insert { block, position, scale_x, scale_y, rotation, attributes } => {
            add(format!("block name: \"{}\"", block.0.name));
            add(format!("at point, {}", xy(*position)));
            add(format!("X scale factor: {scale_x:.4}  Y scale factor: {scale_y:.4}  rotation angle: {rotation:.0}"));
            for attribute in attributes {
                if let Shape::Text { text, .. } = &attribute.text {
                    add(format!("attribute {}: {text}", attribute.tag));
                }
            }
        }
    }
    if let Some(measured) = measure_entity(entity) {
        lines.push(format!("    area {:.4}  perimeter {:.4}", measured.area, measured.perimeter));
    }
    lines
}

pub async fn list_command(io: Io, selected: Vec<Entity>) {
    for object in objects_to_edit(&io, selected).await {
        for line in describe(&object) {
            io.step(Step::Report(line));
        }
    }
}

/// ID: a point's coordinates in the current UCS.
pub async fn id_command(io: Io, defaults: std::rc::Rc<std::cell::RefCell<super::defaults::Defaults>>) {
    let ucs = defaults.borrow().ucs;
    if let Some(Answer::Point(point)) = io.ask(Prompt::point("Specify point:")).await {
        let local = ucs.to_ucs(point);
        // Adding nothing turns a minus zero, which rounding into the UCS can leave, into zero.
        io.step(Step::Report(xy(crate::model::geometry::pt(local.x + 0.0, local.y + 0.0))));
    }
}

pub async fn zoom_command(io: Io) {
    let prompt = Prompt::point("Specify corner of window or [All/Extents/Previous/Window]:").options(&[ALL, EXTENTS, PREVIOUS, WINDOW]);
    let first = match io.ask(prompt).await {
        Some(Answer::Keyword(k)) if k == ALL || k == EXTENTS => {
            io.step(Step::View(ViewRequest::Extents));
            return;
        }
        Some(Answer::Keyword(k)) if k == PREVIOUS => {
            io.step(Step::View(ViewRequest::Previous));
            return;
        }
        Some(Answer::Keyword(_)) => match io.ask(Prompt::point("Specify first corner:")).await {
            Some(Answer::Point(point)) => point,
            _ => return,
        },
        Some(Answer::Point(point)) => point,
        _ => return,
    };
    if let Some(Answer::Point(second)) = io.ask(Prompt::point("Specify opposite corner:").base(first)).await
        && second != first
    {
        io.step(Step::View(ViewRequest::Window(first, second)));
    }
}

fn edited_text(old: &str) -> Prompt {
    let shown = old.replace('\n', "\\P");
    Prompt::new(PromptKind::Text, format!("Enter new text <{shown}>:")).default(Answer::Text(shown))
}

/// Text typed on the command line, where \P starts a new line as in MTEXT.
fn typed_text(typed: &str) -> String {
    typed.replace("\\P", "\n")
}

/// Change a text, an insert's attribute values, or a dimension's text.
pub async fn textedit_command(io: Io, selected: Vec<Entity>) {
    let target = match selected.into_iter().next() {
        Some(entity) => entity,
        None => match io.ask(Prompt::new(PromptKind::Pick, "Select an annotation object:")).await {
            Some(Answer::Picked(picked)) => picked.entity,
            _ => return,
        },
    };
    let shape = match &target.shape {
        Shape::Text { text, .. } => {
            let Some(Answer::Text(typed)) = io.ask(edited_text(text)).await else { return };
            let mut shape = target.shape.clone();
            if let Shape::Text { text, .. } = &mut shape {
                *text = typed_text(&typed);
            }
            shape
        }
        Shape::Insert { attributes, .. } if !attributes.is_empty() => {
            let mut shape = target.shape.clone();
            let Shape::Insert { attributes, .. } = &mut shape else { return };
            for attribute in attributes.iter_mut() {
                let Shape::Text { text, .. } = &mut attribute.text else { continue };
                let prompt =
                    Prompt::new(PromptKind::Text, format!("Enter {} <{text}>:", attribute.tag)).default(Answer::Text(text.clone()));
                let Some(Answer::Text(typed)) = io.ask(prompt).await else { return };
                *text = typed;
            }
            shape
        }
        Shape::Multileader { text, .. } => {
            let prompt = Prompt::new(PromptKind::Text, format!("Enter text <{}>:", text.replace('\n', "\\P")))
                .default(Answer::Text(text.replace('\n', "\\P")));
            let Some(Answer::Text(typed)) = io.ask(prompt).await else { return };
            let mut shape = target.shape.clone();
            if let Shape::Multileader { text, .. } = &mut shape {
                *text = typed.replace("\\P", "\n");
            }
            shape
        }
        Shape::Dimension { look, .. }
        | Shape::Annotation {
            look,
            kind:
                AnnotationKind::Radius
                | AnnotationKind::Diameter
                | AnnotationKind::Angle
                | AnnotationKind::Ordinate { .. }
                | AnnotationKind::ArcLength
                | AnnotationKind::JoggedRadius,
            ..
        } => {
            let current = if look.text.is_empty() { "<>".to_string() } else { look.text.clone() };
            let prompt = Prompt::new(PromptKind::Text, format!("Enter dimension text (<> is the measurement) <{current}>:"))
                .default(Answer::Text(current));
            let Some(Answer::Text(typed)) = io.ask(prompt).await else { return };
            let mut shape = target.shape.clone();
            if let Shape::Dimension { look, .. } | Shape::Annotation { look, .. } = &mut shape {
                look.text = if typed.trim() == "<>" { String::new() } else { typed };
            }
            shape
        }
        _ => {
            io.step(Step::Report("That object has no text to edit.".into()));
            return;
        }
    };
    io.step(Step::Modify(vec![target.with_shape(shape)]));
}

/// Drag one of a selected object's grips to a new point.
pub async fn grip_command(io: Io, target: Entity, index: usize) {
    let Some(&from) = grip_points(&target.shape).get(index) else { return };
    let shown = target.clone();
    let prompt = Prompt::point("Specify stretch point:")
        .base(from)
        .preview(move |cursor| apply_grip(&shown.shape, index, cursor).map(|s| shown.derive(s)).into_iter().collect());
    if let Some(Answer::Point(to)) = io.ask(prompt).await
        && let Some(shape) = apply_grip(&target.shape, index, to)
    {
        io.step(Step::Modify(vec![target.with_shape(shape)]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::entities::line;
    use crate::model::geometry::pt;

    #[test]
    fn list_describes_a_line() {
        let lines = describe(&line(pt(0.0, 0.0), pt(3.0, 4.0)));
        assert!(lines[0].starts_with("LINE  Layer: \"0\""));
        assert!(lines.iter().any(|l| l.contains("Length = 5.0000")));
    }

    #[test]
    fn typed_text_breaks_lines_at_backslash_p() {
        assert_eq!(typed_text("first\\Psecond"), "first\nsecond");
    }
}
