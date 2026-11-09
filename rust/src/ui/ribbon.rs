//! The ribbon, laid out as AutoCAD's: a row of tabs over panels of large and small buttons. Each panel has its name
//! in a strip underneath, which opens the tools the panel has no room for.

use eframe::egui::{
    self, Align, Color32, CornerRadius, FontId, Id, Layout, Pos2, Rect, Response, Sense, Stroke, StrokeKind, UiBuilder, pos2,
    text::LayoutJob, vec2,
};

use super::app::{Action, LinewrightApp, file_command};
use super::panels::{GeneralValues, color_combo, linetype_combo, lineweight_combo};
use super::{icons, theme};
use crate::canvas::aci::color_rgb;
use crate::commands::registry::{COMMANDS, WINDOW_COMMANDS, label, resolve, shortest_alias};
use crate::model::lineweight;

const TAB_HEIGHT: f32 = 24.0;
/// A row of small buttons; a panel holds three.
const ROW: f32 = 22.0;
const ROW_GAP: f32 = 1.0;
const CONTENT_HEIGHT: f32 = 3.0 * ROW + 2.0 * ROW_GAP;
/// The room above a panel's buttons and between them and its title strip.
const PANEL_TOP: f32 = 4.0;
const PANEL_BOTTOM: f32 = 3.0;
const TITLE_HEIGHT: f32 = 16.0;
const BODY_HEIGHT: f32 = PANEL_TOP + CONTENT_HEIGHT + PANEL_BOTTOM + TITLE_HEIGHT;
/// The room at each side of a panel, and between its parts.
const PANEL_PAD: f32 = 3.0;
const PART_GAP: f32 = 2.0;
/// The groove between two panels.
const SEPARATOR: f32 = 2.0;
const LARGE_ICON: f32 = 32.0;
const SMALL_ICON: f32 = 16.0;
/// A small button showing only its icon.
const ICON_WIDTH: f32 = 24.0;
/// The ▾ part of a button with a flyout.
const SPLIT: f32 = 11.0;
const FONT: f32 = 11.5;
const LAYER_FIELD: f32 = 172.0;
const LAYER_TOOL: f32 = 24.0;
const PROPERTY_FIELD: f32 = 122.0;

/// One tool: the command it runs, an option typed after it, its icon, and its words when they differ from the
/// command's label.
#[derive(Clone, Copy)]
struct Tool {
    command: &'static str,
    option: Option<&'static str>,
    icon: &'static str,
    text: &'static str,
}

const fn tool(command: &'static str) -> Tool {
    Tool { command, option: None, icon: command, text: "" }
}

const fn named(command: &'static str, text: &'static str) -> Tool {
    Tool { command, option: None, icon: command, text }
}

/// A command started with one of its options, as AutoCAD's flyouts start CIRCLE with 2P.
const fn with(command: &'static str, option: &'static str, icon: &'static str, text: &'static str) -> Tool {
    Tool { command, option: Some(option), icon, text }
}

impl Tool {
    fn text(&self) -> &'static str {
        match (self.text, label(self.command)) {
            ("", "") => self.command,
            ("", label) => label,
            (text, _) => text,
        }
    }

    /// What the tool does and what to type for it.
    fn tip(&self) -> String {
        let typed = match self.option {
            Some(option) => format!("{} {option}", self.command),
            None => match shortest_alias(self.command) {
                "" => self.command.to_string(),
                alias => alias.to_string(),
            },
        };
        let name = if self.option.is_none() && !label(self.command).is_empty() { label(self.command) } else { self.text() };
        // Tools that are the window's own, such as Zoom In, have nothing to type.
        if self.command.contains('_') { name.replace('\n', " ") } else { format!("{} ({typed})", name.replace('\n', " ")) }
    }

    /// What clicking the tool does: its command, typed as at the command line. A command another part of
    /// Linewright does not know yet does what comes closest, or says it is unknown.
    fn action(&self) -> Action {
        let command = self.command;
        if let Some(&(name, _)) = COMMANDS.iter().find(|(c, _)| *c == command) {
            return match self.option {
                Some(option) => Action::CommandWith(name, option),
                None => Action::Command(name),
            };
        }
        if let Some(&(name, _)) = WINDOW_COMMANDS.iter().find(|(c, _)| *c == command) {
            return file_command(name).unwrap_or(Action::Command(name));
        }
        if let Some(name) = resolve(command) {
            return Action::Command(name);
        }
        match (command, self.option) {
            ("LAYER", _) => Action::Command("LAYER"),
            ("SELECTALL", _) => Action::SelectAll,
            ("COPYCLIP", _) => Action::Copy,
            ("CUTCLIP", _) => Action::Cut,
            ("MEASUREGEOM", Some("AR")) => Action::Command("AREA"),
            ("MEASUREGEOM", _) => Action::Command("DISTANCE"),
            ("ZOOM_IN", _) => Action::ZoomIn,
            ("ZOOM_OUT", _) => Action::ZoomOut,
            ("PROPERTIES", _) => Action::ToggleProperties,
            _ => Action::Input(command.to_string()),
        }
    }

    /// Whether the tool shows as on: its command runs, or the setting or palette it toggles is on.
    fn checked(&self, app: &LinewrightApp) -> bool {
        match self.command {
            "PICKSTYLE" => app.controller.document().group_selection,
            "LWDISPLAY" => app.controller.document().show_lineweights,
            "LAYER" => app.palette_open,
            "PROPERTIES" => app.properties_open,
            command => app.controller.running_command().is_some_and(|running| {
                running == command || Some(running) == resolve(command) || (command == "MTEXT" && running == "MTEXTBOX")
            }),
        }
    }
}

/// A button: one tool, or a flyout of several under a title. A flyout runs the tool used last from it, and its ▾
/// lists them all.
#[derive(Clone, Copy)]
enum Button {
    One(Tool),
    Flyout(&'static str, &'static [Tool]),
}

impl Button {
    fn tools(&self) -> &[Tool] {
        match self {
            Button::One(tool) => std::slice::from_ref(tool),
            Button::Flyout(_, tools) => tools,
        }
    }

    /// Where the flyout keeps the tool used last.
    fn memory(&self) -> Id {
        let first = self.tools()[0];
        Id::new(("ribbon_flyout", first.command, first.option))
    }

