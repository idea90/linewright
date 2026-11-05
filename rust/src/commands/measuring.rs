use super::draw::line_preview;
use super::prompts::{Answer, Io, Prompt, PromptKind, Step};
use crate::model::entities::{polyline_from, vertex};
use crate::model::geometry::Point;
use crate::model::measure::{area_report, distance_report, measure_entity, polygon};

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
