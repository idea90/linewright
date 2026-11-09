//! `linewright --mcp`: a Model Context Protocol server on standard input and output, so an AI agent such as Claude
//! Code can open, draw on, look at and save drawings.
//!
//! The agent works the drawing as a person does at the command line: it types commands and answers to their
//! prompts, which go through the same `Controller` as the window's command line. It sees the drawing as a PNG
//! drawn the way a plot prints it.

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::canvas::raster;
use crate::commands::prompts::PromptKind;
use crate::commands::registry::{ALIASES, COMMANDS, WINDOW_COMMANDS};
use crate::io::files::{open_drawing, save_drawing};
use crate::model::document::{Document, Layer};
use crate::model::entities::{Entity, EntityId, Shape};
use crate::model::geometry::{Point, pt};
use crate::model::index::{Bounds, entity_extent};
use crate::ui::controller::Controller;
use crate::ui::undo::{Change, LayerState};

const PROTOCOL_VERSION: &str = "2025-06-18";
/// How many pixels the drawing's extent spans for picking: picks and boxes reach about 0.8% of it.
const PICK_PIXELS_ACROSS: f64 = 1000.0;
const DEFAULT_LIST_LIMIT: usize = 200;

const INSTRUCTIONS: &str = "Linewright is a 2D CAD program that works like AutoCAD. One drawing is open at a time \
(a new empty one at first). Draw and edit with the `command` tool: give it what you would type at AutoCAD's \
command line, one entry per item, such as [\"LINE\", \"0,0\", \"100,0\", \"@0,50\", \"\"]. An empty string is \
Enter. Points are x,y, @dx,dy or @distance<angle. Options are their capitalised letters. A command left waiting \
for an answer stays running until the next call. Look at the result with `render`, read objects with \
`list_entities` (or select them and run LIST), and write the file with `save_drawing`.";

/// The server's state: the drawing and the command running on it.
pub struct Server {
    controller: Controller,
}

impl Default for Server {
    fn default() -> Self {
        Server::new()
    }
}

/// What a tool hands back: text and pictures, or an error message.
struct Reply {
    content: Vec<Value>,
    error: bool,
}

impl Reply {
    fn text(text: impl Into<String>) -> Reply {
        Reply { content: vec![json!({"type": "text", "text": text.into()})], error: false }
    }

    fn error(text: impl Into<String>) -> Reply {
        Reply { error: true, ..Reply::text(text) }
    }
}

impl Server {
    pub fn new() -> Server {
        let mut controller = Controller::new();
        controller.set_document(Document::new());
        Server { controller }
    }