    /// The tool a click runs: the one used last from a flyout.
    fn current(&self, ui: &egui::Ui) -> Tool {
        let tools = self.tools();
        let last = ui.data(|d| d.get_temp::<usize>(self.memory())).unwrap_or(0);
        tools[last.min(tools.len() - 1)]
    }

    fn checked(&self, app: &LinewrightApp) -> bool {
        self.tools().iter().any(|tool| tool.checked(app))
    }
}

/// What a panel holds, left to right.
enum Part {
    /// A tall button: the icon with its words under it.
    Large(Button),
    /// A column of up to three small buttons showing only their icons.
    Small(&'static [Button]),
    /// A column of small buttons with their words.
    Labelled(&'static [Button]),
    /// The current layer's drop-down over two rows of layer tools.
    Layers,
    /// The color, lineweight and linetype drop-downs.
    Properties,
}

struct Panel {
    title: &'static str,
    parts: &'static [Part],
    /// Tools only the panel's expanded list shows.
    more: &'static [Tool],
}

use Button::{Flyout, One};

const TEXTS: &[Tool] = &[named("MTEXT", "Multiline Text"), named("TEXT", "Single Line")];
const DIMENSIONS: &[Tool] = &[
    named("DIMLINEAR", "Linear"),
    named("DIMALIGNED", "Aligned"),
    named("DIMANGULAR", "Angular"),
    named("DIMARC", "Arc Length"),
    named("DIMRADIUS", "Radius"),
    named("DIMDIAMETER", "Diameter"),
    named("DIMJOGGED", "Jogged"),
    named("DIMORDINATE", "Ordinate"),
];
const LEADERS: &[Tool] = &[
    named("MLEADER", "Multileader"),
    named("MLEADERADD", "Add Leader"),
    named("MLEADERREMOVE", "Remove Leader"),
    named("QLEADER", "Leader"),
];
const ATTRIBUTES: &[Tool] = &[named("ATTEDIT", "Edit Attribute"), named("ATTDEF", "Define Attributes")];
// CIRCLE and ARC take these options at their first prompt; the others AutoCAD lists need a point first.
const CIRCLES: &[Tool] =
    &[named("CIRCLE", "Center, Radius"), with("CIRCLE", "2P", "CIRCLE", "2-Point"), with("CIRCLE", "3P", "CIRCLE", "3-Point")];
const ARCS: &[Tool] = &[named("ARC", "3-Point"), with("ARC", "C", "ARC", "Center, Start, End")];
const ELLIPSES: &[Tool] = &[named("ELLIPSE", "Axis, End"), with("ELLIPSE", "C", "ELLIPSE", "Center")];
const MEASURES: &[Tool] = &[
    with("MEASUREGEOM", "D", "DISTANCE", "Distance"),
    with("MEASUREGEOM", "R", "DIMRADIUS", "Radius"),
    with("MEASUREGEOM", "A", "DIMANGULAR", "Angle"),
    with("MEASUREGEOM", "AR", "AREA", "Area"),
];

const HOME: &[Panel] = &[
    Panel {
        title: "Modify",
        parts: &[
            Part::Labelled(&[One(named("MOVE", "Move")), One(named("COPY", "Copy")), One(named("STRETCH", "Stretch"))]),
            Part::Small(&[One(tool("ROTATE")), One(tool("MIRROR")), One(tool("SCALE"))]),
            Part::Small(&[
                Flyout("Trim", &[named("TRIM", "Trim"), named("EXTEND", "Extend")]),
                Flyout("Fillet", &[named("FILLET", "Fillet"), named("CHAMFER", "Chamfer")]),
                One(tool("ARRAY")),
            ]),
            Part::Small(&[One(tool("ERASE")), One(tool("EXPLODE")), One(tool("OFFSET"))]),
        ],
        more: &[
            tool("BREAK"),
            tool("JOIN"),
            tool("LENGTHEN"),
            tool("ALIGN"),
            tool("PEDIT"),
            tool("SPLINEDIT"),
            tool("HATCHEDIT"),
            tool("MLEDIT"),
            tool("DRAWORDER"),
            tool("OVERKILL"),
        ],
    },
    Panel { title: "Layers", parts: &[Part::Large(One(named("LAYER", "Layer\nProperties"))), Part::Layers], more: &[tool("VPLAYER")] },
    Panel {
        title: "Annotation",
        parts: &[
            Part::Large(Flyout("Text", TEXTS)),
            Part::Large(One(named("DIMLINEAR", "Dimension"))),
            Part::Small(&[Flyout("Linear", DIMENSIONS), Flyout("Leader", LEADERS), One(tool("TABLE"))]),
        ],
        more: &[
            tool("DIMCONTINUE"),
            tool("DIMBASELINE"),
            tool("QDIM"),
            tool("DIMTEDIT"),
            tool("DIMSTYLE"),
            tool("STYLE"),
            tool("MLEADERSTYLE"),
        ],
    },
    Panel {
        title: "Block",
        parts: &[
            Part::Large(One(named("INSERT", "Insert"))),
            Part::Small(&[One(named("BLOCK", "Create")), One(named("BEDIT", "Edit")), Flyout("Attributes", ATTRIBUTES)]),
        ],
        more: &[],
    },
    Panel {
        title: "Properties",
        parts: &[Part::Large(One(named("MATCHPROP", "Match\nProperties"))), Part::Properties],
        more: &[tool("LIST"), tool("LTSCALE")],
    },
    Panel {
        title: "Draw",
        parts: &[
            Part::Large(One(named("LINE", "Line"))),
            Part::Large(One(named("PLINE", "Polyline"))),
            Part::Large(Flyout("Circle", CIRCLES)),
            Part::Large(Flyout("Arc", ARCS)),
            Part::Small(&[
                Flyout("Rectangle", &[named("RECTANGLE", "Rectangle"), named("POLYGON", "Polygon")]),
                Flyout("Ellipse", ELLIPSES),
                Flyout("Hatch", &[named("HATCH", "Hatch"), named("GRADIENT", "Gradient"), named("BOUNDARY", "Boundary")]),
            ]),
        ],
        more: &[
            tool("SPLINE"),
            tool("POINT"),
            tool("MLINE"),
            tool("XLINE"),
            tool("RAY"),
            tool("DONUT"),
            tool("REVCLOUD"),
            tool("WIPEOUT"),
            tool("DIVIDE"),
            tool("MEASURE"),
        ],
    },
    Panel {
        title: "Groups",
        parts: &[
            Part::Large(One(named("GROUP", "Group"))),
            Part::Small(&[One(tool("UNGROUP")), One(tool("GROUPEDIT")), One(named("PICKSTYLE", "Group Selection On/Off"))]),
        ],
        more: &[],
    },
    Panel {
        title: "Utilities",
        parts: &[
            Part::Large(Flyout("Measure", MEASURES)),
            Part::Small(&[
                One(named("QSELECT", "Quick Select")),
                One(named("SELECTALL", "Select All")),
                One(named("QUICKCALC", "QuickCalc")),
            ]),
        ],
        more: &[tool("ID")],
    },
    Panel {
        title: "Clipboard",
        parts: &[
            Part::Large(One(named("PASTECLIP", "Paste"))),
            Part::Small(&[One(named("CUTCLIP", "Cut")), One(named("COPYCLIP", "Copy Clip"))]),
        ],
        more: &[],
    },
];

const INSERT: &[Panel] = &[
    Panel {
        title: "Block",
        parts: &[
            Part::Large(One(named("INSERT", "Insert"))),
            Part::Labelled(&[One(named("BLOCK", "Create")), One(named("BEDIT", "Edit")), One(named("ATTDEF", "Define Attributes"))]),
            Part::Labelled(&[One(named("ATTEDIT", "Edit Attribute"))]),
        ],
        more: &[],
    },
    Panel {
        title: "Reference",
        parts: &[
            Part::Large(One(named("XATTACH", "Attach"))),
            Part::Labelled(&[One(named("XREF", "External References")), One(named("IMAGEATTACH", "Attach Image"))]),
        ],
        more: &[],
    },
];

const ANNOTATE: &[Panel] = &[
    Panel {
        title: "Text",
        parts: &[Part::Large(Flyout("Text", TEXTS)), Part::Labelled(&[One(named("STYLE", "Text Style")), One(named("FIND", "Find Text"))])],
        more: &[],
    },
    Panel {
        title: "Dimensions",
        parts: &[
            Part::Large(One(named("DIMLINEAR", "Dimension"))),
            Part::Labelled(&[Flyout("Linear", DIMENSIONS), One(named("DIMCONTINUE", "Continue")), One(named("DIMBASELINE", "Baseline"))]),
            Part::Labelled(&[One(named("QDIM", "Quick")), One(named("DIMTEDIT", "Text Position")), One(named("DIMSCALE", "Scale"))]),
            Part::Large(One(named("DIMSTYLE", "Dimension\nStyle"))),
        ],
        more: &[],
    },
    Panel {
        title: "Leaders",
        parts: &[
            Part::Large(One(named("MLEADER", "Multileader"))),
            Part::Labelled(&[
                One(named("MLEADERADD", "Add Leader")),
                One(named("MLEADERREMOVE", "Remove Leader")),
                One(named("QLEADER", "Leader")),
            ]),
            Part::Labelled(&[One(named("MLEADERSTYLE", "Multileader Style"))]),
        ],
        more: &[],
    },
    Panel {
        title: "Tables",
        parts: &[Part::Large(One(named("TABLE", "Table"))), Part::Labelled(&[One(named("TABLEDIT", "Edit Cell"))])],
        more: &[],
    },
    Panel {
        title: "Markup",
        parts: &[Part::Large(One(named("REVCLOUD", "Revision\nCloud"))), Part::Labelled(&[One(named("WIPEOUT", "Wipeout"))])],
        more: &[],
    },
];

const VIEW: &[Panel] = &[
    Panel {
        title: "Navigate",
        parts: &[
            Part::Large(One(with("ZOOM", "E", "ZOOM_EXTENTS", "Extents"))),
            Part::Labelled(&[
                One(with("ZOOM", "W", "ZOOM", "Window")),
                One(with("ZOOM", "P", "ZOOM", "Previous")),
                One(with("ZOOM", "A", "ZOOM_EXTENTS", "All")),
            ]),
            Part::Labelled(&[One(named("ZOOM_IN", "Zoom In")), One(named("ZOOM_OUT", "Zoom Out"))]),
        ],
        more: &[],
    },
    Panel {
        title: "Viewports",
        parts: &[
            Part::Large(One(named("MVIEW", "Viewport"))),
            Part::Labelled(&[
                One(named("MSPACE", "Model Space")),
                One(named("PSPACE", "Paper Space")),
                One(named("VPLAYER", "Viewport Layers")),
            ]),
        ],
        more: &[],
    },
    Panel {
        title: "Palettes",
        parts: &[Part::Large(One(named("PROPERTIES", "Properties"))), Part::Large(One(named("LAYER", "Layer\nProperties")))],
        more: &[],
    },
    Panel {
        title: "Interface",
        parts: &[Part::Large(One(Tool { command: "LWDISPLAY", option: None, icon: "LWT", text: "Show\nLineweight" }))],
        more: &[],
    },
];

const MANAGE: &[Panel] = &[
    Panel {
        title: "Cleanup",
        parts: &[
            Part::Large(One(named("PURGE", "Purge"))),
            Part::Labelled(&[One(named("AUDIT", "Audit")), One(named("RECOVER", "Recover")), One(named("OVERKILL", "Overkill"))]),
        ],
        more: &[],
    },
    Panel {
        title: "Settings",
        parts: &[
            Part::Large(One(named("UNITS", "Units"))),
            Part::Labelled(&[One(named("LWEIGHT", "Lineweight")), One(named("OSNAP", "Object Snap")), One(named("SAVETIME", "Autosave"))]),
            Part::Labelled(&[One(named("DRAWINGRECOVERY", "Drawing Recovery"))]),
        ],
        more: &[],
    },
    Panel {
        title: "Styles",
        parts: &[
            Part::Labelled(&[
                One(named("STYLE", "Text Style")),
                One(named("DIMSTYLE", "Dimension Style")),
                One(named("MLSTYLE", "Multiline Style")),
            ]),
            Part::Labelled(&[One(named("MLEADERSTYLE", "Multileader Style"))]),
        ],
        more: &[],
    },
];

const OUTPUT: &[Panel] = &[
    Panel { title: "Plot", parts: &[Part::Large(One(named("PLOT", "Plot")))], more: &[] },
    Panel {
        title: "Layouts",
        parts: &[Part::Large(One(named("LAYOUT", "Layout"))), Part::Labelled(&[One(named("MVIEW", "Viewport"))])],
        more: &[],
    },
];

const TABS: &[(&str, &[Panel])] =
    &[("Home", HOME), ("Insert", INSERT), ("Annotate", ANNOTATE), ("View", VIEW), ("Manage", MANAGE), ("Output", OUTPUT)];

fn tab_id() -> Id {
    Id::new("ribbon_tab")
}

fn collapsed_id() -> Id {
    Id::new("ribbon_collapsed")
}

pub fn ribbon(app: &mut LinewrightApp, ctx: &egui::Context) {
    let frame = egui::Frame::new().fill(theme::RIBBON_TABS);
    egui::TopBottomPanel::top("ribbon").frame(frame).show_separator_line(false).show(ctx, |ui| {
        ui.add_enabled_ui(!app.is_opening(), |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            let tab = tab_row(ui);
            if ui.data(|d| d.get_temp::<bool>(collapsed_id())).unwrap_or(false) {
                return;
            }
            let (body, _) = ui.allocate_exact_size(vec2(ui.available_width(), BODY_HEIGHT + 2.0), Sense::hover());
            ui.painter().rect_filled(body, 0.0, theme::RIBBON);
            ui.painter().line_segment(
                [body.left_bottom() - vec2(0.0, 1.0), body.right_bottom() - vec2(0.0, 1.0)],
                Stroke::new(2.0_f32, theme::RIBBON_EDGE),
            );
            let mut inside =
                ui.new_child(UiBuilder::new().max_rect(body.shrink2(vec2(2.0, 0.0))).layout(Layout::left_to_right(Align::Min)));
            inside.style_mut().spacing.scroll = egui::style::ScrollStyle::solid();
            let budget = inside.available_width();
            egui::ScrollArea::horizontal()
                .id_salt(("ribbon_panels", tab))
                .max_width(budget)
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
                .show(&mut inside, |ui| ui.horizontal(|ui| panels(app, ui, TABS[tab].1, budget)));
        });
    });
}

/// The tabs, with the open one joined to the panels under it. Double-clicking the open tab hides the panels, and
/// clicking a tab shows them again. Returns the open tab.
fn tab_row(ui: &mut egui::Ui) -> usize {
    let mut open = ui.data(|d| d.get_temp::<usize>(tab_id())).unwrap_or(0).min(TABS.len() - 1);
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), TAB_HEIGHT), Sense::hover());
    let mut x = row.left() + 8.0;
    for (index, (name, _)) in TABS.iter().enumerate() {
        let galley = ui.fonts(|f| f.layout_no_wrap(name.to_string(), FontId::proportional(13.0), theme::RIBBON_TAB_TEXT));
        let rect = Rect::from_min_size(pos2(x, row.top() + 3.0), vec2(galley.size().x + 22.0, TAB_HEIGHT - 3.0));
        x = rect.right() + 2.0;
        let response = ui.interact(rect, Id::new(("ribbon_tab_button", index)), Sense::click());
        let color = if index == open {
            ui.painter().rect_filled(rect, CornerRadius { nw: 3, ne: 3, sw: 0, se: 0 }, theme::RIBBON);
            Color32::WHITE
        } else if response.hovered() {
            ui.painter().rect_filled(rect, CornerRadius { nw: 3, ne: 3, sw: 0, se: 0 }, theme::RIBBON_TABS.gamma_multiply(1.6));
            Color32::WHITE
        } else {
            theme::RIBBON_TAB_TEXT
        };
        ui.painter().galley(rect.center() - galley.size() / 2.0, galley, color);
        if response.double_clicked() && index == open {
            ui.data_mut(|d| d.insert_temp(collapsed_id(), !d.get_temp::<bool>(collapsed_id()).unwrap_or(false)));
        } else if response.clicked() && index != open {
            open = index;
            ui.data_mut(|d| {
                d.insert_temp(tab_id(), index);
                d.insert_temp(collapsed_id(), false);
            });
        }
    }
    open
}

