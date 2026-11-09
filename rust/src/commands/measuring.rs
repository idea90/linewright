use super::draw::line_preview;
use super::format_general;
use super::prompts::{Answer, Io, Prompt, PromptKind, Step};
use crate::model::calc::evaluate;
use crate::model::entities::{Shape, polyline_from, vertex};
use crate::model::geometry::Point;
use crate::model::measure::{
    angle_at, angle_between_lines, angle_report, area_report, distance_report, included_angle, measure_entity, polygon, radius_of,
    radius_report,
};

const OBJECT: &str = "O";
const NO_AREA: &str = "That object has no area.";
const TOO_FEW_POINTS: &str = "An area needs at least three points.";

pub async fn distance(io: Io) {
    let Some(Answer::Point(first)) = io.ask(Prompt::point("Specify first point:")).await else { return };
    if let Some(Answer::Point(second)) = io.ask(Prompt::point("Specify second point:").base(first).preview(line_preview(first))).await {
        io.step(Step::Report(distance_report(first, second)));
    }
}

pub async fn area(io: Io) {
    let prompt = Prompt::point("Specify first corner point or [Object] <Object>:").options(&[OBJECT]).default(Answer::keyword(OBJECT));
    match io.ask(prompt).await {
        Some(Answer::Keyword(_)) => area_of_object(&io).await,
        Some(Answer::Point(first)) => area_of_points(&io, first).await,
        _ => {}
    }
}

async fn area_of_points(io: &Io, first: Point) {
    let mut points = vec![first];
    loop {
        let shown = points.clone();
        let prompt =
            Prompt::point("Specify next corner point or press ENTER for total:").base(points[points.len() - 1]).preview(move |cursor| {
                let mut corners = shown.clone();
                corners.push(cursor);
                vec![polyline_from(&corners, true)]
            });
        match io.ask(prompt).await {
            Some(Answer::Point(next)) => points.push(next),
            _ => break,
        }
    }
    if points.len() < 3 {
        io.step(Step::Report(TOO_FEW_POINTS.into()));
        return;
    }
    let vertices: Vec<_> = points.into_iter().map(vertex).collect();
    if let Some(measured) = polygon(&vertices) {
        io.step(Step::Report(area_report(measured)));
    }
}

async fn area_of_object(io: &Io) {
    let mut message = "Select objects:".to_string();
    loop {
        let Some(Answer::Picked(picked)) = io.ask(Prompt::new(PromptKind::Pick, message.clone())).await else { return };
        if let Some(measured) = measure_entity(&picked.entity) {
            io.step(Step::Report(area_report(measured)));
            return;
        }
        message = format!("{NO_AREA} Select objects:");
    }
}

const DISTANCE: &str = "D";
const RADIUS: &str = "R";
const ANGLE: &str = "A";
const AREA: &str = "AR";
const EXIT: &str = "X";
const VERTEX: &str = "V";

/// MEASUREGEOM: distance, radius, angle or area, one after another until eXit.
pub async fn measuregeom(io: Io) {
    let mut measured = false;
    loop {
        let prompt = if measured {
            Prompt::point("Enter an option [Distance/Radius/Angle/ARea/eXit] <eXit>:")
                .options(&[DISTANCE, RADIUS, ANGLE, AREA, EXIT])
                .default(Answer::keyword(EXIT))
        } else {
            Prompt::point("Enter an option [Distance/Radius/Angle/ARea] <Distance>:")
                .options(&[DISTANCE, RADIUS, ANGLE, AREA])
                .default(Answer::keyword(DISTANCE))
        };
        match io.ask(prompt).await {
            Some(Answer::Keyword(option)) => match option.as_str() {
                DISTANCE => distance(io.clone()).await,
                RADIUS => measure_radius(&io).await,
                ANGLE => measure_angle(&io).await,
                AREA => area(io.clone()).await,
                _ => return,
            },
            Some(_) => continue,
            None => return,
        }
        measured = true;
    }
}

async fn measure_radius(io: &Io) {
    let mut message = "Select arc or circle:";
    loop {
        let Some(Answer::Picked(picked)) = io.ask(Prompt::new(PromptKind::Pick, message)).await else { return };
        if let Some(radius) = radius_of(&picked.entity.shape) {
            for line in radius_report(radius).lines() {
                io.step(Step::Report(line.to_string()));
            }
            return;
        }
        message = "Object must be an arc or a circle. Select arc or circle:";
    }
}

async fn measure_angle(io: &Io) {
    let prompt = Prompt::new(PromptKind::Pick, "Select arc, circle, line, or <specify vertex>:").default(Answer::keyword(VERTEX));
    let angle = match io.ask(prompt).await {
        Some(Answer::Picked(picked)) => match picked.entity.shape {
            Shape::Arc { start_angle, end_angle, .. } => Some(included_angle(start_angle, end_angle)),
            Shape::Circle { center, .. } => match io.ask(Prompt::point("Specify second angle endpoint:").base(center)).await {
                Some(Answer::Point(second)) => Some(angle_at(center, picked.point, second)),
                _ => None,
            },
            Shape::Line { start, end } => loop {
                let Some(Answer::Picked(other)) = io.ask(Prompt::new(PromptKind::Pick, "Select second line:")).await else { break None };
                if let Shape::Line { start: other_start, end: other_end } = other.entity.shape {
                    break Some(angle_between_lines((start, end), picked.point, (other_start, other_end), other.point));
                }
                io.step(Step::Report("Object must be a line.".into()));
            },
            _ => {
                io.step(Step::Report("Object must be an arc, a circle or a line.".into()));
                None
            }
        },
        Some(_) => angle_from_points(io).await,
        None => None,
    };
    if let Some(angle) = angle {
        io.step(Step::Report(angle_report(angle)));
    }
}

/// The angle at a vertex between two endpoints.
async fn angle_from_points(io: &Io) -> Option<f64> {
    let Some(Answer::Point(vertex)) = io.ask(Prompt::point("Specify angle vertex:")).await else { return None };
    let prompt = Prompt::point("Specify first angle endpoint:").base(vertex).preview(line_preview(vertex));
    let Some(Answer::Point(first)) = io.ask(prompt).await else { return None };
    let prompt = Prompt::point("Specify second angle endpoint:").base(vertex).preview(line_preview(vertex));
    let Some(Answer::Point(second)) = io.ask(prompt).await else { return None };
    Some(angle_at(vertex, first, second))
}

/// CAL: evaluates an arithmetic expression on the command line.
pub async fn cal(io: Io) {
    let Some(Answer::Text(expression)) = io.ask(Prompt::new(PromptKind::Text, ">> Expression:")).await else { return };
    let message = match evaluate(&expression) {
        Ok(value) => format_general(value),
        Err(error) => error.message().to_string(),
    };
    io.step(Step::Report(message));
}