    /// Answer one JSON-RPC message. Notifications get no answer.
    pub fn handle(&mut self, message: &Value) -> Option<Value> {
        let id = message.get("id").cloned()?;
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let result = match method {
            "initialize" => Ok(json!({
                "protocolVersion": params.get("protocolVersion").and_then(Value::as_str).unwrap_or(PROTOCOL_VERSION),
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "linewright", "version": env!("CARGO_PKG_VERSION")},
                "instructions": INSTRUCTIONS,
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": tools()})),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
                match self.call(name, &arguments) {
                    Some(reply) => Ok(json!({"content": reply.content, "isError": reply.error})),
                    None => Err((-32602, format!("Unknown tool: {name}"))),
                }
            }
            _ => Err((-32601, format!("Unknown method: {method}"))),
        };
        Some(match result {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err((code, message)) => json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}),
        })
    }

    fn call(&mut self, name: &str, arguments: &Value) -> Option<Reply> {
        Some(match name {
            "open_drawing" => self.open(arguments),
            "new_drawing" => {
                self.controller.set_document(Document::new());
                Reply::text("A new, empty drawing is open.")
            }
            "save_drawing" => self.save(arguments),
            "drawing_info" => Reply::text(self.info()),
            "list_entities" => self.list(arguments),
            "command" => self.command(arguments),
            "render" => self.render(arguments),
            "layer" => self.layer(arguments),
            "set_property" => self.set_property(arguments),
            "list_commands" => Reply::text(command_list(arguments.get("filter").and_then(Value::as_str).unwrap_or(""))),
            _ => return None,
        })
    }

    fn open(&mut self, arguments: &Value) -> Reply {
        let Some(path) = arguments.get("path").and_then(Value::as_str) else { return Reply::error("Give the path of a DXF or DWG file.") };
        match open_drawing(Path::new(path)) {
            Ok(document) => {
                self.controller.set_document(document);
                Reply::text(format!("Opened {path}.\n\n{}", self.info()))
            }
            Err(error) => Reply::error(error.to_string()),
        }
    }

    fn save(&mut self, arguments: &Value) -> Reply {
        let given = arguments.get("path").and_then(Value::as_str).map(PathBuf::from);
        let Some(path) = given.or_else(|| self.controller.document().path.clone()) else {
            return Reply::error("The drawing has never been saved: give a path ending in .dxf or .dwg.");
        };
        match save_drawing(&mut self.controller.drawing.document, &path) {
            Ok(()) => {
                self.controller.drawing.document.path = Some(path.clone());
                self.controller.drawing.set_clean();
                Reply::text(format!("Saved {}.", path.display()))
            }
            Err(error) => Reply::error(error.to_string()),
        }
    }

    /// The drawing at a glance: its file, units, extent, layers and what kinds of objects it holds.
    fn info(&self) -> String {
        let document = self.controller.document();
        let mut out = String::new();
        let file = document.path.as_ref().map_or("(not saved yet)".to_string(), |p| p.display().to_string());
        out.push_str(&format!("File: {file}{}\n", if self.controller.drawing.is_clean() { "" } else { " (changed since saved)" }));
        out.push_str(&format!("Units: {}\n", units_name(crate::io::dxf::mm_per_unit(document))));
        let space = match document.shown_layout() {
            Some(index) => format!("layout {:?}", document.layouts[index].name),
            None => "model space".into(),
        };
        out.push_str(&format!("Open space: {space}\n"));
        match document.extent() {
            Some(b) => out.push_str(&format!("Extent: {} to {}\n", coords(pt(b.x0, b.y0)), coords(pt(b.x1, b.y1)))),
            None => out.push_str("Extent: none, the space is empty\n"),
        }
        let mut kinds: HashMap<&str, usize> = HashMap::new();
        let mut per_layer: HashMap<String, usize> = HashMap::new();
        for entity in document.entities() {
            *kinds.entry(entity.shape.kind()).or_default() += 1;
            *per_layer.entry(entity.layer.to_uppercase()).or_default() += 1;
        }
        let mut kinds: Vec<_> = kinds.into_iter().collect();
        kinds.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        let kinds: Vec<String> = kinds.iter().map(|(kind, count)| format!("{count} {kind}")).collect();
        out.push_str(&format!("Objects: {} ({})\n", document.len(), if kinds.is_empty() { "none".into() } else { kinds.join(", ") }));
        out.push_str(&format!("Current layer: {}\nLayers:\n", document.current_layer));
        for layer in &document.layers {
            let mut states = vec![];
            if !layer.visible {
                states.push("off");
            }
            if layer.frozen {
                states.push("frozen");
            }
            if layer.locked {
                states.push("locked");
            }
            if !layer.plot {
                states.push("no plot");
            }
            let count = per_layer.get(&layer.name.to_uppercase()).copied().unwrap_or(0);
            let states = if states.is_empty() { String::new() } else { format!(", {}", states.join(", ")) };
            out.push_str(&format!("  {} (color {}, {}{states}): {count} objects\n", layer.name, layer.color, layer.linetype));
        }
        if !document.layouts.is_empty() {
            let names: Vec<&str> = document.layouts.iter().map(|l| l.name.as_str()).collect();
            out.push_str(&format!("Layouts: {}\n", names.join(", ")));
        }
        let blocks: Vec<&str> = document.blocks.values().map(|b| b.0.name.as_str()).filter(|n| !n.starts_with('*')).collect();
        if !blocks.is_empty() {
            out.push_str(&format!("Blocks: {}\n", blocks.join(", ")));
        }
        out.push_str(&self.status());
        out
    }

    fn list(&self, arguments: &Value) -> Reply {
        let document = self.controller.document();
        let layer = arguments.get("layer").and_then(Value::as_str);
        let kind = arguments.get("kind").and_then(Value::as_str);
        let window = match arguments.get("window").map(bounds_of) {
            Some(None) => return Reply::error("window is [x0, y0, x1, y1]."),
            Some(window) => window,
            None => None,
        };
        let limit = arguments.get("limit").and_then(Value::as_u64).map_or(DEFAULT_LIST_LIMIT, |l| l as usize);
        let found: Vec<&Entity> = document
            .entities()
            .iter()
            .filter(|e| layer.is_none_or(|l| e.layer.eq_ignore_ascii_case(l)))
            .filter(|e| kind.is_none_or(|k| e.shape.kind().eq_ignore_ascii_case(k)))
            .filter(|e| window.is_none_or(|w| entity_extent(e).touches(&w)))
            .collect();
        let mut out: Vec<String> = found.iter().take(limit).map(|e| describe(e)).collect();
        if found.len() > limit {
            out.push(format!("… and {} more (raise limit, or narrow by layer, kind or window)", found.len() - limit));
        }
        if found.is_empty() {
            out.push("No objects match.".into());
        }
        Reply::text(out.join("\n"))
    }

    /// Type entries at the command line, one by one, and tell what came of them.
    fn command(&mut self, arguments: &Value) -> Reply {
        let entries: Vec<String> = match arguments.get("input") {
            Some(Value::String(one)) => vec![one.clone()],
            Some(Value::Array(many)) => many.iter().map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_string)).collect(),
            _ => return Reply::error("input is a list of entries, such as [\"LINE\", \"0,0\", \"10,0\", \"\"]."),
        };
        if let Some(ids) = arguments.get("select").and_then(Value::as_array) {
            let ids: Vec<EntityId> = ids.iter().filter_map(Value::as_u64).collect();
            self.select(&ids, true);
        }
        let before: HashMap<EntityId, Entity> = self.controller.document().entities().iter().map(|e| (e.id, e.clone())).collect();
        self.controller.drawing.log.clear();
        self.fit_pick_aperture();
        let mut problems = vec![];
        for entry in &entries {
            if let Err(problem) = self.enter(entry) {
                problems.push(problem);
            }
            self.controller.apply_layout_request();
        }
        let mut out: Vec<String> = std::mem::take(&mut self.controller.drawing.log);
        out.extend(problems);
        out.push(String::new());
        out.push(self.changes(&before));
        out.push(self.status());
        Reply::text(out.join("\n").trim_start().to_string())
    }

    /// One entry: a click or box written out, a selection by id, Esc, or text typed at the command line.
    fn enter(&mut self, entry: &str) -> Result<(), String> {
        let trimmed = entry.trim();
        let (word, rest) = trimmed.split_once(char::is_whitespace).unwrap_or((trimmed, ""));
        let rest = rest.trim();
        match word.to_uppercase().as_str() {
            "ESC" | "ESCAPE" if rest.is_empty() => self.controller.cancel(),
            "CLICK" => {
                let point = parse_point(rest).ok_or_else(|| format!("click wants a point x,y, not {rest:?}"))?;
                // A point prompt writes its click to the command line itself; a pick or selection does not.
                if let Some(prompt) = self.controller.prompt().filter(|p| matches!(p.kind, PromptKind::Pick | PromptKind::Select)) {
                    let line = format!("{} click {}", prompt.message, coords(point));
                    self.controller.drawing.print(line);
                }
                self.controller.move_cursor(Some(point));
                self.controller.click(point);
                self.controller.release(point, false);
            }
            "BOX" => {
                let corners: Vec<Point> = rest.split_whitespace().filter_map(parse_point).collect();
                let [first, second] = corners[..] else { return Err(format!("box wants two corners x1,y1 x2,y2, not {rest:?}")) };
                self.controller.move_cursor(Some(first));
                self.controller.click(first);
                self.controller.move_cursor(Some(second));
                self.controller.release(second, false);
            }
            // "select" with ids picks those objects; SELECT on its own, or with anything else, is AutoCAD's command.
            "SELECT" if !rest.is_empty() && rest.split_whitespace().all(|w| w.parse::<u64>().is_ok()) => {
                let ids: Vec<EntityId> = rest.split_whitespace().filter_map(|w| w.parse().ok()).collect();
                self.select(&ids, false);
            }
            _ if self.controller.running_command().is_none() && crate::ui::app::file_command(trimmed).is_some() => {
                return Err(window_only(trimmed));
            }
            _ => self.controller.handle_input(entry),
        }
        Ok(())
    }

    /// Select objects by id, as clicking them would, adding to the selection unless `replace`.
    fn select(&mut self, ids: &[EntityId], replace: bool) {
        let document = self.controller.document();
        let (found, missing): (Vec<EntityId>, Vec<EntityId>) =
            ids.iter().partition(|id| document.get(**id).is_some_and(|e| document.is_editable(e)));
        if replace && self.controller.running_command().is_none() {
            self.controller.selection.clear();
        }
        let count = found.len();
        self.controller.selection.add(found);
        let total = self.controller.selection.len();
        self.controller.drawing.print(format!("{count} found, {total} total"));
        if !missing.is_empty() {
            let missing: Vec<String> = missing.iter().map(u64::to_string).collect();
            self.controller.drawing.print(format!("Not in the open space, or on a locked layer: {}", missing.join(" ")));
        }
    }

    /// Picks reach a few pixels: make them a small share of the drawing, as on a screen showing all of it.
    fn fit_pick_aperture(&mut self) {
        if let Some(extent) = self.controller.document().extent() {
            let size = extent.width().max(extent.height());
            if size > 0.0 {
                self.controller.scale = PICK_PIXELS_ACROSS / size;
            }
        }
    }

    /// Which objects a command added, changed and erased.
    fn changes(&self, before: &HashMap<EntityId, Entity>) -> String {
        let document = self.controller.document();
        let mut added = vec![];
        let mut changed = vec![];
        for entity in document.entities() {
            match before.get(&entity.id) {
                None => added.push(entity.id),
                Some(old) if !old.same_state(entity) => changed.push(entity.id),
                Some(_) => {}
            }
        }
        let erased: Vec<EntityId> = before.keys().filter(|id| !document.contains(**id)).copied().collect();
        let ids = |ids: &[EntityId]| ids.iter().map(u64::to_string).collect::<Vec<_>>().join(" ");
        let mut out = vec![];
        if !added.is_empty() {
            out.push(format!("Added: {}", ids(&added)));
        }
        if !changed.is_empty() {
            out.push(format!("Changed: {}", ids(&changed)));
        }
        if !erased.is_empty() {
            let mut erased = erased;
            erased.sort();
            out.push(format!("Erased: {}", ids(&erased)));
        }
        if out.is_empty() {
            out.push("No objects changed.".into());
        }
        out.join("\n")
    }

    /// What the command line waits for now, and what is selected.
    fn status(&self) -> String {
        let selected = self.controller.selection.len();
        let selection = if selected > 0 { format!(" {selected} object(s) selected.") } else { String::new() };
        match (self.controller.running_command(), self.controller.prompt()) {
            (Some(name), Some(prompt)) => {
                let wants = match prompt.kind {
                    PromptKind::Point => "a point",
                    PromptKind::Number => "a number above zero, or a point to measure from the last one",
                    PromptKind::Length => "a length",
                    PromptKind::Angle => "an angle in degrees",
                    PromptKind::Text => "text",
                    PromptKind::Select => "objects: \"select <ids>\", \"click x,y\", \"box x1,y1 x2,y2\" or ALL, then \"\"",
                    PromptKind::Pick => "one object: \"click x,y\" on it",
                    PromptKind::Window => "the other corner x,y",
                    PromptKind::OverkillDialog => "\"\" to accept the options",
                };
                let default = match &prompt.default {
                    Some(answer) => format!(" Enter (\"\") gives {}.", answer_text(answer)),
                    None => String::new(),
                };
                format!("{name} is waiting for {wants}: {}{default}{selection}", prompt.message)
            }
            _ => format!("Command: (no command running){selection}"),
        }
    }

    /// Make a layer, or change one, as the Layer palette does: one undo step.
    fn layer(&mut self, arguments: &Value) -> Reply {
        let Some(name) = arguments.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()) else {
            return Reply::error("Give the layer's name.");
        };
        let document = self.controller.document();
        let mut layers = document.layers.clone();
        let at = match layers.iter().position(|l| l.name.eq_ignore_ascii_case(name)) {
            Some(at) => at,
            None => {
                layers.push(Layer::new(name));
                layers.len() - 1
            }
        };
        let made = at == document.layers.len();
        let layer = &mut layers[at];
        if let Some(color) = arguments.get("color").and_then(Value::as_i64) {
            if !(1..=255).contains(&color) {
                return Reply::error("A layer's color is an AutoCAD color index from 1 to 255.");
            }
            layer.color = color as i32;
        }
        if let Some(linetype) = arguments.get("linetype").and_then(Value::as_str) {
            layer.linetype = crate::model::styles::canonical_linetype(linetype);
        }
        let flag = |key: &str| arguments.get(key).and_then(Value::as_bool);
        if let Some(on) = flag("on") {
            layer.visible = on;
        }
        if let Some(locked) = flag("locked") {
            layer.locked = locked;
        }
        if let Some(plot) = flag("plot") {
            layer.plot = plot;
        }
        let current = flag("current").unwrap_or(false);
        let is_current = current || document.current_layer.eq_ignore_ascii_case(name);
        if let Some(frozen) = flag("frozen") {
            if frozen && is_current {
                return Reply::error("The current layer cannot be frozen.");
            }
            layer.frozen = frozen;
        }
        let layer_name = layer.name.clone();
        let before = LayerState { layers: document.layers.clone(), current: document.current_layer.clone(), entity_layers: vec![] };
        let after =
            LayerState { layers, current: if current { layer_name.clone() } else { before.current.clone() }, entity_layers: vec![] };
        self.controller.push(Change::Layers { before, after });
        let what = if made { "Made" } else { "Changed" };
        let current = if current { ", and it is current" } else { "" };
        Reply::text(format!("{what} layer {layer_name}{current}."))
    }

    /// Set one property of some objects, as typing in the Properties palette does: one undo step.
    fn set_property(&mut self, arguments: &Value) -> Reply {
        let ids: Vec<EntityId> =
            arguments.get("ids").and_then(Value::as_array).map_or(vec![], |ids| ids.iter().filter_map(Value::as_u64).collect());
        let (Some(name), Some(value)) = (arguments.get("name").and_then(Value::as_str), arguments.get("value")) else {
            return Reply::error("Give ids, the property's name and its value.");
        };
        let value = value.as_str().map_or_else(|| value.to_string(), str::to_string);
        let document = self.controller.document();
        let mut states = vec![];
        for id in &ids {
            let Some(entity) = document.get(*id) else { return Reply::error(format!("No object {id} in the open space.")) };
            let state = match name.to_lowercase().as_str() {
                "layer" => match document.layers.iter().find(|l| l.name.eq_ignore_ascii_case(&value)) {
                    Some(layer) => Entity { layer: layer.name.clone(), ..entity.clone() },
                    None => return Reply::error(format!("There is no layer {value}: make it with the layer tool first.")),
                },
                "color" => match parse_color(&value) {
                    Some(color) => Entity { color, ..entity.clone() },
                    None => return Reply::error("A color is ByLayer, ByBlock or an AutoCAD color index from 1 to 255."),
                },
                "linetype" => Entity { linetype: crate::model::styles::canonical_linetype(&value), ..entity.clone() },
                _ => match crate::model::properties::set_property(entity, name, &value) {
                    Ok(state) => state,
                    Err(message) => return Reply::error(format!("{id}: {message}")),
                },
            };
            if !state.same_state(entity) {
                states.push(state);
            }
        }
        if ids.is_empty() {
            return Reply::error("Give the ids of the objects to change.");
        }
        let count = states.len();
        if count > 0 {
            let change = Change::modify(document, states);
            self.controller.push(change);
        }
        Reply::text(format!("{name} set on {count} object(s)."))
    }

    fn render(&mut self, arguments: &Value) -> Reply {
        let side = |key: &str, default: u32| arguments.get(key).and_then(Value::as_u64).map_or(default, |v| v as u32);
        let (width, height) = (side("width", 1200), side("height", 900));
        let window = match arguments.get("window").map(bounds_of) {
            Some(None) => return Reply::error("window is [x0, y0, x1, y1]."),
            Some(window) => window,
            None => None,
        };
        let (png, view) = match raster::picture(self.controller.document(), width, height, window) {
            Ok(picture) => picture,
            Err(error) => return Reply::error(error),
        };
        let area = view.area;
        let mut text = format!(
            "Shows {} to {} at {} pixels a unit; the picture's top left is {}. Dark lines on white, as plotted.",
            coords(pt(area.x0, area.y0)),
            coords(pt(area.x1, area.y1)),
            short(view.pixels_per_unit),
            coords(pt(area.x0, area.y1)),
        );
        if let Some(path) = arguments.get("save_to").and_then(Value::as_str) {
            match std::fs::write(path, &png) {
                Ok(()) => text.push_str(&format!(" Saved to {path}.")),
                Err(error) => return Reply::error(format!("Could not write {path}: {error}")),
            }
        }
        Reply {
            content: vec![json!({"type": "image", "data": base64(&png), "mimeType": "image/png"}), json!({"type": "text", "text": text})],
            error: false,
        }
    }
}