/// The panels of a tab, left to right. Each keeps as many columns of small buttons as fit in the room it gets
/// (see `shared_columns`); the rest go in the list its title strip opens.
fn panels(app: &mut LinewrightApp, ui: &mut egui::Ui, panels: &[Panel], budget: f32) {
    let sizes: Vec<GroupSize> = panels.iter().map(|panel| panel.size(ui)).collect();
    let room = budget - panels.len() as f32 * (2.0 * PANEL_PAD + SEPARATOR);
    for ((panel, size), columns) in panels.iter().zip(&sizes).zip(shared_columns(&sizes, room, PART_GAP)) {
        panel.show(app, ui, size, columns);
        let (groove, _) = ui.allocate_exact_size(vec2(SEPARATOR, BODY_HEIGHT), Sense::hover());
        let top = groove.top() + 3.0;
        ui.painter().line_segment(
            [pos2(groove.left() + 0.5, top), pos2(groove.left() + 0.5, groove.bottom())],
            Stroke::new(1.0_f32, theme::RIBBON_EDGE),
        );
        ui.painter().line_segment(
            [pos2(groove.right() - 0.5, top), pos2(groove.right() - 0.5, groove.bottom())],
            Stroke::new(1.0_f32, theme::RIBBON_HOVER),
        );
    }
}

