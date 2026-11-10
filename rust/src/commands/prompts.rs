//! How a command talks to the user and the drawing.
//!
//! A command is an `async fn`. It awaits `io.ask(prompt)` for each answer and hands changes to the drawing to
//! `io.step(step)`. A `Session` drives it: it polls the command until it asks the next question, performs the
//! steps it produced along the way, and holds the question until the user answers.

use std::cell::RefCell;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use super::parse::{parse_finite, parse_length, parse_number, parse_point};
use crate::model::boundary::{Region, region_in_drawing};
use crate::model::dimension::DimensionStyle;
use crate::model::document::{DrawOrder, Layer, ObjectGroup};
use crate::model::entities::Entity;
use crate::model::geometry::Point;
use crate::model::index::Bounds;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptKind {
    Point,
    /// Only accepts values greater than zero.
    Number,
    /// Accepts zero too.
    Length,
    /// Degrees.
    Angle,
    Text,
    /// Collects objects until Enter.
    Select,
    /// Takes one clicked object.
    Pick,
    /// The second corner of a crossing window from the prompt's base point: answered with both corners and every
    /// object the window touches.
    Window,
    /// The Delete Duplicate Objects dialog: the window shows it, and Enter takes the options it saved.
    OverkillDialog,
}

/// A rubber band for the cursor position.
pub type Preview = Rc<dyn Fn(Point) -> Vec<Entity>>;

/// A question a command asks.
#[derive(Clone)]
pub struct Prompt {
    pub kind: PromptKind,
    pub message: String,
    /// Relative coordinates and clicked distances or angles are measured from here.
    pub base: Option<Point>,
    /// Keywords accepted instead of a value.
    pub options: Vec<String>,
    /// What Enter answers on a prompt that has one.
    pub default: Option<Answer>,
    /// A click on a number prompt answers with distance * click_factor.
    pub click_factor: f64,
    pub preview: Option<Preview>,
    /// On a select prompt, Enter with nothing selected means every object.
    pub or_all: bool,
    /// On a point prompt, the point is answered with the region of the drawing closed off around it.
    pub region: bool,
    /// On a point prompt, the point is answered with the table it falls inside, as a pick.
    pub table: bool,
    /// On a point prompt, a number typed on its own is a length, answered as a number, rather than refused.
    pub or_length: bool,
}

impl fmt::Debug for Prompt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Prompt").field("kind", &self.kind).field("message", &self.message).field("base", &self.base).finish()
    }
}

impl Prompt {
    pub fn new(kind: PromptKind, message: impl Into<String>) -> Prompt {
        Prompt {
            kind,
            message: message.into(),
            base: None,
            options: vec![],
            default: None,
            click_factor: 1.0,
            preview: None,
            or_all: false,
            region: false,
            table: false,
            or_length: false,
        }
    }

    pub fn point(message: impl Into<String>) -> Prompt {
        Prompt::new(PromptKind::Point, message)
    }

    pub fn base(mut self, base: Point) -> Prompt {
        self.base = Some(base);
        self
    }

    pub fn options(mut self, options: &[&str]) -> Prompt {
        self.options = options.iter().map(|o| o.to_string()).collect();
        self
    }

    pub fn default(mut self, answer: Answer) -> Prompt {
        self.default = Some(answer);
        self
    }

    pub fn click_factor(mut self, factor: f64) -> Prompt {
        self.click_factor = factor;
        self
    }

    pub fn preview(mut self, preview: impl Fn(Point) -> Vec<Entity> + 'static) -> Prompt {
        self.preview = Some(Rc::new(preview));
        self
    }

    pub fn or_all(mut self) -> Prompt {
        self.or_all = true;
        self
    }

    pub fn table(mut self) -> Prompt {
        self.table = true;
        self
    }

    pub fn region(mut self) -> Prompt {
        self.region = true;
        self
    }

    pub fn or_length(mut self) -> Prompt {
        self.or_length = true;
        self
    }
}

/// An answer to a pick prompt: the object that was clicked, where, and whether Shift was held.
#[derive(Clone, Debug)]
pub struct Picked {
    pub entity: Entity,
    pub point: Point,
    pub shift: bool,
}