/// Serve on standard input and output, one JSON message a line, until the input ends.
pub fn serve() -> std::io::Result<()> {
    let mut server = Server::new();
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let answer = match serde_json::from_str::<Value>(&line) {
            Ok(message) => server.handle(&message),
            Err(error) => Some(json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": error.to_string()}})),
        };
        if let Some(answer) = answer {
            writeln!(stdout, "{answer}")?;
            stdout.flush()?;
        }
    }
    Ok(())
}

fn tools() -> Value {
    let window = json!({
        "type": "array", "items": {"type": "number"}, "minItems": 4, "maxItems": 4,
        "description": "An area as [x0, y0, x1, y1] in drawing units."
    });
    json!([
        {
            "name": "command",
            "description": "Type at Linewright's command line, as at AutoCAD's. Each entry is one line typed and Enter: a \
    command or alias (LINE, C, MOVE, TRIM, DIMLINEAR, LAYER…), then the answers to its prompts. \"\" is Enter (ends LINE, \
    accepts a default, or repeats the last command). Points: x,y or @dx,dy or @dist<angle; options: their capital letters \
    (\"C\" for Close). Besides typing, an entry can be \"click x,y\" (a mouse click there: picks an object or gives a point), \
    \"box x1,y1 x2,y2\" (drag a selection box: left to right is a window, right to left crossing), \"select <id> <id>…\" \
    (select objects by id), or \"esc\". U and REDO undo and redo. The reply has the command line's messages, the ids of \
    objects added, changed and erased, and what the command waits for next; a waiting command keeps running into the next call.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "input": {"type": "array", "items": {"type": "string"}, "description": "The entries, in order."},
                    "select": {"type": "array", "items": {"type": "integer"}, "description": "Select these object ids first, so a command such as MOVE or ERASE starts with them."}
                },
                "required": ["input"]
            }
        },
        {
            "name": "render",
            "description": "A PNG picture of the drawing, as it would plot: dark lines on white, lineweights, linetypes, \
    hatches and text. Shows everything in model space, or the open layout's sheet, unless a window is given. The reply says \
    which area it shows and how many pixels make a drawing unit, to work out coordinates from the picture.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "width": {"type": "integer", "description": "Largest width in pixels (default 1200)."},
                    "height": {"type": "integer", "description": "Largest height in pixels (default 900)."},
                    "window": window,
                    "save_to": {"type": "string", "description": "Also write the PNG to this file."}
                }
            }
        },
        {
            "name": "list_entities",
            "description": "The objects in the open space, one a line: id, kind, layer, color and their main geometry. \
    Filter by layer, kind (Line, Circle, Polyline, Text…) or an area. For everything about some objects, select them and \
    run LIST with the command tool.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "layer": {"type": "string"},
                    "kind": {"type": "string"},
                    "window": window,
                    "limit": {"type": "integer", "description": "Most objects listed (default 200)."}
                }
            }
        },
        {
            "name": "drawing_info",
            "description": "The open drawing at a glance: file, units, extent, layers, layouts, blocks, how many objects \
    of each kind, and what the command line is waiting for.",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "open_drawing",
            "description": "Open a DXF or DWG file (DWG needs the ODA File Converter), in place of the open drawing. Unsaved changes are lost.",
            "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}
        },
        {
            "name": "new_drawing",
            "description": "Start a new, empty drawing in place of the open one. Unsaved changes are lost.",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "save_drawing",
            "description": "Save the drawing as DXF or DWG (by the file's suffix). Without a path it saves to the file it came from.",
            "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}}}
        },
        {
            "name": "layer",
            "description": "Make a layer, or change one: its color, linetype, on, frozen, locked and plot settings, and \
    whether it is current (new objects go on the current layer). One undo step.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "color": {"type": "integer", "description": "AutoCAD color index 1-255: 1 red, 2 yellow, 3 green, 4 cyan, 5 blue, 6 magenta, 7 white/black, 8 and 9 greys."},
                    "linetype": {"type": "string", "description": "Continuous, Dashed, Hidden, Center, DashDot, Phantom, Dot…"},
                    "current": {"type": "boolean"},
                    "on": {"type": "boolean"},
                    "frozen": {"type": "boolean"},
                    "locked": {"type": "boolean"},
                    "plot": {"type": "boolean"}
                },
                "required": ["name"]
            }
        },
        {
            "name": "set_property",
            "description": "Set a property of objects, as typing in the Properties palette does. General ones: Layer, \
    Color (ByLayer, ByBlock or 1-255), Linetype, Linetype scale. Geometry ones go by the names LIST and the palette use, \
    such as \"Start X\", \"Center Y\", Radius, Diameter, \"Start angle\", Closed, \"Global width\", Contents, Height, \
    Rotation, \"Text override\", \"Dimension scale\", \"Arrow size\". One undo step.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "ids": {"type": "array", "items": {"type": "integer"}},
                    "name": {"type": "string"},
                    "value": {"type": "string"}
                },
                "required": ["ids", "name", "value"]
            }
        },
        {
            "name": "list_commands",
            "description": "The commands the command tool can run, with what they do and their aliases.",
            "inputSchema": {"type": "object", "properties": {"filter": {"type": "string", "description": "Only those whose name or description holds this."}}}
        }
    ])
}