/// The room a panel's parts take, measured before anything is drawn.
struct GroupSize {
    /// The parts that always show: large buttons and drop-downs.
    large_width: f32,
    large_count: usize,
    title_width: f32,
    /// The width of each column of small buttons, left to right.
    column_widths: Vec<f32>,
}

impl GroupSize {
    /// The width of the buttons with the first `columns` small columns.
    fn content(&self, columns: usize, spacing: f32) -> f32 {
        let items = self.large_count + columns;
        self.large_width + self.column_widths[..columns].iter().sum::<f32>() + spacing * items.saturating_sub(1) as f32
    }

    /// The width of the panel with its first `columns` small columns: at least that of its title.
    fn width(&self, columns: usize, spacing: f32) -> f32 {
        self.content(columns, spacing).max(self.title_width)
    }
}

/// How many small columns each panel shows in `room`: one more column to each panel in turn while they fit, so
/// every panel keeps some of its buttons on a narrow ribbon instead of the first panels taking it all.
fn shared_columns(sizes: &[GroupSize], room: f32, spacing: f32) -> Vec<usize> {
    let used = |columns: &[usize]| -> f32 { sizes.iter().zip(columns).map(|(size, &n)| size.width(n, spacing)).sum() };
    let mut columns = vec![0; sizes.len()];
    let mut grew = true;
    while grew {
        grew = false;
        for index in 0..sizes.len() {
            if columns[index] == sizes[index].column_widths.len() {
                continue;
            }
            columns[index] += 1;
            if used(&columns) <= room {
                grew = true;
            } else {
                columns[index] -= 1;
            }
        }
    }
    columns
}