// An answer lives only while it is handed to the command, so a picked entity's size inside it does not matter.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug)]
pub enum Answer {
    Point(Point),
    Number(f64),
    Keyword(String),
    Text(String),
    Objects(Vec<Entity>),
    Picked(Picked),
    Window {
        first: Point,
        second: Point,
        objects: Vec<Entity>,
    },
    /// A point on a region prompt, with the loops closing it off (outer first, then islands); empty when the point
    /// is not closed in.
    Region {
        point: Point,
        loops: Vec<Vec<Point>>,
    },
}

impl Answer {
    pub fn keyword(word: &str) -> Answer {
        Answer::Keyword(word.to_string())
    }

    pub fn is(&self, word: &str) -> bool {
        matches!(self, Answer::Keyword(k) if k == word)
    }
}

/// A change a command makes, or something it tells the user.
#[derive(Clone, Debug)]
pub enum Step {
    /// Draw these brand-new entities. They take the current layer, color and linetype.
    Drawn(Vec<Entity>),
    /// Add entities made from existing ones, which keep the layer and look they were given.
    Commit(Vec<Entity>),
    /// Take back the entities this command committed last.
    Retract,
    /// Delete these entities from the drawing.
    Remove(Vec<Entity>),
    /// Change entities in place: each is the new state of the existing entity with the same id.
    Modify(Vec<Entity>),
    /// Define or redefine a dimension style, carrying a change over to the dimensions that follow it, and make a
    /// style current.
    DimensionStyle { style: Option<DimensionStyle>, current: Option<String> },
    /// Put objects in front of or behind others.
    DrawOrder(Vec<Entity>, DrawOrder),
    /// The drawing's layers as they should be now, and the current layer if it changes, as one undo step.
    Layers { layers: Vec<Layer>, current: Option<String> },
    /// Replace the drawing's groups, as one undo step.
    Groups(Vec<ObjectGroup>),
    /// Add, rename, delete or open a layout.
    Layout(LayoutRequest),
    /// Several steps that undo together as one.
    Compound(Vec<Step>),
    /// Tell the user something on the command line, such as a measurement.
    Report(String),
    /// Change what the drawing area shows.
    View(ViewRequest),
    /// Set whether wipeout frames show and plot: 0 off, 1 on and plotted, 2 shown but not plotted.
    WipeoutFrame(u8),
    /// Set one of the drawing's linetype scales.
    LinetypeScale(ScaleSetting),
}

/// A change to what the drawing area shows, as ZOOM asks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ViewRequest {
    Extents,
    Window(Point, Point),
    Previous,
    /// Zoom by a factor about the middle of the view.
    Scale(f64),
}

/// What a running command may do to the drawing. Each call is one undo step, or all of a group.
pub trait Host {
    fn add(&mut self, entities: Vec<Entity>);
    fn add_new(&mut self, entities: Vec<Entity>);
    fn remove(&mut self, entities: Vec<Entity>);
    fn modify(&mut self, states: Vec<Entity>);
    fn retract(&mut self);
    fn begin_group(&mut self);
    fn end_group(&mut self);
    fn report(&mut self, message: String);
    /// Change what the drawing area shows. A host without one ignores it.
    fn view(&mut self, _request: ViewRequest) {}
    /// The visible objects in an area, and how far the drawing reaches, for finding regions.
    fn visible_in(&self, _area: &Bounds) -> Vec<Entity> {
        vec![]
    }
    fn extent(&self) -> Option<Bounds> {
        None
    }
    fn dimension_style(&mut self, _style: Option<DimensionStyle>, _current: Option<String>) {}
    fn groups(&mut self, _groups: Vec<ObjectGroup>) {}
    fn layers(&mut self, _layers: Vec<Layer>, _current: Option<String>) {}
    fn draw_order(&mut self, _objects: Vec<Entity>, _placement: DrawOrder) {}
    /// A layout change, for the window to make once the command has had its say.
    fn layout(&mut self, _request: LayoutRequest) {}
    /// Set whether wipeout frames show and plot, as WIPEOUTFRAME does.
    fn wipeout_frame(&mut self, _mode: u8) {}
    /// Set one of the drawing's linetype scales: LTSCALE, PSLTSCALE or CELTSCALE.
    fn linetype_scale(&mut self, _setting: ScaleSetting) {}
}

/// One of the drawing's linetype scales, as LTSCALE, PSLTSCALE and CELTSCALE set it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScaleSetting {
    /// LTSCALE: the scale every linetype's dashes are drawn at.
    Global(f64),
    /// PSLTSCALE: whether model space objects in viewports keep their dashes as long on the sheet as paper space ones.
    Paper(bool),
    /// CELTSCALE: the linetype scale new objects get.
    Current(f64),
}