fn command_list(filter: &str) -> String {
    let filter = filter.to_uppercase();
    let mut out: Vec<String> = COMMANDS
        .iter()
        .filter(|(name, label)| filter.is_empty() || name.contains(&filter) || label.to_uppercase().contains(&filter))
        .map(|(name, label)| {
            let aliases: Vec<&str> = ALIASES.iter().filter(|(_, c)| c == name).map(|(a, _)| *a).collect();
            let aliases = if aliases.is_empty() { String::new() } else { format!(" ({})", aliases.join(", ")) };
            format!("{name}{aliases}: {label}")
        })
        .collect();
    if out.is_empty() {
        out.push("No commands match.".into());
    }
    let window: Vec<&str> = WINDOW_COMMANDS.iter().map(|(name, _)| *name).collect();
    out.push(format!(
        "\nThese work only in the window, not here: {}. Use open_drawing, new_drawing, save_drawing and render for files and pictures.",
        window.join(", ")
    ));
    out.join("\n")
}

fn window_only(typed: &str) -> String {
    let word = typed.to_uppercase();
    let instead = match word.as_str() {
        "QSAVE" | "SAVE" | "SAVEAS" => "Use the save_drawing tool.",
        "OPEN" | "RECOVER" => "Use the open_drawing tool.",
        "NEW" | "QNEW" => "Use the new_drawing tool.",
        "PLOT" | "PRINT" | "EXPORTPDF" => "Use the render tool to see the drawing.",
        "MTEXT" | "MT" | "T" => "Use TEXT for text instead.",
        _ => "It needs the Linewright window.",
    };
    format!("{word} opens a dialog or file box in the window and cannot run here. {instead}")
}