fn text_width(ui: &egui::Ui, text: &str) -> f32 {
    ui.fonts(|f| f.layout_no_wrap(text.to_string(), FontId::proportional(FONT), theme::RIBBON_TEXT)).size().x
}

/// The words under a large button: a flyout's title, or the tool's own.
fn large_text(button: &Button) -> &'static str {
    match button {
        One(tool) => tool.text(),
        Flyout(title, _) => title,
    }
}

fn large_width(ui: &egui::Ui, button: &Button) -> f32 {
    (text_width(ui, large_text(button)) + 8.0).max(LARGE_ICON + 8.0)
}

/// The width of a small button; with its words, room for the longest a flyout may show.
fn small_width(ui: &egui::Ui, button: &Button, labelled: bool) -> f32 {
    let split = if matches!(button, Flyout(..)) { SPLIT } else { 0.0 };
    if labelled {
        let words = button.tools().iter().map(|tool| text_width(ui, tool.text())).fold(0.0, f32::max);
        4.0 + SMALL_ICON + 6.0 + words + 8.0 + split
    } else {
        ICON_WIDTH + split
    }
}

fn column_width(ui: &egui::Ui, buttons: &[Button], labelled: bool) -> f32 {
    buttons.iter().map(|button| small_width(ui, button, labelled)).fold(0.0, f32::max)
}

fn title_font() -> FontId {
    FontId::proportional(11.5)
}

impl Panel {
    fn size(&self, ui: &egui::Ui) -> GroupSize {
        let mut size = GroupSize { large_width: 0.0, large_count: 0, title_width: 0.0, column_widths: vec![] };
        for part in self.parts {
            let fixed = match part {
                Part::Large(button) => large_width(ui, button),
                Part::Layers => LAYER_FIELD,
                Part::Properties => SMALL_ICON + 6.0 + PROPERTY_FIELD,
                Part::Small(buttons) => {
                    size.column_widths.push(column_width(ui, buttons, false));
                    continue;
                }
                Part::Labelled(buttons) => {
                    size.column_widths.push(column_width(ui, buttons, true));
                    continue;
                }
            };
            size.large_width += fixed;
            size.large_count += 1;
        }
        let title = ui.fonts(|f| f.layout_no_wrap(self.title.to_string(), title_font(), theme::RIBBON_TITLE_TEXT)).size().x;
        size.title_width = title + 20.0;
        size
    }

    /// Draws the panel with its first `columns` small columns; the others go in the list its title strip opens.
    fn show(&self, app: &mut LinewrightApp, ui: &mut egui::Ui, size: &GroupSize, columns: usize) {
        let width = size.width(columns, PART_GAP) + 2.0 * PANEL_PAD;
        let (rect, _) = ui.allocate_exact_size(vec2(width, BODY_HEIGHT), Sense::hover());
        let content_width = size.content(columns, PART_GAP);
        let left = rect.left() + (width - content_width) / 2.0;
        let content = Rect::from_min_size(pos2(left, rect.top() + PANEL_TOP), vec2(content_width, CONTENT_HEIGHT));
        let mut parts = ui
            .new_child(UiBuilder::new().id_salt(("ribbon_panel", self.title)).max_rect(content).layout(Layout::left_to_right(Align::Min)));
        parts.spacing_mut().item_spacing = vec2(PART_GAP, ROW_GAP);
        let mut hidden: Vec<Tool> = vec![];
        let mut column = 0;
        for part in self.parts {
            match part {
                Part::Large(button) => large_button(app, &mut parts, button),
                Part::Small(buttons) | Part::Labelled(buttons) => {
                    if column < columns {
                        let labelled = matches!(part, Part::Labelled(_));
                        let width = column_width(&parts, buttons, labelled);
                        parts.vertical(|ui| {
                            for button in buttons.iter() {
                                small_button(app, ui, button, labelled, width);
                            }
                        });
                    } else {
                        hidden.extend(buttons.iter().flat_map(|button| button.tools().iter().copied()));
                    }
                    column += 1;
                }
                Part::Layers => layers(app, &mut parts),
                Part::Properties => properties(app, &mut parts),
            }
        }
        hidden.extend(self.more);
        let strip = Rect::from_min_max(pos2(rect.left(), rect.bottom() - TITLE_HEIGHT), rect.max);
        title_strip(app, ui, strip, self.title, &hidden);
    }
}

/// A button's look: lighter under the pointer, AutoCAD's blue when on.
fn background(ui: &egui::Ui, rect: Rect, hovered: bool, checked: bool) {
    let (fill, edge) = match (checked, hovered) {
        (true, _) => (theme::RIBBON_CHECKED, theme::RIBBON_CHECKED_EDGE),
        (false, true) => (theme::RIBBON_HOVER, theme::RIBBON_HOVER_EDGE),
        (false, false) => return,
    };
    ui.painter().rect(rect, CornerRadius::same(2), fill, Stroke::new(1.0_f32, edge), StrokeKind::Inside);
}

/// The small ▾ of a flyout or drop-down.
fn arrow(ui: &egui::Ui, center: Pos2, color: Color32) {
    let points = vec![pos2(center.x - 3.5, center.y - 2.0), pos2(center.x + 3.5, center.y - 2.0), pos2(center.x, center.y + 2.0)];
    ui.painter().add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
}

fn icon_tint(enabled: bool) -> Color32 {
    if enabled { Color32::WHITE } else { theme::TEXT_DIM.gamma_multiply(0.5) }
}

