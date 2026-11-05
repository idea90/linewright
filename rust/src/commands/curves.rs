use std::rc::Rc;

use super::draw::line_preview;
use super::prompts::{Answer, Io, Prompt, PromptKind, Step};
use crate::model::entities::{Entity, arc, circle, line};
use crate::model::geometry::{Point, ThreePointArc, arc_through_points, modulo};

const DIAMETER: &str = "D";
const THREE_POINTS: &str = "3P";
const TWO_POINTS: &str = "2P";
const CENTER: &str = "C";
const END: &str = "E";
const IN_A_LINE: &str = "The points are in a line.";
const NO_ARC: &str = "That does not make an arc.";

type ShapeOf = Rc<dyn Fn(ThreePointArc) -> Entity>;

fn circle_preview(center: Point) -> impl Fn(Point) -> Vec<Entity> {
    move |cursor| vec![circle(center, center.dist(cursor))]
}

fn diameter_circle(first: Point, second: Point) -> Entity {
    circle(first.midpoint(second), first.dist(second) / 2.0)
}

pub async fn circle_command(io: Io) {
    let prompt = Prompt::point("Specify center point for circle or [3P/2P]:").options(&[THREE_POINTS, TWO_POINTS]);
    match io.ask(prompt).await {
        Some(Answer::Keyword(k)) if k == THREE_POINTS => circle_through_three_points(&io).await,
        Some(Answer::Keyword(k)) if k == TWO_POINTS => circle_on_diameter(&io).await,
        Some(Answer::Point(center)) => circle_from_center(&io, center).await,
        _ => {}
    }
}

async fn circle_from_center(io: &Io, center: Point) {
    let prompt = Prompt::new(PromptKind::Number, "Specify radius of circle or [Diameter]:")
        .base(center)
        .options(&[DIAMETER])
        .preview(circle_preview(center));
    let radius = match io.ask(prompt).await {
        Some(Answer::Keyword(k)) if k == DIAMETER => {
            // A click answers with twice its distance, so the circle always passes through the click.
            let prompt = Prompt::new(PromptKind::Number, "Specify diameter of circle:")
                .base(center)
                .click_factor(2.0)
                .preview(circle_preview(center));
            match io.ask(prompt).await {
                Some(Answer::Number(diameter)) => diameter / 2.0,
                _ => return,
            }
        }
        Some(Answer::Number(radius)) => radius,
        _ => return,
    };
    io.step(Step::Drawn(vec![circle(center, radius)]));
}

async fn circle_on_diameter(io: &Io) {
    let Some(Answer::Point(first)) = io.ask(Prompt::point("Specify first end point of circle's diameter:")).await else { return };
    let mut message = "Specify second end point of circle's diameter:".to_string();
    loop {
        let prompt = Prompt::point(message.clone()).base(first).preview(move |cursor| vec![diameter_circle(first, cursor)]);
        let Some(Answer::Point(second)) = io.ask(prompt).await else { return };
        if second != first {
            io.step(Step::Drawn(vec![diameter_circle(first, second)]));
            return;
        }
        message = "The end points are the same. Specify second end point of circle's diameter:".into();
    }
}

fn three_point_preview(first: Point, second: Point, shape: ShapeOf) -> impl Fn(Point) -> Vec<Entity> {
    move |cursor| match arc_through_points(first, second, cursor) {
        None => vec![line(first, second), line(second, cursor)],
        Some(spec) => vec![shape(spec)],
    }
}

/// Ask for the third point until the three are not in a line, then draw the shape they define.
async fn last_of_three_points(io: &Io, first: Point, second: Point, label: &str, shape: ShapeOf) {
    let mut message = format!("Specify {label}:");
    loop {
        let prompt = Prompt::point(message.clone()).base(second).preview(three_point_preview(first, second, shape.clone()));
        let Some(Answer::Point(third)) = io.ask(prompt).await else { return };
        if let Some(spec) = arc_through_points(first, second, third) {
            io.step(Step::Drawn(vec![shape(spec)]));
            return;
        }
        message = format!("{IN_A_LINE} Specify {label}:");
    }
}