/// One object on a line: its id, kind, layer and color, and the geometry that tells it apart.
fn describe(entity: &Entity) -> String {
    let color = match entity.color {
        256 => "ByLayer".to_string(),
        0 => "ByBlock".to_string(),
        index => format!("color {index}"),
    };
    let shape = match &entity.shape {
        Shape::Line { start, end } => format!("{} to {}, length {}", coords(*start), coords(*end), short(start.dist(*end))),
        Shape::Circle { center, radius } => format!("center {}, radius {}", coords(*center), short(*radius)),
        Shape::Arc { center, radius, start_angle, end_angle } => {
            format!("center {}, radius {}, from {}° to {}°", coords(*center), short(*radius), short(*start_angle), short(*end_angle))
        }
        Shape::Polyline { vertices, closed } => {
            let corners: Vec<String> = vertices.iter().take(12).map(|v| coords(pt(v.x, v.y))).collect();
            let more = if vertices.len() > 12 { format!(" … {} vertices", vertices.len()) } else { String::new() };
            let arcs = if vertices.iter().any(|v| v.bulge != 0.0) { ", with arcs" } else { "" };
            format!("{}: {}{more}{arcs}", if *closed { "closed" } else { "open" }, corners.join("  "))
        }
        Shape::Text { position, text, height, rotation, .. } => {
            let rotation = if *rotation != 0.0 { format!(", rotated {}°", short(*rotation)) } else { String::new() };
            format!("{:?} at {}, height {}{rotation}", text, coords(*position), short(*height))
        }
        Shape::Point { position, .. } => format!("at {}", coords(*position)),
        Shape::Ellipse { center, major, ratio, .. } => {
            format!("center {}, major axis to {}, ratio {}", coords(*center), coords(*center + *major), short(*ratio))
        }
        Shape::Insert { block, position, .. } => format!("block {:?} at {}", block.0.name, coords(*position)),
        _ => {
            let b = entity_extent(entity);
            format!("from {} to {}", coords(pt(b.x0, b.y0)), coords(pt(b.x1, b.y1)))
        }
    };
    format!("{} {} on {} ({color}): {shape}", entity.id, entity.shape.kind(), entity.layer)
}