/// Words centered under `top`, one line or two.
fn centered_text(ui: &egui::Ui, text: &str, top: Pos2, color: Color32) -> f32 {
    let mut job = LayoutJob::simple(text.to_string(), FontId::proportional(FONT), color, f32::INFINITY);
    job.halign = Align::Center;
    let galley = ui.fonts(|f| f.layout_job(job));
    let height = galley.size().y;
    ui.painter().galley(top, galley, color);
    height
}

fn large_button(app: &mut LinewrightApp, ui: &mut egui::Ui, button: &Button) {
    let (rect, _) = ui.allocate_exact_size(vec2(large_width(ui, button), CONTENT_HEIGHT), Sense::hover());
    let enabled = ui.is_enabled();
    let tool = button.current(ui);
    let checked = button.checked(app);
    let icon = Rect::from_center_size(pos2(rect.center().x, rect.top() + 3.0 + LARGE_ICON / 2.0), vec2(LARGE_ICON, LARGE_ICON));
    let split = rect.top() + LARGE_ICON + 6.0;
    match button {
        One(tool) => {
            let response = ui.interact(rect, ui.id().with(("large", tool.command, tool.option)), Sense::click());
            background(ui, rect, response.hovered(), checked);
            if response.on_hover_text(tool.tip()).clicked() {
                app.actions.push(tool.action());
            }
        }
        Flyout(..) => {
            // The icon runs the tool used last; the words and ▾ under it open the list.
            let top = Rect::from_min_max(rect.min, pos2(rect.right(), split));
            let bottom = Rect::from_min_max(pos2(rect.left(), split), rect.max);
            let first = ui.interact(top, ui.id().with(("large", tool.command, "run")), Sense::click());
            let list = ui.interact(bottom, ui.id().with(("large", tool.command, "list")), Sense::click());
            let hovered = first.hovered() || list.hovered();
            background(ui, rect, hovered, checked);
            if hovered && !checked {
                let part = if first.hovered() { top } else { bottom };
                ui.painter().rect_filled(part.shrink(1.0), CornerRadius::same(2), theme::RIBBON_HOVER_EDGE.gamma_multiply(0.6));
            }
            if first.on_hover_text(tool.tip()).clicked() {
                app.actions.push(tool.action());
            }
            flyout_list(app, ui, &list, button);
        }
    }
    icons::paint(ui, tool.icon, icon, LARGE_ICON, icon_tint(enabled));
    let color = if enabled { theme::RIBBON_TEXT } else { theme::TEXT_DIM };
    let height = centered_text(ui, large_text(button), pos2(rect.center().x, split + 1.0), color);
    if matches!(button, Flyout(..)) {
        arrow(ui, pos2(rect.center().x, split + height + 6.0), color);
    }
}

fn small_button(app: &mut LinewrightApp, ui: &mut egui::Ui, button: &Button, labelled: bool, width: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, ROW), Sense::hover());
    let enabled = ui.is_enabled();
    let tool = button.current(ui);
    let checked = button.checked(app);
    let flyout = matches!(button, Flyout(..));
    let main = if flyout { rect.with_max_x(rect.right() - SPLIT) } else { rect };
    let run = ui.interact(main, ui.id().with(("small", tool.command, tool.option, "run")), Sense::click());
    let mut hovered = run.hovered();
    if flyout {
        let side = rect.with_min_x(main.right());
        let list = ui.interact(side, ui.id().with(("small", tool.command, "list")), Sense::click());
        hovered |= list.hovered();
        background(ui, rect, hovered, checked);
        if list.hovered() {
            ui.painter().rect_filled(side.shrink(1.0), CornerRadius::same(2), theme::RIBBON_HOVER_EDGE.gamma_multiply(0.6));
        }
        arrow(ui, side.center(), if enabled { theme::RIBBON_TEXT } else { theme::TEXT_DIM });
        flyout_list(app, ui, &list, button);
    } else {
        background(ui, rect, hovered, checked);
    }
    let icon = if labelled {
        Rect::from_min_size(pos2(main.left() + 4.0, main.center().y - SMALL_ICON / 2.0), vec2(SMALL_ICON, SMALL_ICON))
    } else {
        Rect::from_center_size(main.center(), vec2(SMALL_ICON, SMALL_ICON))
    };
    icons::paint(ui, tool.icon, icon, SMALL_ICON, icon_tint(enabled));
    if labelled {
        let color = if enabled { theme::RIBBON_TEXT } else { theme::TEXT_DIM };
        let galley = ui.fonts(|f| f.layout_no_wrap(tool.text().to_string(), FontId::proportional(FONT), color));
        ui.painter().galley(pos2(icon.right() + 6.0, main.center().y - galley.size().y / 2.0), galley, color);
    }
    if run.on_hover_text(tool.tip()).clicked() {
        app.actions.push(tool.action());
    }
}

/// A row of a drop-down list of tools: the icon and the words.
fn tool_row(ui: &mut egui::Ui, tool: &Tool) -> Response {
    let icon = icons::image(tool.icon, Color32::WHITE).fit_to_exact_size(vec2(SMALL_ICON, SMALL_ICON));
    ui.add(egui::Button::image_and_text(icon, tool.text().replace('\n', " ")).frame(false)).on_hover_text(tool.tip())
}

/// The list a flyout's ▾ opens. The tool picked runs, and the flyout shows it from then on.
fn flyout_list(app: &mut LinewrightApp, ui: &mut egui::Ui, response: &Response, button: &Button) {
    let id = response.id.with("list");
    if response.clicked() {
        ui.memory_mut(|m| m.toggle_popup(id));
    }
    egui::popup::popup_below_widget(ui, id, response, egui::popup::PopupCloseBehavior::CloseOnClickOutside, |ui| {
        ui.set_min_width(150.0);
        for (index, tool) in button.tools().iter().enumerate() {
            if tool_row(ui, tool).clicked() {
                ui.data_mut(|d| d.insert_temp(button.memory(), index));
                app.actions.push(tool.action());
                ui.memory_mut(|m| m.close_popup());
            }
        }
    });
}

