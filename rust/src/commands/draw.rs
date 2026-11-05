use super::prompts::{Answer, Io, Prompt, PromptKind, Step};
use crate::model::entities::{Entity, Shape, Vertex, line, polyline, polyline_from, text as text_entity, vertex};
use crate::model::geometry::{Point, pt};

pub const DEFAULT_TEXT_HEIGHT: f64 = 2.5;
pub const DEFAULT_TEXT_ROTATION: f64 = 0.0;
const CLOSE: &str = "C";
const WIDTH: &str = "W";
const HALFWIDTH: &str = "H";
const UNDO: &str = "U";

fn rectangle_shape(first: Point, other: Point) -> Entity {
    polyline_from(&[first, pt(other.x, first.y), other, pt(first.x, other.y)], true)
}

pub fn line_preview(start: Point) -> impl Fn(Point) -> Vec<Entity> {
    move |cursor| vec![line(start, cursor)]
}

/// The prompt for the next point of a chain: Undo from the first segment, Close from the third point.
fn next_point_prompt(points: &[Point]) -> Prompt {
    let keywords: Vec<&str> = if points.len() >= 3 { vec![CLOSE, UNDO] } else { vec![UNDO] };
    let names: Vec<&str> = keywords.iter().map(|k| if *k == CLOSE { "Close" } else { "Undo" }).collect();
    Prompt::point(format!("Specify next point or [{}]:", names.join("/"))).base(points[points.len() - 1]).options(&keywords)
}

pub async fn line_command(io: Io) {
    let mut points: Vec<Point> = vec![];
    loop {
        if points.is_empty() {
            match io.ask(Prompt::point("Specify first point:")).await {
                Some(Answer::Point(first)) => points.push(first),
                _ => return,
            }
            continue;
        }
        let last = points[points.len() - 1];
        match io.ask(next_point_prompt(&points).preview(line_preview(last))).await {
            Some(Answer::Keyword(k)) if k == UNDO => {
                if points.len() > 1 {
                    io.step(Step::Retract);
                }
                points.pop();
            }
            Some(Answer::Keyword(k)) if k == CLOSE => {
                io.step(Step::Drawn(vec![line(last, points[0])]));
                return;
            }
            Some(Answer::Point(next)) => {
                io.step(Step::Drawn(vec![line(last, next)]));
                points.push(next);
            }
            _ => return,
        }
    }
}

/// A polyline with each point's segment at its widths.
fn wide_polyline(points: &[(Point, f64, f64)], closed: bool) -> Entity {
    let vertices = points
        .iter()
        .map(|(p, start_width, end_width)| Vertex { start_width: *start_width, end_width: *end_width, ..vertex(*p) })
        .collect();
    polyline(vertices, closed)
}

pub async fn polyline_command(io: Io) {
    let Some(Answer::Point(first)) = io.ask(Prompt::point("Specify start point:")).await else { return };
    // Each point with the widths its segment starts and ends at. Width and Halfwidth set the next segment's.
    let (mut start_width, mut end_width) = (0.0, 0.0);
    let mut points = vec![(first, start_width, end_width)];
    loop {
        let shown = points.clone();
        let message = if points.len() >= 3 {
            "Specify next point or [Halfwidth/Width/Close/Undo]:"
        } else {
            "Specify next point or [Halfwidth/Width/Undo]:"
        };
        let options: &[&str] = if points.len() >= 3 { &[HALFWIDTH, WIDTH, CLOSE, UNDO] } else { &[HALFWIDTH, WIDTH, UNDO] };
        let base = points[points.len() - 1].0;
        let prompt = Prompt::point(message).base(base).options(options).preview(move |cursor| {
            let mut path = shown.clone();
            path.push((cursor, 0.0, 0.0));
            vec![wide_polyline(&path, false)]
        });
        match io.ask(prompt).await {
            Some(Answer::Keyword(k)) if k == WIDTH || k == HALFWIDTH => {
                let factor = if k == WIDTH { 1.0 } else { 2.0 };
                let name = if k == WIDTH { "width" } else { "half-width" };
                let ask = |message: String, default: f64| {
                    let io = io.clone();
                    async move {
                        let prompt = Prompt::new(PromptKind::Length, format!("{message} <{}>:", super::format_general(default)))
                            .default(Answer::Number(default));
                        match io.ask(prompt).await {
                            Some(Answer::Number(value)) => Some(value.abs()),
                            _ => None,
                        }
                    }
                };
                let Some(start) = ask(format!("Specify starting {name}"), end_width / factor).await else { return };
                let Some(end) = ask(format!("Specify ending {name}"), start).await else { return };
                (start_width, end_width) = (start * factor, end * factor);
                if let Some(last) = points.last_mut() {
                    (last.1, last.2) = (start_width, end_width);
                }
                // After a taper, later segments carry on at the ending width, as in AutoCAD.
                start_width = end_width;
            }
            Some(Answer::Keyword(k)) if k == CLOSE => {
                io.step(Step::Drawn(vec![wide_polyline(&points, true)]));
                return;
            }
            Some(Answer::Keyword(k)) if k == UNDO => {
                if points.len() > 1 {
                    points.pop();
                }
            }
            Some(Answer::Point(next)) => points.push((next, start_width, end_width)),
            _ => {
                if points.len() >= 2 {
                    io.step(Step::Drawn(vec![wide_polyline(&points, false)]));
                }
                return;
            }
        }
    }
}

pub async fn rectangle_command(io: Io) {
    let Some(Answer::Point(first)) = io.ask(Prompt::point("Specify first corner point:")).await else { return };
    let prompt = Prompt::point("Specify other corner point:").base(first).preview(move |cursor| vec![rectangle_shape(first, cursor)]);
    if let Some(Answer::Point(other)) = io.ask(prompt).await {
        io.step(Step::Drawn(vec![rectangle_shape(first, other)]));
    }
}

pub async fn text_command(io: Io, defaults: std::rc::Rc<std::cell::RefCell<super::defaults::Defaults>>) {
    let style = defaults.borrow().text_style.clone();
    let Some(Answer::Point(position)) = io.ask(Prompt::point("Specify start point of text:")).await else { return };
    // A style with a height of its own does not ask for one.
    let height = if style.height > 0.0 {
        Some(Answer::Number(style.height))
    } else {
        io.ask(
            Prompt::new(PromptKind::Number, format!("Specify height <{DEFAULT_TEXT_HEIGHT}>:"))
                .base(position)
                .default(Answer::Number(DEFAULT_TEXT_HEIGHT)),
        )
        .await
    };
    let rotation = io
        .ask(
            Prompt::new(PromptKind::Angle, format!("Specify rotation angle of text <{}>:", super::format_general(DEFAULT_TEXT_ROTATION)))
                .base(position)
                .default(Answer::Number(DEFAULT_TEXT_ROTATION))
                .preview(line_preview(position)),
        )
        .await;
    let content = io.ask(Prompt::new(PromptKind::Text, "Enter text:")).await;
    if let (Some(Answer::Number(height)), Some(Answer::Number(rotation)), Some(Answer::Text(content))) = (height, rotation, content) {
        let mut entity = text_entity(position, &content, height, rotation);
        if let Shape::Text { font, style: name, width_factor, oblique, .. } = &mut entity.shape {
            // A style with no font of its own leaves the text in the interface font, as it reopens.
            if !style.font.is_empty() {
                (*font, *name) = (style.font, style.name);
            }
            (*width_factor, *oblique) = (style.width_factor, style.oblique);
        }
        io.step(Step::Drawn(vec![entity]));
    }
}