/// What LAYOUT asks of the window. Layouts go by index; None is model space.
#[derive(Clone, Debug, PartialEq)]
pub enum LayoutRequest {
    New(String),
    Rename(usize, String),
    Delete(usize),
    Open(Option<usize>),
}

#[derive(Default)]
struct Exchange {
    prompt: Option<Prompt>,
    answer: Option<Option<Answer>>,
    steps: Vec<Step>,
}

/// The command's side of the conversation.
#[derive(Clone, Default)]
pub struct Io(Rc<RefCell<Exchange>>);

impl Io {
    /// Ask the user. None means Enter on a prompt without a default, or Esc.
    pub fn ask(&self, prompt: Prompt) -> Ask {
        Ask { io: self.clone(), prompt: Some(prompt) }
    }

    pub fn step(&self, step: Step) {
        self.0.borrow_mut().steps.push(step);
    }
}

pub struct Ask {
    io: Io,
    prompt: Option<Prompt>,
}

impl Future for Ask {
    type Output = Option<Answer>;

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Answer>> {
        let this = &mut *self;
        let mut exchange = this.io.0.borrow_mut();
        if let Some(prompt) = this.prompt.take() {
            exchange.prompt = Some(prompt);
            exchange.answer = None;
            return Poll::Pending;
        }
        match exchange.answer.take() {
            Some(answer) => Poll::Ready(answer),
            None => Poll::Pending,
        }
    }
}

pub type Command = Pin<Box<dyn Future<Output = ()>>>;

pub const SELECT_HINT: &str = "Select objects by clicking or dragging, then press Enter.";

fn invalid_answer(kind: PromptKind) -> &'static str {
    match kind {
        PromptKind::Point => "Enter a point as x,y or @dx,dy or @distance<angle.",
        PromptKind::Number => "Enter a number greater than zero.",
        PromptKind::Length => "Enter a number, zero or greater.",
        PromptKind::Angle => "Enter an angle in degrees.",
        PromptKind::Window => "Specify the other corner of the window as x,y.",
        PromptKind::OverkillDialog => "Press Enter to accept the options, or Esc to cancel.",
        PromptKind::Text | PromptKind::Select | PromptKind::Pick => SELECT_HINT,
    }
}

/// Runs one command and hands the user's answers to it.
pub struct Session {
    command: Option<Command>,
    io: Io,
    pub prompt: Option<Prompt>,
}

impl Session {
    /// Start the command: it runs until its first question.
    pub fn start(build: impl FnOnce(Io) -> Command, host: &mut dyn Host) -> Session {
        let io = Io::default();
        let mut session = Session { command: Some(build(io.clone())), io, prompt: None };
        session.run(host);
        session
    }

    pub fn active(&self) -> bool {
        self.prompt.is_some()
    }