/// The strip at the bottom of a panel: its name, and a ▾ that lists the tools the panel has no room for.
fn title_strip(app: &mut LinewrightApp, ui: &mut egui::Ui, rect: Rect, title: &str, hidden: &[Tool]) {
    let expands = !hidden.is_empty();
    let response = ui.interact(rect, Id::new(("ribbon_title", title)), if expands { Sense::click() } else { Sense::hover() });
    let id = response.id.with("list");
    if response.clicked() {
        ui.memory_mut(|m| m.toggle_popup(id));
    }
    let open = ui.memory(|m| m.is_popup_open(id));
    let fill = if expands && (response.hovered() || open) { theme::RIBBON_HOVER } else { theme::RIBBON_TITLE };
    ui.painter().rect_filled(rect, 0.0, fill);
    let color = if expands && response.hovered() { Color32::WHITE } else { theme::RIBBON_TITLE_TEXT };
    let galley = ui.fonts(|f| f.layout_no_wrap(title.to_string(), title_font(), color));
    let width = galley.size().x + if expands { 12.0 } else { 0.0 };
    let start = pos2(rect.center().x - width / 2.0, rect.center().y - galley.size().y / 2.0);
    let right = start.x + galley.size().x;
    ui.painter().galley(start, galley, color);
    if expands {
        arrow(ui, pos2(right + 8.0, rect.center().y), color);
    }
    let response = if expands { response.on_hover_text(format!("More {} tools", title.to_lowercase())) } else { response };
    egui::popup::popup_below_widget(ui, id, &response, egui::popup::PopupCloseBehavior::CloseOnClickOutside, |ui| {
        ui.set_min_width(180.0);
        for tool in hidden {
            if tool_row(ui, tool).clicked() {
                app.actions.push(tool.action());
                ui.memory_mut(|m| m.close_popup());
            }
        }
    });
}

// ------------------------------------------------------------------------------------------------ layers

/// The layer tools under the layer drop-down, as AutoCAD's two rows.
const LAYER_TOOLS: [[Tool; 5]; 2] = [
    [
        named("LAYOFF", "Off"),
        named("LAYISO", "Isolate"),
        named("LAYFRZ", "Freeze"),
        named("LAYLCK", "Lock"),
        named("LAYMCH", "Match Layer"),
    ],
    [
        named("LAYON", "Turn All Layers On"),
        named("LAYUNISO", "Unisolate"),
        named("LAYTHW", "Thaw All Layers"),
        named("LAYULK", "Unlock"),
        named("LAYMCUR", "Make Current"),
    ],
];

fn layers(app: &mut LinewrightApp, ui: &mut egui::Ui) {
    ui.vertical(|ui| {
        ui.set_width(LAYER_FIELD);
        layer_field(app, ui);
        for row in &LAYER_TOOLS {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                ui.add_space(6.0);
                for tool in row {
                    small_button(app, ui, &One(*tool), false, LAYER_TOOL);
                }
            });
        }
    });
}

/// Where a layer's state icons, color and name go in a row of the layer list.
struct LayerRow {
    on: Rect,
    frozen: Rect,
    locked: Rect,
    swatch: Rect,
    name: Pos2,
}

impl LayerRow {
    fn at(rect: Rect) -> LayerRow {
        let icon = |index: f32| Rect::from_min_size(pos2(rect.left() + 4.0 + index * 20.0, rect.center().y - 9.0), vec2(18.0, 18.0));
        LayerRow {
            on: icon(0.0),
            frozen: icon(1.0),
            locked: icon(2.0),
            swatch: Rect::from_center_size(icon(3.0).center(), vec2(11.0, 11.0)),
            name: pos2(icon(3.0).right() + 6.0, rect.center().y),
        }
    }
}

/// A layer's state icons, its color and its name in a row of the layer list.
fn paint_layer(ui: &egui::Ui, row: &LayerRow, layer: &crate::model::document::Layer, current: bool) {
    icons::paint(ui, if layer.visible { "LAYER_ON" } else { "LAYER_OFF" }, row.on, SMALL_ICON, Color32::WHITE);
    icons::paint(ui, if layer.frozen { "LAYER_FROZEN" } else { "LAYER_THAWED" }, row.frozen, SMALL_ICON, Color32::WHITE);
    icons::paint(ui, if layer.locked { "LAYER_LOCKED" } else { "LAYER_UNLOCKED" }, row.locked, SMALL_ICON, Color32::WHITE);
    let (r, g, b) = color_rgb(layer.color).unwrap_or((255, 255, 255));
    ui.painter().rect(row.swatch, 0.0, Color32::from_rgb(r, g, b), Stroke::new(1.0_f32, theme::RIBBON_EDGE), StrokeKind::Outside);
    let font = FontId::proportional(13.0);
    let color = if current { Color32::WHITE } else { theme::RIBBON_TEXT };
    ui.painter().text(row.name, egui::Align2::LEFT_CENTER, &layer.name, font, color);
}