fn answer_text(answer: &crate::commands::prompts::Answer) -> String {
    use crate::commands::prompts::Answer;
    match answer {
        Answer::Point(p) => coords(*p),
        Answer::Number(n) => short(*n),
        Answer::Keyword(k) | Answer::Text(k) => k.clone(),
        _ => "the default".into(),
    }
}

fn parse_color(text: &str) -> Option<i32> {
    match text.trim().to_lowercase().as_str() {
        "bylayer" => Some(256),
        "byblock" => Some(0),
        number => number.parse::<i32>().ok().filter(|c| (1..=255).contains(c)),
    }
}

fn units_name(mm_per_unit: f64) -> &'static str {
    const NAMES: [(f64, &str); 6] =
        [(1.0, "millimeters"), (10.0, "centimeters"), (1000.0, "meters"), (25.4, "inches"), (304.8, "feet"), (1_000_000.0, "kilometers")];
    NAMES.iter().find(|(mm, _)| (mm - mm_per_unit).abs() < 1e-6 * mm).map_or("unitless", |(_, name)| name)
}

/// A number with up to four decimals, without trailing zeros.
fn short(value: f64) -> String {
    let text = format!("{value:.4}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" { "0".into() } else { text.into() }
}

fn coords(p: Point) -> String {
    format!("{},{}", short(p.x), short(p.y))
}

fn parse_point(text: &str) -> Option<Point> {
    crate::commands::parse::parse_point(text, None)
}

fn bounds_of(value: &Value) -> Option<Bounds> {
    let numbers: Vec<f64> = value.as_array()?.iter().filter_map(Value::as_f64).collect();
    let [x0, y0, x1, y1] = numbers[..] else { return None };
    Some(Bounds::from_corners(pt(x0, y0), pt(x1, y1)))
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | (*b as u32) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(server: &mut Server, tool: &str, arguments: Value) -> Value {
        let message = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": tool, "arguments": arguments}});
        server.handle(&message).unwrap()["result"].clone()
    }

    fn text(result: &Value) -> String {
        result["content"].as_array().unwrap().iter().filter_map(|c| c["text"].as_str()).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn it_introduces_itself_and_lists_its_tools() {
        let mut server = Server::new();
        let hello = json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {"protocolVersion": "2025-03-26"}});
        let answer = server.handle(&hello).unwrap();
        assert_eq!(answer["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(answer["result"]["serverInfo"]["name"], "linewright");
        assert!(server.handle(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).is_none());
        let tools = server.handle(&json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).unwrap();
        let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"command") && names.contains(&"render"));
    }

    #[test]
    fn typed_commands_draw_and_say_what_changed() {
        let mut server = Server::new();
        let result = call(&mut server, "command", json!({"input": ["LINE", "0,0", "100,0", "@0,50"]}));
        let reply = text(&result);
        assert!(reply.contains("Added: "), "{reply}");
        assert!(reply.contains("LINE is waiting for a point"), "a waiting command keeps running: {reply}");
        let reply = text(&call(&mut server, "command", json!({"input": ["c"]})));
        assert!(reply.contains("Command: (no command running)"), "{reply}");
        assert_eq!(server.controller.document().len(), 3);
        let listed = text(&call(&mut server, "list_entities", json!({"kind": "line"})));
        assert!(listed.contains("Line on 0 (ByLayer): 0,0 to 100,0, length 100"), "{listed}");
        call(&mut server, "command", json!({"input": ["u"]}));
        assert_eq!(server.controller.document().len(), 2, "U takes back the last segment");
    }

    #[test]
    fn objects_are_selected_by_id_click_or_box() {
        let mut server = Server::new();
        call(&mut server, "command", json!({"input": ["CIRCLE", "0,0", "10", "CIRCLE", "100,0", "10"]}));
        let ids: Vec<u64> = server.controller.document().entities().iter().map(|e| e.id).collect();
        let reply = text(&call(&mut server, "command", json!({"input": ["MOVE", "0,0", "@0,20"], "select": [ids[0]]})));
        assert!(reply.contains(&format!("Changed: {}", ids[0])), "{reply}");
        call(&mut server, "command", json!({"input": ["ERASE", "click 110,0", ""]}));
        assert_eq!(server.controller.document().len(), 1, "the click picked the circle on its edge");
        call(&mut server, "command", json!({"input": ["ERASE", "box -20,-20 20,40", ""]}));
        assert!(server.controller.document().is_empty());
    }

    #[test]
    fn window_commands_point_to_the_tools() {
        let mut server = Server::new();
        let reply = text(&call(&mut server, "command", json!({"input": ["QSAVE"]})));
        assert!(reply.contains("save_drawing"), "{reply}");
    }

    #[test]
    fn a_render_is_a_png_with_its_scale() {
        let mut server = Server::new();
        assert_eq!(call(&mut server, "render", json!({}))["isError"], true, "nothing to show yet");
        call(&mut server, "command", json!({"input": ["RECTANG", "0,0", "200,100"]}));
        let result = call(&mut server, "render", json!({"width": 400, "height": 400}));
        let image = &result["content"][0];
        assert_eq!(image["mimeType"], "image/png");
        assert!(image["data"].as_str().unwrap().starts_with("iVBORw0KGgo"), "PNG's signature in base64");
        assert!(text(&result).contains("pixels a unit"));
    }

    #[test]
    fn drawings_save_and_open_again() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("plan.dxf");
        let mut server = Server::new();
        call(&mut server, "command", json!({"input": ["CIRCLE", "5,5", "2"]}));
        let saved = call(&mut server, "save_drawing", json!({"path": path.to_str().unwrap()}));
        assert_eq!(saved["isError"], false, "{}", text(&saved));
        call(&mut server, "new_drawing", json!({}));
        let opened = text(&call(&mut server, "open_drawing", json!({"path": path.to_str().unwrap()})));
        assert!(opened.contains("Objects: 1 (1 Circle)"), "{opened}");
    }

    #[test]
    fn layers_are_made_current_and_objects_moved_onto_them() {
        let mut server = Server::new();
        let reply = text(&call(&mut server, "layer", json!({"name": "Walls", "color": 1, "current": true})));
        assert_eq!(reply, "Made layer Walls, and it is current.");
        call(&mut server, "command", json!({"input": ["CIRCLE", "0,0", "5"]}));
        let circle = server.controller.document().entities()[0].clone();
        assert_eq!(circle.layer, "Walls");
        call(&mut server, "layer", json!({"name": "Doors"}));
        let moved = call(&mut server, "set_property", json!({"ids": [circle.id], "name": "Layer", "value": "doors"}));
        assert_eq!(text(&moved), "Layer set on 1 object(s).");
        call(&mut server, "set_property", json!({"ids": [circle.id], "name": "Radius", "value": "8"}));
        let now = &server.controller.document().entities()[0];
        assert_eq!((now.layer.as_str(), &now.shape), ("Doors", &Shape::Circle { center: pt(0.0, 0.0), radius: 8.0 }));
        let refused = call(&mut server, "layer", json!({"name": "Walls", "frozen": true}));
        assert_eq!(refused["isError"], true, "the current layer cannot be frozen");
        call(&mut server, "command", json!({"input": ["u", "u"]}));
        assert_eq!(server.controller.document().entities()[0].layer, "Walls", "each change undoes on its own");
    }

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }
}