async fn circle_through_three_points(io: &Io) {
    let Some(Answer::Point(first)) = io.ask(Prompt::point("Specify first point on circle:")).await else { return };
    let prompt = Prompt::point("Specify second point on circle:").base(first).preview(line_preview(first));
    if let Some(Answer::Point(second)) = io.ask(prompt).await {
        last_of_three_points(io, first, second, "third point on circle", Rc::new(|spec: ThreePointArc| circle(spec.center, spec.radius)))
            .await;
    }
}

/// The counter-clockwise arc around the center from the start point toward the end point's direction.
fn arc_about_center(center: Point, start: Point, end: Point) -> Option<Entity> {
    let radius = center.dist(start);
    if radius == 0.0 || end == center {
        return None;
    }
    let start_angle = modulo(center.angle_to(start), 360.0);
    let end_angle = modulo(center.angle_to(end), 360.0);
    if (start_angle - end_angle).abs() <= 1e-9 {
        return None;
    }
    Some(arc(center, radius, start_angle, end_angle))
}

fn about_center_preview(center: Point, start: Point) -> impl Fn(Point) -> Vec<Entity> {
    move |cursor| vec![arc_about_center(center, start, cursor).unwrap_or_else(|| line(center, start))]
}

pub async fn arc_command(io: Io) {
    match io.ask(Prompt::point("Specify start point of arc or [Center]:").options(&[CENTER])).await {
        Some(Answer::Keyword(k)) if k == CENTER => arc_from_center(&io).await,
        Some(Answer::Point(start)) => arc_from_start(&io, start).await,
        _ => {}
    }
}

async fn arc_from_start(io: &Io, start: Point) {
    let prompt =
        Prompt::point("Specify second point of arc or [Center/End]:").base(start).options(&[CENTER, END]).preview(line_preview(start));
    match io.ask(prompt).await {
        Some(Answer::Keyword(k)) if k == CENTER => arc_start_center_end(io, start).await,
        Some(Answer::Keyword(k)) if k == END => arc_start_end_center(io, start).await,
        Some(Answer::Point(second)) => {
            let shape: ShapeOf = Rc::new(|spec: ThreePointArc| arc(spec.center, spec.radius, spec.start_angle, spec.end_angle));
            last_of_three_points(io, start, second, "end point of arc", shape).await;
        }
        _ => {}
    }
}

async fn arc_from_center(io: &Io) {
    let Some(Answer::Point(center)) = io.ask(Prompt::point("Specify center point of arc:")).await else { return };
    let mut message = "Specify start point of arc:".to_string();
    loop {
        let Some(Answer::Point(start)) = io.ask(Prompt::point(message.clone()).base(center).preview(line_preview(center))).await else {
            return;
        };
        if start != center {
            arc_end_about_center(io, center, start).await;
            return;
        }
        message = "The start point is the center. Specify start point of arc:".into();
    }
}

async fn arc_start_center_end(io: &Io, start: Point) {
    let mut message = "Specify center point of arc:".to_string();
    loop {
        let Some(Answer::Point(center)) = io.ask(Prompt::point(message.clone()).base(start).preview(line_preview(start))).await else {
            return;
        };
        if center != start {
            arc_end_about_center(io, center, start).await;
            return;
        }
        message = "The center is the start point. Specify center point of arc:".into();
    }
}

async fn arc_start_end_center(io: &Io, start: Point) {
    let Some(Answer::Point(end)) = io.ask(Prompt::point("Specify end point of arc:").base(start).preview(line_preview(start))).await else {
        return;
    };
    let mut message = "Specify center point of arc:".to_string();
    loop {
        let prompt = Prompt::point(message.clone()).base(end).preview(move |cursor| vec![line(cursor, start), line(cursor, end)]);
        let Some(Answer::Point(center)) = io.ask(prompt).await else { return };
        if let Some(found) = arc_about_center(center, start, end) {
            io.step(Step::Drawn(vec![found]));
            return;
        }
        message = format!("{NO_ARC} Specify center point of arc:");
    }
}

async fn arc_end_about_center(io: &Io, center: Point, start: Point) {
    let mut message = "Specify end point of arc:".to_string();
    loop {
        let prompt = Prompt::point(message.clone()).base(center).preview(about_center_preview(center, start));
        let Some(Answer::Point(end)) = io.ask(prompt).await else { return };
        if let Some(found) = arc_about_center(center, start, end) {
            io.step(Step::Drawn(vec![found]));
            return;
        }
        message = format!("{NO_ARC} Specify end point of arc:");
    }
}