/// The layer drop-down: the current layer, or the selected objects' layer. Its list sets the layer and turns
/// layers on or off, freezes or thaws and locks or unlocks them from their icons.
fn layer_field(app: &mut LinewrightApp, ui: &mut egui::Ui) {
    let values = GeneralValues::of(app);
    let document = app.controller.document();
    let layers = document.layers.clone();
    let current = document.current_layer.clone();
    let (rect, response) = ui.allocate_exact_size(vec2(LAYER_FIELD, ROW), Sense::click());
    let id = response.id.with("list");
    let open = ui.memory(|m| m.is_popup_open(id));
    let edge = if response.hovered() || open { theme::RIBBON_CHECKED_EDGE } else { theme::RIBBON_FIELD_EDGE };
    ui.painter().rect(rect, CornerRadius::same(2), theme::RIBBON_FIELD, Stroke::new(1.0_f32, edge), StrokeKind::Inside);
    arrow(ui, pos2(rect.right() - 10.0, rect.center().y), theme::RIBBON_TEXT);
    match values.layer.as_ref().and_then(|name| layers.iter().find(|l| &l.name == name)) {
        Some(layer) => paint_layer(ui, &LayerRow::at(rect), layer, false),
        None => {
            ui.painter().text(
                rect.left_center() + vec2(8.0, 0.0),
                egui::Align2::LEFT_CENTER,
                "*VARIES*",
                FontId::proportional(13.0),
                theme::RIBBON_TEXT,
            );
        }
    }
    let response = response.on_hover_text("Layer");
    if response.clicked() {
        ui.memory_mut(|m| m.toggle_popup(id));
    }
    let mut actions = vec![];
    egui::popup::popup_below_widget(ui, id, &response, egui::popup::PopupCloseBehavior::CloseOnClickOutside, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
            for layer in &layers {
                let (row, picked) = ui.allocate_exact_size(vec2(LAYER_FIELD - 12.0, ROW), Sense::click());
                let place = LayerRow::at(row);
                let toggle = |ui: &egui::Ui, rect: Rect, what: &str| {
                    ui.interact(rect, Id::new(("ribbon_layer", &layer.name, what)), Sense::click()).on_hover_text(what.to_string())
                };
                let on = toggle(ui, place.on, "On / Off");
                let frozen = toggle(ui, place.frozen, "Freeze / Thaw");
                let locked = toggle(ui, place.locked, "Lock / Unlock");
                if picked.hovered() || on.hovered() || frozen.hovered() || locked.hovered() {
                    ui.painter().rect_filled(row, 0.0, theme::RIBBON_HOVER);
                }
                for (response, rect) in [(&on, place.on), (&frozen, place.frozen), (&locked, place.locked)] {
                    if response.hovered() {
                        ui.painter().rect_stroke(
                            rect,
                            CornerRadius::same(2),
                            Stroke::new(1.0_f32, theme::RIBBON_CHECKED_EDGE),
                            StrokeKind::Inside,
                        );
                    }
                }
                paint_layer(ui, &place, layer, layer.name == current);
                if on.clicked() {
                    actions.push(Action::LayerVisible(layer.name.clone(), !layer.visible));
                } else if frozen.clicked() {
                    actions.push(Action::LayerFrozen(layer.name.clone(), !layer.frozen));
                } else if locked.clicked() {
                    actions.push(Action::LayerLocked(layer.name.clone(), !layer.locked));
                } else if picked.clicked() {
                    actions.push(Action::UseLayer(layer.name.clone()));
                    ui.memory_mut(|m| m.close_popup());
                }
            }
        });
    });
    app.actions.extend(actions);
}

// ------------------------------------------------------------------------------------------------ properties

/// Color, lineweight and linetype, each with its icon before it, as AutoCAD's Properties panel.
fn properties(app: &mut LinewrightApp, ui: &mut egui::Ui) {
    let values = GeneralValues::of(app);
    let mut actions = vec![];
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, ROW_GAP);
        ui.spacing_mut().interact_size.y = ROW;
        ui.spacing_mut().button_padding = vec2(5.0, 1.0);
        let widgets = &mut ui.visuals_mut().widgets;
        for state in [&mut widgets.inactive, &mut widgets.hovered, &mut widgets.active, &mut widgets.open] {
            state.weak_bg_fill = theme::RIBBON_FIELD;
            state.bg_fill = theme::RIBBON_FIELD;
            state.corner_radius = CornerRadius::same(2);
        }
        widgets.inactive.bg_stroke = Stroke::new(1.0_f32, theme::RIBBON_FIELD_EDGE);
        widgets.hovered.bg_stroke = Stroke::new(1.0_f32, theme::RIBBON_CHECKED_EDGE);
        widgets.open.bg_stroke = Stroke::new(1.0_f32, theme::RIBBON_CHECKED_EDGE);
        let row = |ui: &mut egui::Ui, icon: &str, add: &mut dyn FnMut(&mut egui::Ui)| {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(SMALL_ICON, ROW), Sense::hover());
                icons::paint(ui, icon, rect, SMALL_ICON, Color32::WHITE);
                add(ui);
            });
        };
        row(ui, "COLOR_WHEEL", &mut |ui| color_combo(ui, "ribbon_color", PROPERTY_FIELD, values.color, &mut actions));
        row(ui, "LINEWEIGHT", &mut |ui| {
            let choices = lineweight::choices();
            lineweight_combo(ui, "ribbon_lineweight", PROPERTY_FIELD, values.lineweight, choices, Action::UseLineweight, true, &mut actions)
        });
        row(ui, "LINETYPE", &mut |ui| linetype_combo(ui, "ribbon_linetype", PROPERTY_FIELD, &values.linetype, &mut actions));
    });
    app.actions.extend(actions);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size(columns: usize) -> GroupSize {
        GroupSize { large_width: 0.0, large_count: 0, title_width: 0.0, column_widths: vec![100.0; columns] }
    }

    #[test]
    fn a_narrow_ribbon_shares_its_columns_between_the_panels() {
        let sizes = [size(4), size(7), size(3)];
        // Room for six columns: the first panel no longer takes them all.
        assert_eq!(shared_columns(&sizes, 6.0 * 100.0, 0.0), vec![2, 2, 2]);
        // Room for everything shows everything.
        assert_eq!(shared_columns(&sizes, 1400.0, 0.0), vec![4, 7, 3]);
        // No room at all leaves only what the title strips list.
        assert_eq!(shared_columns(&sizes, 0.0, 0.0), vec![0, 0, 0]);
    }

    #[test]
    fn ribbon_tools_run_their_commands_with_options() {
        assert!(matches!(with("CIRCLE", "2P", "CIRCLE", "2-Point").action(), Action::CommandWith("CIRCLE", "2P")));
        assert!(matches!(tool("MOVE").action(), Action::Command("MOVE")));
        // A window command goes the way typing it does.
        assert!(matches!(tool("PLOT").action(), Action::ExportPdf));
        // An alias for another command runs that command.
        assert!(matches!(tool("ATTEDIT").action(), Action::Command("TEXTEDIT")));
    }

    #[test]
    fn every_ribbon_command_is_known_or_has_a_stand_in() {
        // Commands that other work adds; until then the ribbon does what comes closest or says they are unknown.
        const COMING: &[&str] = &["LAYMCH", "QUICKCALC"];
        let panels = TABS.iter().flat_map(|(_, panels)| panels.iter());
        for panel in panels {
            let parts = panel.parts.iter().flat_map(|part| match part {
                Part::Large(button) => button.tools().to_vec(),
                Part::Small(buttons) | Part::Labelled(buttons) => buttons.iter().flat_map(|b| b.tools().to_vec()).collect(),
                Part::Layers => LAYER_TOOLS.iter().flatten().copied().collect(),
                Part::Properties => vec![],
            });
            for tool in parts.chain(panel.more.iter().copied()) {
                let unknown = matches!(tool.action(), Action::Input(_));
                assert!(!unknown || COMING.contains(&tool.command), "{} has no command", tool.command);
            }
        }
    }
}