    /// Answer the current prompt with typed text. Returns an error message if it was not valid.
    pub fn submit_text(&mut self, text: &str, host: &mut dyn Host) -> Option<&'static str> {
        let prompt = self.prompt.as_ref()?;
        let text = text.trim();
        let answer = if text.is_empty() {
            prompt.default.clone()
        } else {
            match Self::parse(prompt, text) {
                None => return Some(invalid_answer(prompt.kind)),
                answer => answer,
            }
        };
        self.advance(answer, host);
        None
    }

    /// Answer with a click: a point, a distance from the base point, or an angle around it.
    pub fn submit_point(&mut self, point: Point, host: &mut dyn Host) {
        let Some(prompt) = &self.prompt else { return };
        match prompt.kind {
            PromptKind::Text | PromptKind::Select | PromptKind::Pick | PromptKind::Window => {}
            PromptKind::Point => self.advance(Some(Answer::Point(point)), host),
            _ => {
                if let Some(base) = prompt.base
                    && point != base
                {
                    let measured =
                        if prompt.kind == PromptKind::Angle { base.angle_to(point) } else { base.dist(point) * prompt.click_factor };
                    self.advance(Some(Answer::Number(measured)), host);
                }
            }
        }
    }

    /// Answer a window prompt with its second corner and the objects the window touches.
    pub fn submit_window(&mut self, second: Point, objects: Vec<Entity>, host: &mut dyn Host) {
        let Some(prompt) = self.prompt.as_ref().filter(|p| p.kind == PromptKind::Window) else { return };
        let first = prompt.base.unwrap_or(second);
        self.advance(Some(Answer::Window { first, second, objects }), host);
    }

    /// Finish a select prompt with the chosen objects.
    pub fn submit_objects(&mut self, objects: Vec<Entity>, host: &mut dyn Host) {
        if self.prompt.as_ref().is_some_and(|p| p.kind == PromptKind::Select) {
            self.advance(Some(Answer::Objects(objects)), host);
        }
    }

    /// Answer a pick prompt with the clicked object and where it was clicked.
    pub fn submit_pick(&mut self, entity: Entity, point: Point, shift: bool, host: &mut dyn Host) {
        if self.prompt.as_ref().is_some_and(|p| p.kind == PromptKind::Pick) {
            self.advance(Some(Answer::Picked(Picked { entity, point, shift })), host);
        }
    }

    /// Esc. A command that can finish with what it has does so, any other stops.
    pub fn cancel(&mut self, host: &mut dyn Host) {
        if self.prompt.is_none() {
            return;
        }
        self.advance(None, host);
        if self.prompt.is_some() {
            self.command = None;
            self.prompt = None;
        }
    }

    fn parse(prompt: &Prompt, text: &str) -> Option<Answer> {
        let upper = text.to_uppercase();
        if prompt.options.contains(&upper) {
            return Some(Answer::Keyword(upper));
        }
        match prompt.kind {
            PromptKind::Point if prompt.or_length && parse_number(text).is_some() => parse_number(text).map(Answer::Number),
            PromptKind::Point => parse_point(text, prompt.base).map(Answer::Point),
            PromptKind::Number => parse_number(text).map(Answer::Number),
            PromptKind::Length => parse_length(text).map(Answer::Number),
            PromptKind::Angle => parse_finite(text).map(Answer::Number),
            PromptKind::Text => Some(Answer::Text(text.to_string())),
            PromptKind::Select | PromptKind::Pick | PromptKind::Window | PromptKind::OverkillDialog => None,
        }
    }

    fn advance(&mut self, answer: Option<Answer>, host: &mut dyn Host) {
        let answer = match answer {
            Some(Answer::Point(point)) if self.prompt.as_ref().is_some_and(|p| p.table) => {
                let near = host.visible_in(&Bounds::square_around(point, 0.0));
                let table = near.into_iter().find(|e| crate::model::table::cell_at(&e.shape, point).is_some());
                table.map(|entity| Answer::Picked(Picked { entity, point, shift: false }))
            }
            Some(Answer::Point(point)) if self.prompt.as_ref().is_some_and(|p| p.region) => {
                let region = host.extent().and_then(|extent| region_in_drawing(point, extent, |area| host.visible_in(area)));
                Some(Answer::Region { point, loops: region.map(Region::loops).unwrap_or_default() })
            }
            answer => answer,
        };
        self.io.0.borrow_mut().answer = Some(answer);
        self.run(host);
    }

    fn run(&mut self, host: &mut dyn Host) {
        let Some(command) = self.command.as_mut() else {
            self.prompt = None;
            return;
        };
        let finished = command.as_mut().poll(&mut Context::from_waker(Waker::noop())).is_ready();
        let steps = std::mem::take(&mut self.io.0.borrow_mut().steps);
        for step in steps {
            perform(step, host);
        }
        if finished {
            self.command = None;
            self.prompt = None;
        } else {
            self.prompt = self.io.0.borrow_mut().prompt.take();
        }
    }
}

fn perform(step: Step, host: &mut dyn Host) {
    match step {
        Step::Compound(steps) => {
            host.begin_group();
            for inner in steps {
                perform(inner, host);
            }
            host.end_group();
        }
        Step::Drawn(entities) => host.add_new(entities),
        Step::Commit(entities) => host.add(entities),
        Step::Remove(entities) => host.remove(entities),
        Step::Modify(states) => host.modify(states),
        Step::Report(message) => host.report(message),
        Step::View(request) => host.view(request),
        Step::Retract => host.retract(),
        Step::DimensionStyle { style, current } => host.dimension_style(style, current),
        Step::Layout(request) => host.layout(request),
        Step::Groups(groups) => host.groups(groups),
        Step::Layers { layers, current } => host.layers(layers, current),
        Step::DrawOrder(objects, placement) => host.draw_order(objects, placement),
        Step::WipeoutFrame(mode) => host.wipeout_frame(mode),
        Step::LinetypeScale(setting) => host.linetype_scale(setting),
    }
}
