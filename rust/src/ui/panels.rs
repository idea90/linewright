//! The parts of the window. Each draws itself from the app's state and asks for changes through `app.actions`.

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, CursorIcon, FontId, Frame, Id, Key, Layout, Margin, Modifiers, Pos2, Rect, RichText, Sense,
    Stroke, StrokeKind, TextEdit, TextStyle, WidgetText, pos2, text::LayoutJob, vec2,
};

use super::app::{
    Action, COMMAND_INPUT, ColorForm, ColorTarget, ContextMenu, Dialog, LinewrightApp, MenuKind, QUICK_SELECT_PROPERTIES, RENAME_INPUT,
    StyleForm, UNIT_CHOICES, UNTITLED, WINDOW_NAME, quick_select_value,
};
use super::controller::{IDLE_HINT, IDLE_PROMPT};
use super::{frame, icons, theme};
use crate::canvas::aci::{BY_BLOCK, BY_LAYER, NAMED_COLORS, color_name, color_rgb, is_true_color, parse_color, true_color};
use crate::canvas::plot::{FIT, PAPER_SIZES};
use crate::canvas::render::{self, Overlay, Screen};
use crate::commands::parse::{dynamic_entry, parse_length};
use crate::commands::prompts::PromptKind;
use crate::commands::registry::{Suggestion, label, resolve, shortest_alias, suggestions};
use crate::io::dxf::unit_names;
use crate::model::dimension::{Arrowhead, STANDARD_STYLE, Tolerance};
use crate::model::entities::{EntityId, Shape};
use crate::model::lineweight;
use crate::model::mline::{MlineElement, MlineStyle};
use crate::model::multileader::MleaderStyle;
use crate::model::overkill::OverkillSettings;
use crate::model::properties::shared_properties;
use crate::model::styles::{BYLAYER_LINETYPE, LINETYPES};
use crate::snap::osnap::SnapKind;
use crate::snap::resolve::SnapMode;

const HEADER_HEIGHT: f32 = 40.0;
const QUICK_ICON: f32 = 18.0;
const LARGE_ICON: f32 = 28.0;
const SMALL_ICON: f32 = 20.0;
const VARIES: &str = "*VARIES*";
const HISTORY_LINES: usize = 3;
const COORDS_WIDTH: f32 = 170.0;
const SCALES: [&str; 11] = [FIT, "1:1", "1:2", "1:5", "1:10", "1:20", "1:50", "1:100", "1:200", "1:500", "2:1"];

/// Commands behind a large button: right-click it to see them.
const MORE: &[(&str, &[&str])] = &[
    ("DIMLINEAR", &["DIMALIGNED", "DIMCONTINUE", "DIMBASELINE", "QDIM", "DIMORDINATE", "DIMARC", "DIMJOGGED", "DIMTEDIT", "DIMSCALE"]),
    ("MTEXT", &["TEXT", "MLEADER", "MLEADERADD", "MLEADERREMOVE"]),
];

const TOGGLES: [(&str, SnapMode, &str, &str); 7] = [
    ("Grid", SnapMode::ShowGrid, "F7", "GRID"),
    ("Snap", SnapMode::Grid, "F9", "SNAP"),
    ("Ortho", SnapMode::Ortho, "F8", "ORTHO"),
    ("Polar", SnapMode::Polar, "F10", "POLAR"),
    ("Object snap", SnapMode::Osnap, "F3", "OSNAP"),
    ("Tracking", SnapMode::Tracking, "F11", "OTRACK"),
    ("Dynamic input", SnapMode::Dynamic, "F12", "DYN"),
];

fn tool_tip(name: &str) -> String {
    match shortest_alias(name) {
        "" => format!("{} ({name})", label(name)),
        alias => format!("{} ({alias})", label(name)),
    }
}

fn icon_tint(hovered: bool, checked: bool, enabled: bool) -> Color32 {
    if !enabled {
        theme::TEXT_DIM.gamma_multiply(0.5)
    } else if checked {
        theme::ACCENT
    } else if hovered {
        Color32::WHITE
    } else {
        theme::TEXT
    }
}

fn button_background(ui: &egui::Ui, rect: egui::Rect, hovered: bool, checked: bool) {
    if checked {
        ui.painter().rect(rect, CornerRadius::same(6), theme::ACCENT_TINT, Stroke::new(1.0_f32, theme::ACCENT_EDGE), StrokeKind::Inside);
    } else if hovered {
        ui.painter().rect_filled(rect, CornerRadius::same(6), theme::PANEL_RAISED);
    }
}

/// A square button showing only an icon.
fn icon_button(ui: &mut egui::Ui, icon: &str, size: egui::Vec2, icon_size: f32, enabled: bool, checked: bool) -> egui::Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    let hovered = enabled && response.hovered();
    button_background(ui, rect, hovered, checked);
    icons::paint(ui, icon, rect, icon_size, icon_tint(hovered, checked, enabled));
    response
}

/// A big ribbon button: the icon with its label underneath.
fn large_button(ui: &mut egui::Ui, name: &str, checked: bool) -> egui::Response {
    // AutoCAD calls its multiline text button just Text.
    let text = if name == "MTEXT" { "Text" } else { label(name) };
    let galley = ui.fonts(|f| f.layout_no_wrap(text.to_string(), FontId::proportional(12.0), theme::TEXT));
    let size = vec2((galley.size().x + 14.0).max(56.0), 64.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hovered = response.hovered();
    button_background(ui, rect, hovered, checked);
    let icon_rect = egui::Rect::from_center_size(pos2(rect.center().x, rect.top() + 22.0), vec2(LARGE_ICON, LARGE_ICON));
    icons::paint(ui, name, icon_rect, LARGE_ICON, icon_tint(hovered, checked, true));
    ui.painter().galley(pos2(rect.center().x - galley.size().x / 2.0, rect.bottom() - galley.size().y - 6.0), galley, theme::TEXT);
    response
}

fn swatch_text(color: Option<i32>, text: &str) -> WidgetText {
    let mut job = LayoutJob::default();
    let font = FontId::proportional(13.5);
    match color.and_then(color_rgb) {
        Some((r, g, b)) => {
            job.append("■  ", 0.0, egui::TextFormat::simple(font.clone(), Color32::from_rgb(r, g, b)));
        }
        None if color.is_some() => job.append("□  ", 0.0, egui::TextFormat::simple(font.clone(), theme::TEXT_DIM)),
        None => {}
    }
    job.append(text, 0.0, egui::TextFormat::simple(font, theme::TEXT));
    job.into()
}

/// The one value they all share, or None when they differ or there are none.
fn common<T: PartialEq + Clone>(values: impl IntoIterator<Item = T>) -> Option<T> {
    let mut values = values.into_iter();
    let first = values.next()?;
    values.all(|v| v == first).then_some(first)
}

fn divider(ui: &mut egui::Ui, height: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(1.0, height), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, theme::BORDER_STRONG);
}

// ------------------------------------------------------------------------------------------------ header

pub fn header(app: &mut LinewrightApp, ctx: &egui::Context) {
    let frame = Frame::new()
        .fill(theme::PANEL_DARK)
        .inner_margin(Margin { left: 8, right: 0, top: 0, bottom: 0 })
        .stroke(Stroke::new(1.0_f32, theme::BORDER));
    egui::TopBottomPanel::top("header").exact_height(HEADER_HEIGHT).frame(frame).show(ctx, |ui| {
        // The header stands in for the system title bar: it moves the window and toggles maximize. It is laid
        // down first, so the buttons on top of it keep their clicks.
        let bar = ui.interact(ui.max_rect(), Id::new("title_bar"), Sense::click_and_drag());
        if bar.double_clicked() {
            frame::toggle_maximized(ctx);
        } else if bar.drag_started_by(egui::PointerButton::Primary) {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        ui.horizontal_centered(|ui| {
            let menu_icon = icons::image("MENU", theme::ACCENT).fit_to_exact_size(vec2(QUICK_ICON, QUICK_ICON));
            ui.menu_image_text_button(menu_icon, RichText::new(WINDOW_NAME).color(theme::ACCENT).strong().size(15.0), |ui| {
                ui.set_min_width(220.0);
                for (text, shortcut, action) in [
                    ("New", "Ctrl+N", Action::New),
                    ("Open...", "Ctrl+O", Action::Open),
                    ("Save", "Ctrl+S", Action::Save),
                    ("Save As...", "Ctrl+Shift+S", Action::SaveAs),
                    ("Export PDF...", "Ctrl+P", Action::ExportPdf),
                ] {
                    if ui.add(egui::Button::new(text).shortcut_text(shortcut)).clicked() {
                        app.actions.push(action);
                        ui.close_menu();
                    }
                }
            });
            divider(ui, 18.0);
            let can_undo = app.controller.drawing.undo.can_undo();
            let can_redo = app.controller.drawing.undo.can_redo();
            let quick = [
                Some(("NEW", "New (Ctrl+N)", Action::New, true)),
                Some(("OPEN", "Open (Ctrl+O)", Action::Open, true)),
                Some(("SAVE", "Save (Ctrl+S)", Action::Save, true)),
                Some(("PLOT", "Export PDF (Ctrl+P)", Action::ExportPdf, true)),
                None,
                Some(("UNDO", "Undo (Ctrl+Z)", Action::Undo, can_undo)),
                Some(("REDO", "Redo (Ctrl+Y)", Action::Redo, can_redo)),
            ];
            for item in quick {
                match item {
                    None => divider(ui, 18.0),
                    Some((icon, tip, action, enabled)) => {
                        let enabled = enabled && !app.is_opening();
                        if icon_button(ui, icon, vec2(30.0, 30.0), QUICK_ICON, enabled, false).on_hover_text(tip).clicked() {
                            app.actions.push(action);
                        }
                    }
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                if window_button(ui, "WIN_CLOSE", "Close", true).clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                let maximized = frame::is_maximized(ctx);
                let (icon, tip) = if maximized { ("WIN_RESTORE", "Restore Down") } else { ("WIN_MAXIMIZE", "Maximize") };
                if window_button(ui, icon, tip, false).clicked() {
                    frame::toggle_maximized(ctx);
                }
                if window_button(ui, "WIN_MINIMIZE", "Minimize", false).clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                }
                let title = format!("{}{}", app.file_name(), if app.modified() { "*" } else { "" });
                ui.with_layout(Layout::centered_and_justified(egui::Direction::LeftToRight), |ui| {
                    let mut job = LayoutJob::default();
                    job.append(&title, 0.0, egui::TextFormat::simple(FontId::proportional(13.5), theme::TEXT));
                    job.append(&format!("  · {WINDOW_NAME}"), 0.0, egui::TextFormat::simple(FontId::proportional(13.5), theme::TEXT_DIM));
                    // The title is part of the bar: dragging or double-clicking it acts on the window.
                    ui.add(egui::Label::new(job).selectable(false).sense(Sense::hover()));
                });
            });
        });
    });
}

/// Minimize, maximize and close, at the right end of the header. Close turns red under the pointer.
fn window_button(ui: &mut egui::Ui, icon: &str, tip: &str, close: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(46.0, HEADER_HEIGHT), Sense::click());
    let hovered = response.hovered();
    if hovered {
        ui.painter().rect_filled(rect, 0.0, if close { theme::CLOSE_HOVER } else { theme::PANEL_RAISED });
    }
    icons::paint(ui, icon, rect, 16.0, if hovered { Color32::WHITE } else { theme::TEXT });
    response.on_hover_text(tip)
}

// ------------------------------------------------------------------------------------------------ ribbon

fn group(ui: &mut egui::Ui, title: &str, content: impl FnOnce(&mut egui::Ui)) {
    ui.vertical(|ui| {
        ui.set_min_height(86.0);
        let width = ui
            .horizontal(|ui| {
                ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
                ui.set_min_height(66.0);
                content(ui);
            })
            .response
            .rect
            .width();
        ui.allocate_ui_with_layout(vec2(width, 16.0), Layout::top_down(Align::Center), |ui| {
            ui.label(RichText::new(title).color(theme::TEXT_DIM).size(11.5));
        });
    });
}

fn large_buttons(app: &mut LinewrightApp, ui: &mut egui::Ui, names: &[&'static str]) {
    let running = app.controller.running_command();
    for name in names {
        let more = MORE.iter().find(|(command, _)| command == name).map(|(_, more)| more);
        let mut tip = tool_tip(name);
        if let Some(more) = more {
            tip = format!("{tip}. Right-click for {}.", more.iter().map(|m| label(m)).collect::<Vec<_>>().join(" and "));
        }
        let checked = running == Some(*name) || more.is_some_and(|more| more.iter().any(|m| running == Some(*m)));
        let response = large_button(ui, name, checked).on_hover_text(tip);
        if response.clicked() {
            app.actions.push(Action::Command(name));
        }
        if let Some(more) = more {
            response.context_menu(|ui| {
                for extra in more.iter() {
                    if ui.button(label(extra)).clicked() {
                        app.actions.push(Action::Command(extra));
                        ui.close_menu();
                    }
                }
            });
        }
    }
}

fn small_buttons(app: &mut LinewrightApp, ui: &mut egui::Ui, names: &[&'static str], rows: usize) {
    let running = app.controller.running_command();
    for column in names.chunks(rows) {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
            for name in column {
                // The group selection button shows whether it is on, as AutoCAD's does.
                let checked = if *name == "PICKSTYLE" { app.controller.document().group_selection } else { running == Some(*name) };
                let response = icon_button(ui, name, vec2(32.0, 31.0), SMALL_ICON, true, checked).on_hover_text(tool_tip(name));
                if response.clicked() {
                    app.actions.push(Action::Command(name));
                }
            }
        });
    }
}

/// What the selection has in common, or what new objects get when nothing is selected: layer, color, linetype
/// and lineweight. None where selected objects differ.
struct GeneralValues {
    layer: Option<String>,
    color: Option<i32>,
    linetype: Option<String>,
    lineweight: Option<i16>,
    layers: Vec<(String, i32)>,
}

impl GeneralValues {
    fn of(app: &LinewrightApp) -> GeneralValues {
        let document = app.controller.document();
        let selected = app.controller.selected();
        let any = !selected.is_empty();
        GeneralValues {
            layer: if any { common(selected.iter().map(|e| e.layer.clone())) } else { Some(document.current_layer.clone()) },
            color: if any { common(selected.iter().map(|e| e.color)) } else { Some(document.current_color) },
            linetype: if any { common(selected.iter().map(|e| e.linetype.clone())) } else { Some(document.current_linetype.clone()) },
            lineweight: if any { common(selected.iter().map(|e| e.lineweight)) } else { Some(document.current_lineweight) },
            layers: document.layers.iter().map(|l| (l.name.clone(), l.color)).collect(),
        }
    }
}

fn layer_combo(ui: &mut egui::Ui, id: &str, width: f32, values: &GeneralValues, actions: &mut Vec<Action>) {
    let layer_color = values.layer.as_ref().and_then(|name| values.layers.iter().find(|(n, _)| n == name)).map(|(_, c)| *c);
    egui::ComboBox::from_id_salt(id)
        .width(width)
        .selected_text(match &values.layer {
            Some(name) => swatch_text(layer_color, name),
            None => VARIES.into(),
        })
        .show_ui(ui, |ui| {
            for (name, color) in &values.layers {
                if ui.selectable_label(values.layer.as_ref() == Some(name), swatch_text(Some(*color), name)).clicked() {
                    actions.push(Action::UseLayer(name.clone()));
                }
            }
        })
        .response
        .on_hover_text("Layer");
}

fn color_combo(ui: &mut egui::Ui, id: &str, width: f32, color: Option<i32>, actions: &mut Vec<Action>) {
    egui::ComboBox::from_id_salt(id)
        .width(width)
        .selected_text(match color {
            Some(color) => swatch_text(Some(color), &color_name(color)),
            None => VARIES.into(),
        })
        .show_ui(ui, |ui| {
            let standard = [BY_LAYER, BY_BLOCK].into_iter().chain(NAMED_COLORS.iter().take(7).map(|(c, _)| *c));
            // The color in use stays on the list even when it is not a standard one.
            let shown: Vec<i32> = standard.chain(color.filter(|c| ![BY_LAYER, BY_BLOCK].contains(c) && !(1..=7).contains(c))).collect();
            for choice in shown {
                if ui.selectable_label(color == Some(choice), swatch_text(Some(choice), &color_name(choice))).clicked() {
                    actions.push(Action::UseColor(choice));
                }
            }
            ui.separator();
            if ui.button("More colors…").clicked() {
                actions.push(Action::ChooseColor(ColorTarget::Selection));
            }
        })
        .response
        .on_hover_text("Color");
}

fn linetype_combo(ui: &mut egui::Ui, id: &str, width: f32, linetype: &Option<String>, actions: &mut Vec<Action>) {
    egui::ComboBox::from_id_salt(id)
        .width(width)
        .selected_text(linetype.clone().unwrap_or_else(|| VARIES.into()))
        .show_ui(ui, |ui| {
            for name in std::iter::once(BYLAYER_LINETYPE).chain(LINETYPES) {
                if ui.selectable_label(linetype.as_deref() == Some(name), name).clicked() {
                    actions.push(Action::UseLinetype(name.to_string()));
                }
            }
        })
        .response
        .on_hover_text("Linetype");
}

/// A multiline style's settings as AutoCAD's New/Modify Multiline Style dialog lays them out: description, caps at
/// each end, fill, and the lines with their offsets, colors and linetypes.
fn mline_style_fields(ui: &mut egui::Ui, style: &mut MlineStyle, element: &mut usize) {
    ui.horizontal(|ui| {
        ui.label("Description");
        ui.add(TextEdit::singleline(&mut style.description).desired_width(300.0));
    });
    ui.add_space(6.0);
    ui.horizontal_top(|ui| {
        egui::Grid::new("mline_caps").num_columns(3).spacing(vec2(12.0, 4.0)).show(ui, |ui| {
            ui.label(RichText::new("Caps").color(theme::TEXT_DIM));
            ui.label("Start");
            ui.label("End");
            ui.end_row();
            for (name, start, end) in [
                ("Line", &mut style.start_cap, &mut style.end_cap),
                ("Outer arc", &mut style.start_round, &mut style.end_round),
                ("Inner arcs", &mut style.start_inner_arcs, &mut style.end_inner_arcs),
            ] {
                ui.label(name);
                ui.checkbox(start, "");
                ui.checkbox(end, "");
                ui.end_row();
            }
            ui.label("Display joints");
            ui.checkbox(&mut style.joints, "");
            ui.end_row();
        });
        ui.add_space(16.0);
        ui.vertical(|ui| {
            let mut filled = style.fill.is_some();
            if ui.checkbox(&mut filled, "Fill").changed() {
                style.fill = filled.then_some(BY_LAYER);
            }
            if let Some(fill) = &mut style.fill {
                mline_color_combo(ui, "mline_fill_color", fill);
            }
        });
    });
    ui.add_space(8.0);
    ui.label(RichText::new("Elements").color(theme::TEXT_DIM));
    egui::Grid::new("mline_elements").num_columns(3).striped(true).spacing(vec2(12.0, 2.0)).show(ui, |ui| {
        ui.label("Offset");
        ui.label("Color");
        ui.label("Linetype");
        ui.end_row();
        for (index, line) in style.elements.iter().enumerate() {
            let picked = *element == index;
            if ui.selectable_label(picked, crate::commands::format_general(line.offset)).clicked() {
                *element = index;
            }
            ui.label(color_name(line.color));
            ui.label(&line.linetype);
            ui.end_row();
        }
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if ui.button("Add").clicked() {
            style.elements.push(MlineElement { offset: 0.0, color: BY_LAYER, linetype: BYLAYER_LINETYPE.to_string() });
            *element = style.elements.len() - 1;
        }
        if ui.add_enabled(style.elements.len() > 1, egui::Button::new("Delete")).clicked() {
            style.elements.remove(*element);
            *element = element.saturating_sub(1);
        }
    });
    if let Some(line) = style.elements.get_mut(*element) {
        ui.horizontal(|ui| {
            ui.label("Offset");
            ui.add(egui::DragValue::new(&mut line.offset).speed(0.05).max_decimals(6));
            ui.label("Color");
            mline_color_combo(ui, "mline_element_color", &mut line.color);
            ui.label("Linetype");
            egui::ComboBox::from_id_salt("mline_element_linetype").width(110.0).selected_text(line.linetype.clone()).show_ui(ui, |ui| {
                for name in std::iter::once(BYLAYER_LINETYPE).chain(["ByBlock"]).chain(LINETYPES) {
                    ui.selectable_value(&mut line.linetype, name.to_string(), name);
                }
            });
        });
    }
    // The lines are listed from the top one down, as AutoCAD lists them, and the picked one stays picked.
    let mut order: Vec<usize> = (0..style.elements.len()).collect();
    order.sort_by(|&a, &b| style.elements[b].offset.total_cmp(&style.elements[a].offset));
    if order.iter().enumerate().any(|(at, &i)| at != i) {
        *element = order.iter().position(|&i| i == *element).unwrap_or(0);
        style.elements = order.iter().map(|&i| style.elements[i].clone()).collect();
    }
}

fn mline_color_combo(ui: &mut egui::Ui, id: &str, color: &mut i32) {
    egui::ComboBox::from_id_salt(id).width(110.0).selected_text(swatch_text(Some(*color), &color_name(*color))).show_ui(ui, |ui| {
        for choice in [BY_LAYER, BY_BLOCK].into_iter().chain(NAMED_COLORS.iter().take(7).map(|(c, _)| *c)) {
            ui.selectable_value(color, choice, swatch_text(Some(choice), &color_name(choice)));
        }
    });
}

/// A lineweight choice as AutoCAD lists it: a sample of the line, then its name.
fn lineweight_choice(ui: &mut egui::Ui, value: i16, selected: bool) -> egui::Response {
    const SAMPLE: f32 = 40.0;
    let galley = ui.fonts(|f| f.layout_no_wrap(lineweight::name(value), FontId::proportional(13.5), theme::TEXT));
    let width = (SAMPLE + 22.0 + galley.size().x).max(ui.available_width());
    let (rect, response) = ui.allocate_exact_size(vec2(width, 22.0), Sense::click());
    if selected || response.hovered() {
        let fill = if selected { theme::ACCENT_TINT } else { theme::PANEL_RAISED };
        ui.painter().rect_filled(rect, CornerRadius::same(4), fill);
    }
    let thickness = if value >= 0 { lineweight::model_pixels(value as f64 / 100.0).min(7.0) } else { 1.0 };
    let (x, y) = (rect.left() + 6.0, rect.center().y);
    ui.painter().line_segment([pos2(x, y), pos2(x + SAMPLE, y)], Stroke::new(thickness, theme::TEXT));
    ui.painter().galley(pos2(x + SAMPLE + 10.0, y - galley.size().y / 2.0), galley, theme::TEXT);
    response
}

fn lineweight_combo(
    ui: &mut egui::Ui,
    id: &str,
    width: f32,
    current: Option<i16>,
    choices: impl Iterator<Item = i16>,
    pick: impl Fn(i16) -> Action,
    actions: &mut Vec<Action>,
) {
    egui::ComboBox::from_id_salt(id)
        .width(width)
        .height(420.0)
        .selected_text(current.map_or_else(|| VARIES.into(), lineweight::name))
        .show_ui(ui, |ui| {
            for value in choices {
                if lineweight_choice(ui, value, current == Some(value)).clicked() {
                    actions.push(pick(value));
                }
            }
        })
        .response
        .on_hover_text("Lineweight");
}

fn properties(app: &mut LinewrightApp, ui: &mut egui::Ui) {
    let values = GeneralValues::of(app);
    let mut actions = vec![];
    // Two rows without labels, so the ribbon keeps its tools in view; each control says what it is on hover.
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        ui.horizontal(|ui| {
            layer_combo(ui, "layer_combo", 140.0, &values, &mut actions);
            color_combo(ui, "color_combo", 100.0, values.color, &mut actions);
        });
        ui.horizontal(|ui| {
            if icon_button(ui, "LAYERS", vec2(32.0, 28.0), 18.0, true, app.palette_open).on_hover_text("Layer Properties (LA)").clicked() {
                actions.push(Action::TogglePalette);
            }
            linetype_combo(ui, "linetype_combo", 90.0, &values.linetype, &mut actions);
            lineweight_combo(ui, "lineweight_combo", 90.0, values.lineweight, lineweight::choices(), Action::UseLineweight, &mut actions);
        });
    });
    app.actions.extend(actions);
}

pub fn ribbon(app: &mut LinewrightApp, ctx: &egui::Context) {
    let frame = Frame::new()
        .fill(theme::PANEL)
        .inner_margin(Margin { left: 10, right: 10, top: 8, bottom: 4 })
        .stroke(Stroke::new(1.0_f32, theme::BORDER));
    egui::TopBottomPanel::top("ribbon").frame(frame).show(ctx, |ui| {
        ui.add_enabled_ui(!app.is_opening(), |ui| {
            // Properties keeps its place at the right end; the tools take the room that is left and scroll
            // sideways when the window is too narrow for them all.
            ui.horizontal(|ui| {
                // The Properties group's width as last drawn, so the tools leave room for it.
                let width_id = Id::new("ribbon_properties_width");
                let reserved = ui.data(|d| d.get_temp::<f32>(width_id)).unwrap_or(560.0) + 16.0;
                ui.style_mut().spacing.scroll = egui::style::ScrollStyle::solid();
                egui::ScrollArea::horizontal()
                    .max_width((ui.available_width() - reserved).max(0.0))
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
                    .show(ui, |ui| ui.horizontal(|ui| tool_groups(app, ui)));
                divider(ui, 82.0);
                let start = ui.cursor().left();
                group(ui, "Properties", |ui| properties(app, ui));
                let width = ui.min_rect().right() - start;
                ui.data_mut(|d| d.insert_temp(width_id, width));
            });
        });
    });
}

fn tool_groups(app: &mut LinewrightApp, ui: &mut egui::Ui) {
    group(ui, "Draw", |ui| {
        large_buttons(app, ui, &["LINE", "PLINE"]);
        small_buttons(
            app,
            ui,
            &["CIRCLE", "ARC", "RECTANGLE", "POLYGON", "ELLIPSE", "HATCH", "BOUNDARY", "SPLINE", "POINT", "MLINE", "WIPEOUT"],
            2,
        );
    });
    divider(ui, 82.0);
    group(ui, "Modify", |ui| {
        small_buttons(
            app,
            ui,
            &[
                "MOVE",
                "MIRROR",
                "COPY",
                "OFFSET",
                "SCALE",
                "FILLET",
                "ROTATE",
                "TRIM",
                "ERASE",
                "EXTEND",
                "STRETCH",
                "CHAMFER",
                "ARRAY",
                "BREAK",
                "EXPLODE",
                "JOIN",
                "OVERKILL",
                "MATCHPROP",
                "LIST",
            ],
            2,
        )
    });
    divider(ui, 82.0);
    group(ui, "Annotate and measure", |ui| {
        large_buttons(app, ui, &["MTEXT", "DIMLINEAR"]);
        small_buttons(app, ui, &["QLEADER", "DIMALIGNED", "DIMRADIUS", "DIMDIAMETER", "DIMANGULAR", "DIMSTYLE", "DISTANCE", "AREA"], 2);
    });
    divider(ui, 82.0);
    group(ui, "Blocks & Groups", |ui| small_buttons(app, ui, &["INSERT", "BLOCK", "GROUP", "UNGROUP", "GROUPEDIT", "PICKSTYLE"], 2));
}

// ------------------------------------------------------------------------------------------------ document tab

pub fn document_tab(app: &mut LinewrightApp, ctx: &egui::Context) {
    let frame = Frame::new().fill(theme::PANEL_DARK).inner_margin(Margin { left: 8, right: 8, top: 6, bottom: 0 });
    egui::TopBottomPanel::top("document_tab").frame(frame).show_separator_line(false).show(ctx, |ui| {
        ui.horizontal(|ui| {
            let tab = Frame::new()
                .fill(theme::CANVAS)
                .stroke(Stroke::new(1.0_f32, theme::BORDER))
                .corner_radius(CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 })
                .inner_margin(Margin { left: 14, right: 6, top: 4, bottom: 4 });
            let response = tab.show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!("{}{}", app.file_name(), if app.modified() { "*" } else { "" }));
                    if icon_button(ui, "WIN_CLOSE", vec2(20.0, 20.0), 14.0, !app.is_opening(), false)
                        .on_hover_text("Close drawing")
                        .clicked()
                    {
                        app.actions.push(Action::New);
                    }
                });
            });
            let top = response.response.rect;
            ui.painter()
                .line_segment([top.left_top() + vec2(1.0, 0.0), top.right_top() - vec2(1.0, 0.0)], Stroke::new(2.0_f32, theme::ACCENT));
            if icon_button(ui, "PLUS", vec2(26.0, 26.0), 14.0, !app.is_opening(), false).on_hover_text("New drawing").clicked() {
                app.actions.push(Action::New);
            }
        });
    });
}

// ------------------------------------------------------------------------------------------------ layout tabs

/// Model and the layouts, under the drawing as in AutoCAD. Right-click a layout to rename or delete it; + adds one.
pub fn layout_tabs(app: &mut LinewrightApp, ctx: &egui::Context) {
    if app.welcome {
        return;
    }
    if let Some(name) = app.document().editing_block().map(|b| b.0.name.clone()) {
        // In the block editor the tabs give way to what is being edited and how to finish.
        let frame = Frame::new().fill(theme::PANEL_RAISED).inner_margin(Margin { left: 12, right: 12, top: 4, bottom: 4 });
        egui::TopBottomPanel::bottom("block_editor").frame(frame).show_separator_line(false).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Block Editor: {name}")).strong());
                ui.label(RichText::new("Draw and change the block's objects, then close.").color(theme::TEXT_DIM));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Close Block Editor").clicked() {
                        app.actions.push(Action::Command("BCLOSE"));
                    }
                });
            });
        });
        return;
    }
    let frame = Frame::new().fill(theme::PANEL_DARK).inner_margin(Margin { left: 8, right: 8, top: 2, bottom: 3 });
    egui::TopBottomPanel::bottom("layout_tabs").frame(frame).show_separator_line(false).show(ctx, |ui| {
        ui.add_enabled_ui(!app.is_opening(), |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = vec2(2.0, 0.0);
                let active = app.document().shown_layout();
                let names: Vec<String> = app.document().layouts.iter().map(|l| l.name.clone()).collect();
                let tab = |ui: &mut egui::Ui, label: &str, open: bool| {
                    let text = if open { RichText::new(label).strong() } else { RichText::new(label).color(theme::TEXT_DIM) };
                    ui.selectable_label(open, text)
                };
                if tab(ui, "Model", active.is_none()).clicked() {
                    app.actions.push(Action::OpenSpace(None));
                }
                for (index, name) in names.iter().enumerate() {
                    let response = tab(ui, name, active == Some(index));
                    if response.clicked() {
                        app.actions.push(Action::OpenSpace(Some(index)));
                    }
                    if response.double_clicked() {
                        app.actions.push(Action::RenameLayout(index));
                    }
                    response.context_menu(|ui| {
                        if ui.button("Rename").clicked() {
                            app.actions.push(Action::RenameLayout(index));
                            ui.close_menu();
                        }
                        if ui.add_enabled(names.len() > 1, egui::Button::new("Delete")).clicked() {
                            app.actions.push(Action::DeleteLayout(index));
                            ui.close_menu();
                        }
                        if ui.button("New layout").clicked() {
                            app.actions.push(Action::NewLayout);
                            ui.close_menu();
                        }
                    });
                }
                if icon_button(ui, "PLUS", vec2(22.0, 22.0), 13.0, true, false).on_hover_text("New layout").clicked() {
                    app.actions.push(Action::NewLayout);
                }
            });
        });
    });
}

// ------------------------------------------------------------------------------------------------ status bar

/// A status bar button that is on or off, with its icon and name. True when clicked.
fn status_toggle(ui: &mut egui::Ui, text: &str, icon: &str, on: bool, tip: &str) -> bool {
    let galley = ui.fonts(|f| f.layout_no_wrap(text.to_string(), FontId::proportional(13.0), theme::TEXT));
    let size = vec2(galley.size().x + 40.0, 26.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hovered = response.hovered();
    if on {
        ui.painter().rect_filled(rect, CornerRadius::same(6), theme::ACCENT_TINT);
    } else if hovered {
        ui.painter().rect_filled(rect, CornerRadius::same(6), theme::PANEL_RAISED);
    }
    let color = if on {
        theme::ACCENT
    } else if hovered {
        theme::TEXT
    } else {
        theme::TEXT_DIM
    };
    icons::paint(ui, icon, egui::Rect::from_center_size(pos2(rect.left() + 18.0, rect.center().y), vec2(16.0, 16.0)), 16.0, color);
    ui.painter().galley(pos2(rect.left() + 30.0, rect.center().y - galley.size().y / 2.0), galley, color);
    response.on_hover_text(tip).clicked()
}

pub fn status_bar(app: &mut LinewrightApp, ctx: &egui::Context) {
    let frame = Frame::new()
        .fill(theme::PANEL)
        .inner_margin(Margin { left: 8, right: 8, top: 3, bottom: 3 })
        .stroke(Stroke::new(1.0_f32, theme::BORDER));
    egui::TopBottomPanel::bottom("status_bar").frame(frame).show(ctx, |ui| {
        ui.horizontal(|ui| {
            let point = app.controller.cursor.as_ref().map(|c| c.point).unwrap_or_default();
            let places = app.document().precision;
            let coords = RichText::new(format!("{:.*}, {:.*}", places, point.x, places, point.y)).monospace().strong().color(theme::TEXT);
            ui.add_sized(vec2(COORDS_WIDTH, 22.0), egui::Label::new(coords));
            ui.label(RichText::new(format!("Layer {}", app.document().current_layer)).color(theme::TEXT_DIM));
            ui.add_space(14.0);
            ui.label(RichText::new(format!("Units {}", unit_names(app.document()).1)).color(theme::TEXT_DIM));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if status_toggle(ui, "Lineweight", "LWT", app.document().show_lineweights, "Show lineweights (LWDISPLAY)") {
                    app.actions.push(Action::ToggleLineweights);
                }
                // On a layout, whether model space is worked on through a viewport or the sheet itself.
                if app.document().shown_layout().is_some() {
                    let through = app.document().through().is_some();
                    let (text, tip) = if through {
                        ("Model", "Working in model space through a viewport: click for paper space (PSPACE)")
                    } else {
                        ("Paper", "Working on the sheet: click to work in a viewport (MSPACE)")
                    };
                    if status_toggle(ui, text, "MSPACE", through, tip) {
                        app.actions.push(Action::Command(if through { "PSPACE" } else { "MSPACE" }));
                    }
                }
                for (text, mode, key, icon) in TOGGLES.iter().rev() {
                    if status_toggle(ui, text, icon, app.controller.snap.get(*mode), &format!("{text} ({key})")) {
                        app.actions.push(Action::ToggleSnap(*mode));
                    }
                }
            });
        });
    });
}

// ------------------------------------------------------------------------------------------------ command line

pub fn command_panel(app: &mut LinewrightApp, ctx: &egui::Context) {
    let frame = Frame::new()
        .fill(theme::PANEL)
        .inner_margin(Margin { left: 16, right: 16, top: 8, bottom: 10 })
        .stroke(Stroke::new(1.0_f32, theme::BORDER));
    egui::TopBottomPanel::bottom("command_panel").frame(frame).show(ctx, |ui| {
        let line_height = ui.text_style_height(&TextStyle::Monospace);
        egui::ScrollArea::vertical()
            .id_salt("history")
            .max_height(line_height * HISTORY_LINES as f32 + 6.0)
            .min_scrolled_height(line_height * HISTORY_LINES as f32 + 6.0)
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for line in &app.controller.drawing.log {
                    ui.label(RichText::new(line).monospace().color(theme::TEXT_DIM));
                }
            });
        ui.add_space(6.0);
        let prompt = app.controller.prompt().map_or(IDLE_PROMPT.to_string(), |p| p.message.clone());
        let hint = if app.controller.prompt().is_some() { "" } else { IDLE_HINT };
        Frame::new()
            .fill(theme::PANEL_DARK)
            .stroke(Stroke::new(1.0_f32, theme::BORDER_STRONG))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin { left: 14, right: 10, top: 4, bottom: 4 })
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(!app.is_opening(), |ui| {
                        ui.label(RichText::new(">_").monospace().color(theme::TEXT_DIM));
                        ui.label(RichText::new(prompt).strong().color(theme::TEXT));
                        // AutoComplete, at the Command: prompt only, as in AutoCAD.
                        let offered = if app.controller.prompt().is_none() { suggestions(&app.input, MAX_SUGGESTIONS) } else { vec![] };
                        if app.suggestion_for != app.input {
                            (app.suggestion, app.suggestion_for) = (0, app.input.clone());
                        }
                        let focused = ui.memory(|m| m.has_focus(Id::new(COMMAND_INPUT)));
                        // Dynamic input at a point prompt: Tab moves what is typed into the first field.
                        let point_prompt =
                            app.controller.snap.dynamic && app.controller.prompt().is_some_and(|p| p.kind == PromptKind::Point);
                        if !point_prompt {
                            app.dynamic_first = None;
                        } else if focused && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Tab)) && !app.input.trim().is_empty() {
                            app.dynamic_first = Some(std::mem::take(&mut app.input).trim().to_string());
                        }
                        if focused && !offered.is_empty() {
                            ui.input_mut(|i| {
                                if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                                    app.suggestion = (app.suggestion + 1) % offered.len();
                                }
                                if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                                    app.suggestion = (app.suggestion + offered.len() - 1) % offered.len();
                                }
                                if i.consume_key(Modifiers::NONE, Key::Tab) {
                                    app.input = offered[app.suggestion.min(offered.len() - 1)].command.to_string();
                                    app.suggestion_for = app.input.clone();
                                }
                            });
                        }
                        let response = ui.add(
                            TextEdit::singleline(&mut app.input)
                                .id(Id::new(COMMAND_INPUT))
                                .hint_text(RichText::new(hint).color(theme::TEXT_DIM))
                                .frame(false)
                                .font(TextStyle::Monospace)
                                .desired_width(f32::INFINITY)
                                .margin(vec2(4.0, 6.0)),
                        );
                        let entered = response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                        let picked = suggestion_list(ui, &response, &offered, &mut app.suggestion);
                        if entered || picked.is_some() {
                            let text = std::mem::take(&mut app.input);
                            let word = text.trim();
                            // What was typed runs when it is a command; otherwise the picked suggestion does.
                            let known = resolve(word).is_some() || super::app::file_command(word).is_some();
                            let chosen = picked.or_else(|| {
                                (!known && app.controller.prompt().is_none()).then(|| offered.get(app.suggestion).copied()).flatten()
                            });
                            let mut run = chosen.map_or(word.to_string(), |s| s.command.to_string());
                            if point_prompt {
                                let base = app.controller.prompt().and_then(|p| p.base);
                                let cursor = app.controller.cursor.as_ref().map(|c| c.point);
                                if let Some(entry) = dynamic_entry(&run, app.dynamic_first.as_deref(), base, cursor) {
                                    run = entry;
                                }
                                app.dynamic_first = None;
                            }
                            app.actions.push(Action::Input(run));
                            if picked.is_some() {
                                response.request_focus();
                            }
                        }
                        // Delete with nothing typed erases the selected drawing objects, as in AutoCAD.
                        if response.has_focus() && app.input.is_empty() && ui.input(|i| i.key_pressed(Key::Delete)) {
                            app.actions.push(Action::EraseSelection);
                        }
                    });
                });
            });
    });
}

const MAX_SUGGESTIONS: usize = 8;

/// The AutoComplete list just above the command line: command, what it is, and its short name. Gives the one
/// clicked.
fn suggestion_list(ui: &egui::Ui, input: &egui::Response, offered: &[Suggestion], picked: &mut usize) -> Option<Suggestion> {
    // Still shown the frame the line loses focus, which is the frame a click on the list lands.
    if offered.is_empty() || !(input.has_focus() || input.lost_focus()) {
        return None;
    }
    let row = 26.0;
    let mut clicked = None;
    egui::Area::new(Id::new("autocomplete"))
        .order(egui::Order::Foreground)
        .pivot(Align2::LEFT_BOTTOM)
        .fixed_pos(input.rect.left_top() - vec2(4.0, 10.0))
        .show(ui.ctx(), |ui| {
            Frame::new()
                .fill(theme::PANEL_RAISED)
                .stroke(Stroke::new(1.0_f32, theme::BORDER_STRONG))
                .corner_radius(8)
                .inner_margin(5)
                .show(ui, |ui| {
                    ui.set_width(440.0);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (index, suggestion) in offered.iter().enumerate() {
                        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), row), Sense::click());
                        if response.hovered() {
                            *picked = index;
                        }
                        let painter = ui.painter();
                        if index == *picked {
                            painter.rect_filled(rect, CornerRadius::same(5), theme::BORDER_STRONG);
                        }
                        icons::paint(
                            ui,
                            suggestion.command,
                            Rect::from_center_size(pos2(rect.left() + 14.0, rect.center().y), vec2(16.0, 16.0)),
                            15.0,
                            theme::TEXT_DIM,
                        );
                        let font = FontId::monospace(13.0);
                        painter.text(
                            pos2(rect.left() + 30.0, rect.center().y),
                            Align2::LEFT_CENTER,
                            suggestion.command,
                            font.clone(),
                            theme::TEXT,
                        );
                        painter.text(
                            pos2(rect.left() + 160.0, rect.center().y),
                            Align2::LEFT_CENTER,
                            suggestion.label,
                            FontId::proportional(13.0),
                            theme::TEXT_DIM,
                        );
                        if !suggestion.alias.is_empty() {
                            painter.text(
                                pos2(rect.right() - 8.0, rect.center().y),
                                Align2::RIGHT_CENTER,
                                suggestion.alias,
                                font,
                                theme::TEXT_DIM,
                            );
                        }
                        if response.clicked() {
                            clicked = Some(*suggestion);
                        }
                    }
                });
        });
    clicked
}

// ------------------------------------------------------------------------------------------------ layer palette

pub fn layer_palette(app: &mut LinewrightApp, ctx: &egui::Context) {
    let frame = Frame::new().fill(theme::PANEL).inner_margin(Margin::same(8)).stroke(Stroke::new(1.0_f32, theme::BORDER));
    let mut open = app.palette_open;
    egui::SidePanel::left("layer_palette").frame(frame).default_width(520.0).min_width(440.0).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Layer Properties").strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if icon_button(ui, "WIN_CLOSE", vec2(22.0, 22.0), 14.0, true, false).on_hover_text("Close").clicked() {
                    open = false;
                }
            });
        });
        ui.separator();
        let document = app.controller.document();
        let current = document.current_layer.clone();
        let layers = document.layers.clone();
        // Working through a viewport, layers can be frozen in that viewport alone.
        let viewport_frozen: Option<Vec<String>> = match document.through_viewport().map(|v| &v.shape) {
            Some(crate::model::entities::Shape::Viewport { frozen, .. }) => Some(frozen.clone()),
            _ => None,
        };
        let mut actions = vec![];
        egui::ScrollArea::vertical().max_height(ui.available_height() - 40.0).show(ui, |ui| {
            let columns = 8 + usize::from(viewport_frozen.is_some());
            egui::Grid::new("layers").num_columns(columns).striped(true).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                for heading in ["Name", "On", "Freeze", "Lock", "Plot", "Color", "Linetype", "Lineweight"] {
                    ui.label(RichText::new(heading).color(theme::TEXT_DIM));
                }
                if viewport_frozen.is_some() {
                    ui.label(RichText::new("VP Freeze").color(theme::TEXT_DIM)).on_hover_text("Frozen in the viewport worked through");
                }
                ui.end_row();
                for layer in &layers {
                    let name = layer.name.clone();
                    match &mut app.renaming {
                        Some((old, typed)) if *old == name => {
                            let response = ui.add(TextEdit::singleline(typed).id(Id::new(RENAME_INPUT)).desired_width(150.0));
                            if !response.has_focus() && !response.lost_focus() {
                                response.request_focus();
                            }
                            if response.lost_focus() {
                                if !ui.input(|i| i.key_pressed(Key::Escape)) {
                                    actions.push(Action::RenameLayer(old.clone(), typed.clone()));
                                }
                                actions.push(Action::Cancel);
                            }
                        }
                        _ => {
                            let mut text = RichText::new(&name);
                            if name == current {
                                text = text.strong().color(theme::ACCENT);
                            }
                            let response = ui
                                .add(egui::SelectableLabel::new(app.palette_row.as_ref() == Some(&name), text))
                                .on_hover_text("Double-click to rename");
                            if response.clicked() {
                                app.palette_row = Some(name.clone());
                            }
                            if response.double_clicked() && name != "0" {
                                app.renaming = Some((name.clone(), name.clone()));
                            }
                        }
                    }
                    let mut visible = layer.visible;
                    if ui.checkbox(&mut visible, "").changed() {
                        actions.push(Action::LayerVisible(name.clone(), visible));
                    }
                    let mut frozen = layer.frozen;
                    if ui.checkbox(&mut frozen, "").on_hover_text("Frozen").changed() {
                        actions.push(Action::LayerFrozen(name.clone(), frozen));
                    }
                    let mut locked = layer.locked;
                    if ui.checkbox(&mut locked, "").changed() {
                        actions.push(Action::LayerLocked(name.clone(), locked));
                    }
                    let mut plot = layer.plot;
                    if ui.checkbox(&mut plot, "").on_hover_text("Plots").changed() {
                        actions.push(Action::LayerPlot(name.clone(), plot));
                    }
                    egui::ComboBox::from_id_salt(("layer_color", &name))
                        .width(110.0)
                        .selected_text(swatch_text(Some(layer.color), &color_name(layer.color)))
                        .show_ui(ui, |ui| {
                            for (aci, label) in NAMED_COLORS.iter().take(7) {
                                if ui.selectable_label(layer.color == *aci, swatch_text(Some(*aci), label)).clicked() {
                                    actions.push(Action::LayerColor(name.clone(), *aci));
                                }
                            }
                            ui.separator();
                            if ui.button("More colors…").clicked() {
                                actions.push(Action::ChooseColor(ColorTarget::Layer(name.clone())));
                            }
                        });
                    egui::ComboBox::from_id_salt(("layer_linetype", &name)).width(100.0).selected_text(layer.linetype.clone()).show_ui(
                        ui,
                        |ui| {
                            for linetype in LINETYPES {
                                if ui.selectable_label(layer.linetype == linetype, linetype).clicked() {
                                    actions.push(Action::LayerLinetype(name.clone(), linetype.to_string()));
                                }
                            }
                        },
                    );
                    // A layer's lineweight is a real one or Default; ByLayer and ByBlock mean nothing for a layer.
                    let layer_name = name.clone();
                    lineweight_combo(
                        ui,
                        &format!("layer_lineweight_{name}"),
                        90.0,
                        Some(layer.lineweight),
                        lineweight::choices().filter(|lw| *lw >= 0 || *lw == lineweight::DEFAULT),
                        move |lw| Action::LayerLineweight(layer_name.clone(), lw),
                        &mut actions,
                    );
                    if let Some(frozen) = &viewport_frozen {
                        let mut here = frozen.iter().any(|f| f.eq_ignore_ascii_case(&name));
                        if ui.checkbox(&mut here, "").on_hover_text("Frozen in this viewport").changed() {
                            actions.push(Action::LayerViewportFrozen(name.clone(), here));
                        }
                    }
                    ui.end_row();
                }
            });
        });
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("New").clicked() {
                actions.push(Action::NewLayer);
            }
            let chosen = app.palette_row.clone().filter(|row| layers.iter().any(|l| &l.name == row));
            if ui.add_enabled(chosen.is_some(), egui::Button::new("Delete")).clicked() {
                actions.extend(chosen.clone().map(Action::DeleteLayer));
            }
            if ui.add_enabled(chosen.is_some(), egui::Button::new("Set Current")).clicked() {
                actions.extend(chosen.map(Action::CurrentLayer));
            }
        });
        app.actions.extend(actions);
    });
    app.palette_open = open;
}

// ------------------------------------------------------------------------------------------------ properties

/// The Properties palette (Ctrl+1): what is selected, and the properties of a single object, each editable by typing
/// a new value and pressing Enter.
pub fn properties_palette(app: &mut LinewrightApp, ctx: &egui::Context) {
    let frame = Frame::new().fill(theme::PANEL).inner_margin(Margin::same(8)).stroke(Stroke::new(1.0_f32, theme::BORDER));
    let mut open = app.properties_open;
    egui::SidePanel::right("properties_palette").frame(frame).default_width(300.0).min_width(260.0).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Properties").strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if icon_button(ui, "WIN_CLOSE", vec2(22.0, 22.0), 14.0, true, false).on_hover_text("Close (Ctrl+1)").clicked() {
                    open = false;
                }
            });
        });
        ui.separator();
        let selected = app.controller.selected();
        let ids: Vec<EntityId> = selected.iter().map(|e| e.id).collect();
        if app.property_target != ids {
            app.property_target = ids.clone();
            app.property_edits.clear();
        }
        if selected.is_empty() {
            ui.label(RichText::new("No selection").color(theme::TEXT_DIM));
        } else {
            let mut kinds: std::collections::BTreeMap<&str, usize> = Default::default();
            for entity in &selected {
                *kinds.entry(entity.shape.kind()).or_default() += 1;
            }
            if let [(kind, count)] = kinds.iter().collect::<Vec<_>>()[..] {
                let heading = if *count == 1 { kind.to_string() } else { format!("{kind} ({count})") };
                ui.label(RichText::new(heading).strong().color(theme::ACCENT));
            } else {
                ui.label(RichText::new(format!("{} objects selected", selected.len())).strong());
                for (kind, n) in kinds {
                    ui.label(RichText::new(format!("{n} × {kind}")).color(theme::TEXT_DIM));
                }
                ui.label(RichText::new("Select objects of one kind to change them together.").color(theme::TEXT_DIM));
            }
            ui.add_space(4.0);
            let mut actions = vec![];
            // General: what every object has, whatever its kind.
            let values = GeneralValues::of(app);
            ui.label(RichText::new("General").strong());
            egui::Grid::new("general_rows").num_columns(2).spacing(vec2(10.0, 6.0)).striped(true).show(ui, |ui| {
                ui.label(RichText::new("Color").color(theme::TEXT_DIM));
                color_combo(ui, "palette_color", 150.0, values.color, &mut actions);
                ui.end_row();
                ui.label(RichText::new("Layer").color(theme::TEXT_DIM));
                layer_combo(ui, "palette_layer", 150.0, &values, &mut actions);
                ui.end_row();
                ui.label(RichText::new("Linetype").color(theme::TEXT_DIM));
                linetype_combo(ui, "palette_linetype", 150.0, &values.linetype, &mut actions);
                ui.end_row();
                ui.label(RichText::new("Lineweight").color(theme::TEXT_DIM));
                lineweight_combo(
                    ui,
                    "palette_lineweight",
                    150.0,
                    values.lineweight,
                    lineweight::choices(),
                    Action::UseLineweight,
                    &mut actions,
                );
                ui.end_row();
            });
            ui.add_space(6.0);
            ui.label(RichText::new("Geometry").strong());
            egui::Grid::new("property_rows").num_columns(2).spacing(vec2(10.0, 6.0)).striped(true).show(ui, |ui| {
                for property in shared_properties(&selected) {
                    ui.label(RichText::new(&property.name).color(theme::TEXT_DIM));
                    if property.editable {
                        let buffer = app.property_edits.entry(property.name.clone()).or_insert_with(|| property.value.clone());
                        let response = ui.add(TextEdit::singleline(buffer).desired_width(150.0));
                        if response.has_focus() {
                            app.property_focus = true;
                        }
                        if response.lost_focus() {
                            if ui.input(|i| i.key_pressed(Key::Enter)) && *buffer != property.value {
                                actions.push(Action::SetProperty(ids.clone(), property.name.clone(), buffer.clone()));
                            }
                            app.property_edits.remove(&property.name);
                        }
                    } else {
                        ui.label(&property.value);
                    }
                    ui.end_row();
                }
            });
            app.actions.extend(actions);
        }
    });
    app.properties_open = open;
}

// ------------------------------------------------------------------------------------------------ canvas

const WELCOME_WIDTH: f32 = 860.0;
const TWO_COLUMNS_FROM: f32 = 720.0;
const RECENT_ROW: f32 = 38.0;
/// Room the page takes besides the recent rows: the title, the cards, the panels' headings and margins, the keys.
const WELCOME_FIXED_HEIGHT: f32 = 310.0;

/// The start page: a faint drafting grid behind the name, cards for a new or an existing drawing, the drawings used
/// lately, and first steps with the shortcuts worth knowing. Two columns when there is room, one when there is not.
fn welcome(app: &mut LinewrightApp, ctx: &egui::Context) {
    egui::CentralPanel::default().frame(Frame::new().fill(theme::CANVAS)).show(ctx, |ui| {
        app.canvas_rect = ui.max_rect();
        welcome_grid(ui, ui.max_rect());
        // Heights as last drawn: the page's, to center it, and each panel's, so the two match.
        let page_id = Id::new("welcome_page_height");
        let page_height = ui.data(|d| d.get_temp::<f32>(page_id)).unwrap_or(0.0);
        let room = ui.available_height();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(((room - page_height) / 2.0).max(12.0));
                let width = ui.available_width().min(WELCOME_WIDTH) - 32.0;
                let page = ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| {
                    welcome_title(ui);
                    ui.add_space(18.0);
                    let enabled = !app.is_opening();
                    ui.columns(2, |columns| {
                        if welcome_card(&mut columns[0], "NEW", "New drawing", "A blank sheet in millimeters", enabled).clicked() {
                            app.actions.push(Action::New);
                        }
                        if welcome_card(&mut columns[1], "OPEN", "Open drawing", "DXF, or DWG through the ODA converter", enabled).clicked()
                        {
                            app.actions.push(Action::Open);
                        }
                    });
                    ui.add_space(14.0);
                    let panels_id = Id::new("welcome_panel_height");
                    let tallest = ui.data(|d| d.get_temp::<f32>(panels_id)).unwrap_or(0.0);
                    // As many recent drawings as fit without scrolling, but always a few.
                    let rows = (((room - WELCOME_FIXED_HEIGHT) / RECENT_ROW) as usize).clamp(3, 8);
                    let heights = if width >= TWO_COLUMNS_FROM {
                        ui.columns(2, |columns| {
                            [welcome_recent(app, &mut columns[0], tallest, rows), welcome_tips(&mut columns[1], tallest)]
                        })
                    } else {
                        let recent = welcome_recent(app, ui, 0.0, rows);
                        ui.add_space(12.0);
                        [recent, welcome_tips(ui, 0.0)]
                    };
                    ui.data_mut(|d| d.insert_temp(panels_id, heights[0].max(heights[1])));
                    ui.add_space(12.0);
                    welcome_shortcuts(ui);
                });
                ui.data_mut(|d| d.insert_temp(page_id, page.response.rect.height()));
                ui.add_space(12.0);
            });
        });
    });
}

/// Graph-paper lines, fading toward the bottom, as on a drafting sheet.
fn welcome_grid(ui: &egui::Ui, area: Rect) {
    let painter = ui.painter_at(area);
    let step = 28.0;
    let mut x = area.left() + (area.width() % step) / 2.0;
    while x < area.right() {
        painter.line_segment([pos2(x, area.top()), pos2(x, area.bottom())], Stroke::new(1.0_f32, theme::GRID_MINOR));
        x += step;
    }
    let mut row = 0;
    let mut y = area.top() + step;
    while y < area.bottom() {
        let color = if row % 5 == 4 { theme::GRID_MAJOR } else { theme::GRID_MINOR };
        painter.line_segment([pos2(area.left(), y), pos2(area.right(), y)], Stroke::new(1.0_f32, color));
        y += step;
        row += 1;
    }
    // Let the grid sink into the background lower down, where the lists are.
    let fade = Rect::from_min_max(pos2(area.left(), area.top() + area.height() * 0.3), area.max);
    let mut mesh = egui::Mesh::default();
    let clear = Color32::from_rgba_premultiplied(0, 0, 0, 0);
    mesh.colored_vertex(fade.left_top(), clear);
    mesh.colored_vertex(fade.right_top(), clear);
    mesh.colored_vertex(fade.right_bottom(), theme::CANVAS);
    mesh.colored_vertex(fade.left_bottom(), theme::CANVAS);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
}

/// The mark and name: a drafting square with a line drawn across it and a compass arc, then the tagline.
fn welcome_title(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(58.0, 58.0), Sense::hover());
        let painter = ui.painter();
        painter.rect(
            rect.shrink(2.0),
            CornerRadius::same(12),
            theme::PANEL_RAISED,
            Stroke::new(1.0_f32, theme::BORDER_STRONG),
            StrokeKind::Inside,
        );
        let inner = rect.shrink(15.0);
        let pen = Stroke::new(2.4_f32, theme::TEXT);
        painter.line_segment([inner.left_bottom(), inner.right_top()], pen);
        painter.circle_filled(inner.left_bottom(), 3.2, theme::TEXT);
        painter.circle_filled(inner.right_top(), 3.2, theme::TEXT);
        // A compass arc swung from the line's start, as a draftsman checks a length.
        let arc: Vec<Pos2> = (0..=24)
            .map(|i| {
                let a = std::f32::consts::FRAC_PI_2 * i as f32 / 24.0;
                inner.left_bottom() + vec2(a.cos(), -a.sin()) * inner.width() * 0.7
            })
            .collect();
        painter.add(egui::Shape::line(arc, Stroke::new(1.6_f32, theme::ACCENT_EDGE.gamma_multiply(1.6))));
        ui.add_space(10.0);
        ui.vertical(|ui| {
            ui.add_space(2.0);
            ui.label(RichText::new(WINDOW_NAME).size(30.0).strong());
            ui.label(RichText::new("2D drafting for DXF and DWG drawings, the AutoCAD way").color(theme::TEXT_DIM));
        });
    });
}

/// A large clickable card with an icon, a title and a line saying what it does.
fn welcome_card(ui: &mut egui::Ui, icon: &str, title: &str, detail: &str, enabled: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 76.0), if enabled { Sense::click() } else { Sense::hover() });
    let hovered = response.hovered() && enabled;
    let fill = if hovered { theme::PANEL_RAISED } else { theme::PANEL };
    let edge = if hovered { theme::ACCENT_EDGE } else { theme::BORDER };
    let painter = ui.painter();
    painter.rect(rect, CornerRadius::same(10), fill, Stroke::new(1.0_f32, edge), StrokeKind::Inside);
    let badge = Rect::from_center_size(pos2(rect.left() + 38.0, rect.center().y), vec2(44.0, 44.0));
    painter.rect_filled(badge, CornerRadius::same(9), if hovered { theme::BORDER_STRONG } else { theme::PANEL_RAISED });
    icons::paint(ui, icon, badge, 22.0, if enabled { theme::TEXT } else { theme::TEXT_DIM });
    let text_left = badge.right() + 14.0;
    painter.text(pos2(text_left, rect.center().y - 11.0), Align2::LEFT_CENTER, title, FontId::proportional(16.0), theme::TEXT);
    painter.text(pos2(text_left, rect.center().y + 12.0), Align2::LEFT_CENTER, detail, FontId::proportional(12.5), theme::TEXT_DIM);
    response.on_hover_cursor(if enabled { CursorIcon::PointingHand } else { CursorIcon::Default })
}

/// A panel at least `height` tall inside, so two side by side end level. Gives how tall its content was.
fn welcome_panel(ui: &mut egui::Ui, title: &str, height: f32, content: impl FnOnce(&mut egui::Ui)) -> f32 {
    Frame::new()
        .fill(theme::PANEL)
        .stroke(Stroke::new(1.0_f32, theme::BORDER))
        .corner_radius(10)
        .inner_margin(14)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let top = ui.cursor().top();
            ui.label(RichText::new(title).strong().size(14.5));
            ui.add_space(6.0);
            content(ui);
            let used = ui.cursor().top() - top;
            if height > used {
                ui.add_space(height - used);
            }
            used
        })
        .inner
}

/// The drawings used lately, one row each: file name over its folder, highlighted under the pointer.
fn welcome_recent(app: &mut LinewrightApp, ui: &mut egui::Ui, height: f32, rows: usize) -> f32 {
    let recent: Vec<std::path::PathBuf> = app.recent.iter().take(rows).cloned().collect();
    let enabled = !app.is_opening();
    welcome_panel(ui, "Recent drawings", height, |ui| {
        if recent.is_empty() {
            ui.add_space(6.0);
            ui.label(RichText::new("Drawings you open or save show here.").color(theme::TEXT_DIM));
            ui.add_space(6.0);
        }
        for path in recent {
            let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
            let folder = path.parent().map_or(String::new(), |p| p.display().to_string());
            let (rect, response) =
                ui.allocate_exact_size(vec2(ui.available_width(), RECENT_ROW), if enabled { Sense::click() } else { Sense::hover() });
            let painter = ui.painter_at(rect);
            if response.hovered() && enabled {
                painter.rect_filled(rect, CornerRadius::same(7), theme::PANEL_RAISED);
            }
            icons::paint(
                ui,
                "NEW",
                Rect::from_center_size(pos2(rect.left() + 18.0, rect.center().y), vec2(20.0, 20.0)),
                18.0,
                theme::TEXT_DIM,
            );
            let left = rect.left() + 38.0;
            painter.text(pos2(left, rect.top() + 11.0), Align2::LEFT_CENTER, &name, FontId::proportional(14.0), theme::TEXT);
            // A long folder keeps its end, which tells folders apart best.
            let room = ((rect.right() - left - 8.0) / 6.6) as usize;
            let chars: Vec<char> = folder.chars().collect();
            let shown = if chars.len() > room && room > 1 {
                format!("…{}", chars[chars.len() - room + 1..].iter().collect::<String>())
            } else {
                folder
            };
            painter.text(pos2(left, rect.top() + 27.0), Align2::LEFT_CENTER, shown, FontId::proportional(11.5), theme::TEXT_DIM);
            let response = response.on_hover_text(path.display().to_string());
            if response.clicked() {
                app.actions.push(Action::OpenRecent(path));
            }
        }
    })
}

fn welcome_chip(ui: &mut egui::Ui, text: &str) {
    Frame::new()
        .fill(theme::PANEL_RAISED)
        .stroke(Stroke::new(1.0_f32, theme::BORDER_STRONG))
        .corner_radius(5)
        .inner_margin(Margin::symmetric(6, 1))
        .show(ui, |ui| {
            ui.label(RichText::new(text).monospace().size(12.0));
        });
}

/// First steps: the commands to type for each kind of work.
fn welcome_tips(ui: &mut egui::Ui, height: f32) -> f32 {
    welcome_panel(ui, "Type a command, then Enter", height, |ui| {
        let steps: [(&str, &[&str]); 4] = [
            ("Draw", &["LINE", "CIRCLE", "HATCH"]),
            ("Change", &["MOVE", "TRIM", "OFFSET"]),
            ("Annotate", &["DIMLINEAR", "QLEADER"]),
            ("Sheets", &["LAYOUT", "MVIEW", "PLOT"]),
        ];
        egui::Grid::new("welcome_steps").num_columns(2).spacing(vec2(12.0, 7.0)).show(ui, |ui| {
            for (text, commands) in steps {
                ui.label(RichText::new(text).color(theme::TEXT_DIM));
                ui.horizontal(|ui| {
                    for command in commands {
                        welcome_chip(ui, command);
                    }
                });
                ui.end_row();
            }
        });
    })
}

/// The keys worth knowing, in one line under the panels.
fn welcome_shortcuts(ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (index, (keys, what)) in
            [("Space", "Enter or repeat"), ("Esc", "Cancel"), ("Ctrl+1", "Properties"), ("F3", "Object snaps"), ("Ctrl+Z", "Undo")]
                .into_iter()
                .enumerate()
        {
            if index > 0 {
                ui.add_space(10.0);
            }
            welcome_chip(ui, keys);
            ui.label(RichText::new(what).color(theme::TEXT_DIM));
        }
    });
}

pub fn canvas(app: &mut LinewrightApp, ctx: &egui::Context) {
    if app.welcome {
        welcome(app, ctx);
        return;
    }
    egui::CentralPanel::default().frame(Frame::new().fill(theme::CANVAS)).show(ctx, |ui| {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        app.canvas_rect = rect;
        let opening = app.is_opening();
        let hovered = response.hovered() && !opening && !app.on_window_edge;
        if !opening {
            handle_canvas_input(app, ui, rect, hovered);
        }
        app.controller.scale = app.camera.scale;
        let screen = Screen::new(rect, app.camera, &app.drawing_fonts, app.document());
        let overlay = Overlay {
            show_grid: app.controller.snap.show_grid,
            pointer: app.pointer,
            panning: app.panning.is_some(),
            typed: app.input.clone(),
            first_field: app.dynamic_first.clone(),
        };
        render::paint(ui, &screen, &app.controller, &overlay);
        navigation_bar(app, ctx, rect);
        prompt_hint(app, ctx, rect);
        if let Some(name) = app.opening_name().map(str::to_string) {
            loading_overlay(ui, ctx, rect, &name);
        }
    });
}

fn handle_canvas_input(app: &mut LinewrightApp, ui: &egui::Ui, rect: egui::Rect, hovered: bool) {
    let (latest, scroll, pinch, primary_pressed, primary_released, middle_down, shift) = ui.input(|i| {
        (
            i.pointer.latest_pos(),
            i.raw_scroll_delta.y,
            i.zoom_delta(),
            i.pointer.primary_pressed(),
            i.pointer.primary_released(),
            i.pointer.middle_down(),
            i.modifiers.shift,
        )
    });
    let center = rect.center();
    let mut moved_view = false;
    // Zooming and panning change what is worked in: the viewport worked through, or the whole view.
    if hovered && let Some(pos) = latest {
        let offset = pos - center;
        // One wheel notch is 50 points of scrolling in egui, and zooms one step.
        let factor = crate::canvas::view::ZOOM_STEP.powf(scroll as f64 / 50.0) * pinch as f64;
        if factor != 1.0 {
            let mut camera = app.view_camera();
            camera.zoom_at(factor, offset.x as f64, offset.y as f64);
            app.set_view_camera(camera);
            moved_view = true;
        }
    }
    match (middle_down, app.panning, latest) {
        (true, Some(previous), Some(pos)) => {
            let delta = pos - previous;
            if delta != egui::Vec2::ZERO {
                let mut camera = app.view_camera();
                camera.pan(delta.x as f64, delta.y as f64);
                app.set_view_camera(camera);
                moved_view = true;
            }
            app.panning = Some(pos);
        }
        (true, None, Some(pos)) if hovered => app.panning = Some(pos),
        (false, Some(_), _) => app.panning = None,
        _ => {}
    }
    let sheet = Screen::new(rect, app.camera, &app.drawing_fonts, app.document());
    // Through a viewport, points are in model space as the viewport shows it.
    let through = app.document().through_viewport().and_then(|v| render::viewport_screen(&sheet, v));
    let frame = through.as_ref().map(|(_, frame)| *frame);
    let screen = through.map_or(sheet, |(inside, _)| inside);
    app.controller.scale = screen.camera.scale;
    let pointer = app.pinned_pointer.or(latest.filter(|_| hovered || app.panning.is_some()));
    if pointer != app.pointer || moved_view {
        app.pointer = pointer;
        app.controller.move_cursor(pointer.map(|p| screen.world(p)));
    }
    // On a layout, the viewport under a point (not counting the one worked through).
    let viewport_at = |app: &LinewrightApp, pos: Pos2| -> Option<EntityId> {
        let document = app.document();
        let layout = document.shown_layout()?;
        let through = document.through().map(|t| t.viewport);
        document.space_store(Some(layout)).entities().iter().rev().find_map(|entity| {
            let (_, frame) = render::viewport_screen(&sheet, entity)?;
            (frame.contains(pos) && Some(entity.id) != through && document.is_visible(entity)).then_some(entity.id)
        })
    };
    let outside_frame = |pos: Pos2| frame.is_some_and(|frame| !frame.contains(pos));
    // A click while a right-click menu is open only closes the menu.
    let menu_open = app.context_menu.is_some();
    if hovered
        && primary_pressed
        && !menu_open
        && let Some(pos) = latest
    {
        if outside_frame(pos) {
            // Clicking another viewport makes it the one worked through; elsewhere on the sheet does nothing.
            if let Some(other) = viewport_at(app, pos) {
                app.actions.push(Action::EnterViewport(other));
            }
        } else {
            app.controller.click(screen.world(pos));
        }
    }
    if primary_released && let Some(pos) = latest {
        app.controller.release(screen.world(pos), shift);
    }
    let (double_clicked, right_clicked) = ui.input(|i| {
        (i.pointer.button_double_clicked(egui::PointerButton::Primary), i.pointer.button_clicked(egui::PointerButton::Secondary))
    });
    if hovered && let Some(pos) = latest {
        let idle = app.controller.running_command().is_none();
        if double_clicked && frame.is_some() && outside_frame(pos) && viewport_at(app, pos).is_none() {
            // Double-clicking the sheet outside the viewports goes back to paper space, as in AutoCAD.
            app.actions.push(Action::Command("PSPACE"));
        } else if double_clicked
            && frame.is_none()
            && idle
            && app.document().active_layout().is_some()
            && let Some(viewport) =
                viewport_at(app, pos).filter(|_| app.controller.hit(screen.world(pos)).is_none_or(|hit| is_viewport(app, hit)))
        {
            // Double-clicking inside a viewport works on model space through it.
            app.actions.push(Action::EnterViewport(viewport));
        } else if double_clicked && !outside_frame(pos) {
            app.controller.double_click(screen.world(pos));
        }
        if right_clicked {
            open_context_menu(app, pos, ui.input(|i| i.modifiers.shift));
        }
    }
    if hovered {
        let icon = match (app.panning.is_some(), latest.is_some_and(outside_frame)) {
            (true, _) => CursorIcon::Grabbing,
            (false, true) => CursorIcon::Default, // outside the viewport worked through, as AutoCAD's arrow
            (false, false) => CursorIcon::None,
        };
        ui.ctx().set_cursor_icon(icon);
    }
}

/// The right button on the drawing, as AutoCAD answers it: with Shift, the object snaps while a point is asked;
/// during a command, its menu when the prompt has options and Enter when not; otherwise the menu for the selection,
/// or for nothing selected.
pub fn open_context_menu(app: &mut LinewrightApp, at: Pos2, shift: bool) {
    let controller = &app.controller;
    let kind = if shift && controller.wants_point() {
        MenuKind::Snap
    } else if controller.running_command().is_some() {
        if controller.prompt_options().is_empty() {
            app.actions.push(Action::Enter);
            return;
        }
        MenuKind::Command
    } else if controller.selection.is_empty() {
        MenuKind::Default
    } else {
        MenuKind::Edit
    };
    app.context_menu = Some(ContextMenu { at, kind, fresh: true });
}

/// One line of a right-click menu, with its shortcut on the right. True when chosen.
fn menu_item(ui: &mut egui::Ui, text: &str, shortcut: &str, enabled: bool) -> bool {
    ui.add_enabled(enabled, egui::Button::new(text).shortcut_text(shortcut).min_size(vec2(MENU_WIDTH, 0.0))).clicked()
}

const MENU_WIDTH: f32 = 230.0;

/// A line of a right-click menu that opens more, with an arrow at its right end.
fn submenu(ui: &mut egui::Ui, text: &str, add: impl FnOnce(&mut egui::Ui)) {
    let response = ui.menu_button(text, add).response;
    let right = ui.max_rect().right() - 4.0;
    ui.painter().text(pos2(right, response.rect.center().y), Align2::RIGHT_CENTER, "⏵", FontId::proportional(12.0), theme::TEXT_DIM);
}

fn recent_menu(ui: &mut egui::Ui, recent: &[&'static str], actions: &mut Vec<Action>) {
    ui.add_enabled_ui(!recent.is_empty(), |ui| {
        submenu(ui, "Recent Input", |ui| {
            for name in recent {
                if ui.button(format!("{} ({name})", label(name))).clicked() {
                    actions.push(Action::Command(name));
                    ui.close_menu();
                }
            }
        });
    });
}

fn zoom_menu(ui: &mut egui::Ui, actions: &mut Vec<Action>) {
    submenu(ui, "Zoom", |ui| {
        for (text, action) in [("Extents", Action::ZoomExtents), ("In", Action::ZoomIn), ("Out", Action::ZoomOut)] {
            if ui.button(text).clicked() {
                actions.push(action);
                ui.close_menu();
            }
        }
    });
}

fn snap_items(ui: &mut egui::Ui, actions: &mut Vec<Action>) -> bool {
    let mut chosen = false;
    for kind in SnapKind::ALL {
        let text = RichText::new(kind.name());
        if ui.add(egui::Button::new(text).min_size(vec2(MENU_WIDTH - 40.0, 0.0))).clicked() {
            actions.push(Action::SnapOverride(kind));
            chosen = true;
            ui.close_menu();
        }
    }
    chosen
}

/// The open right-click menu, at the point it was opened.
pub fn context_menu(app: &mut LinewrightApp, ctx: &egui::Context) {
    let Some(menu) = app.context_menu else { return };
    let mut actions: Vec<Action> = vec![];
    let mut chosen = false;
    let recent: Vec<&'static str> = app.controller.recent_commands().to_vec();
    let selected = app.controller.selected();
    let clipboard_full = !app.controller.defaults.borrow().clipboard.is_empty();
    let area = egui::Area::new(Id::new("context_menu")).order(egui::Order::Foreground).fixed_pos(menu.at).constrain(true).show(ctx, |ui| {
        Frame::menu(ui.style()).show(ui, |ui| {
            ui.set_max_width(MENU_WIDTH);
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            // Menu lines are flat until pointed at, as in AutoCAD's menus.
            ui.style_mut().visuals.button_frame = false;
            ui.spacing_mut().item_spacing.y = 1.0;
            let mut item = |ui: &mut egui::Ui, text: &str, shortcut: &str, enabled: bool, action: Action| {
                if menu_item(ui, text, shortcut, enabled) {
                    actions.push(action);
                    chosen = true;
                }
            };
            match menu.kind {
                MenuKind::Snap => {
                    ui.label(RichText::new("Snap to").color(theme::TEXT_DIM));
                    let mut picked = vec![];
                    chosen |= snap_items(ui, &mut picked);
                    actions.extend(picked);
                }
                MenuKind::Command => {
                    item(ui, "Enter", "", true, Action::Enter);
                    item(ui, "Cancel", "Esc", true, Action::Cancel);
                    ui.separator();
                    for (name, typed) in app.controller.prompt_options() {
                        item(ui, &name, "", true, Action::Answer(typed));
                    }
                    if app.controller.wants_point() {
                        ui.separator();
                        submenu(ui, "Snap Overrides", |ui| {
                            let mut picked = vec![];
                            if snap_items(ui, &mut picked) {
                                chosen = true;
                            }
                            actions.extend(picked);
                        });
                    }
                    let mut more = vec![];
                    zoom_menu(ui, &mut more);
                    chosen |= !more.is_empty();
                    actions.extend(more);
                }
                MenuKind::Default | MenuKind::Edit => {
                    if let Some(last) = recent.first() {
                        item(ui, &format!("Repeat {}", label(last)), "Enter", true, Action::Command(last));
                    }
                    let mut more = vec![];
                    recent_menu(ui, &recent, &mut more);
                    ui.separator();
                    let edit = menu.kind == MenuKind::Edit;
                    if edit {
                        item(ui, "Cut", "Ctrl+X", true, Action::Cut);
                        item(ui, "Copy", "Ctrl+C", true, Action::Copy);
                    }
                    item(ui, "Paste", "Ctrl+V", clipboard_full, Action::Paste);
                    ui.separator();
                    if edit {
                        // What only some objects offer, for one object picked.
                        if let [only] = &selected[..] {
                            match &only.shape {
                                Shape::Text { .. } | Shape::Dimension { .. } | Shape::Multileader { .. } => {
                                    item(ui, "Edit Text", "", true, Action::Command("TEXTEDIT"))
                                }
                                Shape::Insert { .. } => {
                                    item(ui, "Block Editor", "", true, Action::Command("BEDIT"));
                                    item(ui, "Explode", "", true, Action::Command("EXPLODE"));
                                }
                                Shape::Polyline { .. } => item(ui, "Polyline Edit", "", true, Action::Command("PEDIT")),
                                Shape::Viewport { locked, .. } => {
                                    item(ui, "Work in Viewport", "", true, Action::EnterViewport(only.id));
                                    let (text, value) = if *locked { ("Unlock Display", "No") } else { ("Lock Display", "Yes") };
                                    item(ui, text, "", true, Action::SetProperty(vec![only.id], "Display locked".into(), value.into()));
                                }
                                _ => {}
                            }
                        }
                        item(ui, "Erase", "Del", true, Action::EraseSelection);
                        item(ui, "Move", "", true, Action::Command("MOVE"));
                        item(ui, "Copy Selection", "", true, Action::Command("COPY"));
                        item(ui, "Scale", "", true, Action::Command("SCALE"));
                        item(ui, "Rotate", "", true, Action::Command("ROTATE"));
                        submenu(ui, "Draw Order", |ui| {
                            for (text, answer) in [("Bring to Front", "F"), ("Send to Back", "B")] {
                                if ui.button(text).clicked() {
                                    more.push(Action::CommandWith("DRAWORDER", answer));
                                    ui.close_menu();
                                }
                            }
                        });
                        submenu(ui, "Group", |ui| {
                            for (text, name) in [("Group", "GROUP"), ("Ungroup", "UNGROUP")] {
                                if ui.button(text).clicked() {
                                    more.push(Action::Command(name));
                                    ui.close_menu();
                                }
                            }
                        });
                        ui.separator();
                        item(ui, "Select Similar", "", true, Action::Command("SELECTSIMILAR"));
                        item(ui, "Deselect All", "", true, Action::DeselectAll);
                    } else {
                        item(ui, "Undo", "Ctrl+Z", true, Action::Undo);
                        item(ui, "Redo", "Ctrl+Y", true, Action::Redo);
                        zoom_menu(ui, &mut more);
                        item(ui, "Select All", "Ctrl+A", true, Action::SelectAll);
                    }
                    ui.separator();
                    item(ui, "Quick Select…", "", true, Action::Command("QSELECT"));
                    item(ui, "Find…", "", true, Action::Command("FIND"));
                    item(ui, "Properties", "Ctrl+1", true, Action::ShowProperties);
                    chosen |= !more.is_empty();
                    actions.extend(more);
                }
            }
        });
    });
    // A press anywhere but on the menu (or one of its submenus) closes it.
    let pressed_away = ctx.input(|i| i.pointer.any_pressed()) && !area.response.hovered() && !ctx.is_pointer_over_area();
    if chosen || (pressed_away && !menu.fresh) {
        app.context_menu = None;
    } else if let Some(open) = &mut app.context_menu {
        open.fresh = false;
    }
    app.actions.extend(actions);
}

fn is_viewport(app: &LinewrightApp, id: EntityId) -> bool {
    app.document().get(id).is_some_and(|e| matches!(e.shape, crate::model::entities::Shape::Viewport { .. }))
}

fn navigation_bar(app: &mut LinewrightApp, ctx: &egui::Context, canvas: egui::Rect) {
    let size = vec2(42.0, 116.0);
    let position = pos2(canvas.right() - size.x - 12.0, canvas.center().y - size.y / 2.0);
    egui::Area::new(Id::new("navigation")).order(egui::Order::Foreground).fixed_pos(position).show(ctx, |ui| {
        Frame::new()
            .fill(theme::PANEL.gamma_multiply(0.94))
            .stroke(Stroke::new(1.0_f32, theme::BORDER_STRONG))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::same(3))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
                for (icon, tip, action) in [
                    ("ZOOM_EXTENTS", "Zoom Extents (Ctrl+E)", Action::ZoomExtents),
                    ("ZOOM_IN", "Zoom In", Action::ZoomIn),
                    ("ZOOM_OUT", "Zoom Out", Action::ZoomOut),
                ] {
                    if icon_button(ui, icon, vec2(34.0, 34.0), 20.0, true, false).on_hover_text(tip).clicked() {
                        app.actions.push(action);
                    }
                }
            });
    });
}

/// A chip in the corner of the drawing area naming the running command and what it asks for.
fn prompt_hint(app: &LinewrightApp, ctx: &egui::Context, canvas: egui::Rect) {
    let (Some(command), Some(prompt)) = (app.controller.running_command(), app.controller.prompt()) else { return };
    egui::Area::new(Id::new("prompt_hint"))
        .order(egui::Order::Foreground)
        .interactable(false)
        .fixed_pos(canvas.min + vec2(16.0, 16.0))
        .show(ctx, |ui| {
            Frame::new()
                .fill(theme::PANEL.gamma_multiply(0.94))
                .stroke(Stroke::new(1.0_f32, theme::BORDER_STRONG))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(Margin { left: 12, right: 14, top: 7, bottom: 7 })
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(icons::image(command, theme::ACCENT).fit_to_exact_size(vec2(16.0, 16.0)));
                        ui.label(RichText::new(label(command)).strong().color(theme::TEXT));
                        ui.label(RichText::new(format!("· {}", prompt.message.trim_end_matches(':'))).color(theme::TEXT_DIM));
                    });
                });
        });
}

/// Covers the drawing area with a message and a spinner while a file is opening.
fn loading_overlay(ui: &egui::Ui, ctx: &egui::Context, canvas: egui::Rect, name: &str) {
    ui.painter().rect_filled(canvas, 0.0, theme::OVERLAY);
    egui::Area::new(Id::new("loading")).order(egui::Order::Foreground).anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
        Frame::new()
            .fill(theme::PANEL_RAISED)
            .stroke(Stroke::new(1.0_f32, theme::BORDER_STRONG))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin { left: 20, right: 20, top: 18, bottom: 18 })
            .show(ui, |ui| {
                ui.set_width(300.0);
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().color(theme::ACCENT));
                    ui.label(RichText::new(format!("Opening {name}")).strong().size(14.5));
                });
                ui.add_space(8.0);
                ui.label(RichText::new("Large drawings can take a little while.").color(theme::TEXT_DIM));
            });
    });
}

// ------------------------------------------------------------------------------------------------ dialogs

pub fn dialogs(app: &mut LinewrightApp, ctx: &egui::Context) {
    let Some(dialog) = &mut app.dialog else { return };
    match dialog {
        Dialog::Message(message) => {
            let message = message.clone();
            let modal = egui::Modal::new(Id::new("message")).show(ctx, |ui| {
                ui.set_max_width(420.0);
                ui.label(RichText::new(WINDOW_NAME).strong().size(15.0));
                ui.add_space(8.0);
                ui.label(message);
                ui.add_space(12.0);
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| ui.button("OK").clicked()).inner
            });
            if modal.inner || modal.should_close() || ctx.input(|i| i.key_pressed(Key::Enter)) {
                app.dialog = None;
            }
        }
        Dialog::Discard(then) => {
            let then = *then;
            let name = app.document().path.as_ref().map_or(UNTITLED.to_string(), |_| app.file_name());
            let modal = egui::Modal::new(Id::new("discard")).show(ctx, |ui| {
                ui.label(RichText::new(WINDOW_NAME).strong().size(15.0));
                ui.add_space(8.0);
                ui.label(format!("Save changes to {name}?"));
                ui.add_space(12.0);
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    if ui.button("Cancel").clicked() {
                        return Some(None);
                    }
                    if ui.button("Discard").clicked() {
                        return Some(Some(false));
                    }
                    if ui.button("Save").clicked() {
                        return Some(Some(true));
                    }
                    None
                })
                .inner
            });
            match modal.inner {
                Some(Some(save_first)) => app.discard_answered(then, save_first, ctx),
                Some(None) => app.dialog = None,
                None if modal.should_close() => app.dialog = None,
                None => {}
            }
        }
        Dialog::Plot(form) => {
            let units = unit_names(&app.controller.drawing.document).0;
            let mut accepted = false;
            let mut cancelled = false;
            let modal = egui::Modal::new(Id::new("plot")).show(ctx, |ui| {
                ui.set_width(360.0);
                ui.label(RichText::new(if form.layout { "Plot Layout to PDF" } else { "Export PDF" }).strong().size(15.0));
                ui.add_space(8.0);
                egui::Grid::new("plot_form").num_columns(2).spacing(vec2(12.0, 8.0)).show(ui, |ui| {
                    ui.label("Plot style table");
                    let shown = if form.style_table.is_empty() { "None" } else { form.style_table.as_str() };
                    egui::ComboBox::from_id_salt("plot_style_table").width(200.0).selected_text(shown).show_ui(ui, |ui| {
                        ui.selectable_value(&mut form.style_table, String::new(), "None");
                        for name in &form.tables {
                            ui.selectable_value(&mut form.style_table, name.clone(), name);
                        }
                    });
                    ui.end_row();
                    ui.label("");
                    // With a plot style table, its pens decide the lineweights, as in AutoCAD.
                    ui.add_enabled_ui(form.style_table.is_empty(), |ui| ui.checkbox(&mut form.lineweights, "Plot object lineweights"));
                    ui.end_row();
                    if form.layout {
                        return;
                    }
                    ui.label("Paper");
                    egui::ComboBox::from_id_salt("paper").width(180.0).selected_text(PAPER_SIZES[form.paper].0).show_ui(ui, |ui| {
                        for (index, (name, _, _)) in PAPER_SIZES.iter().enumerate() {
                            ui.selectable_value(&mut form.paper, index, *name);
                        }
                    });
                    ui.end_row();
                    ui.label("Orientation");
                    egui::ComboBox::from_id_salt("orientation")
                        .width(180.0)
                        .selected_text(if form.landscape { "Landscape" } else { "Portrait" })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut form.landscape, true, "Landscape");
                            ui.selectable_value(&mut form.landscape, false, "Portrait");
                        });
                    ui.end_row();
                    ui.label("Scale");
                    ui.horizontal(|ui| {
                        ui.add(TextEdit::singleline(&mut form.scale).id(Id::new(super::app::PLOT_SCALE_INPUT)).desired_width(130.0));
                        ui.menu_button("▾", |ui| {
                            for scale in SCALES {
                                if ui.button(scale).clicked() {
                                    form.scale = scale.to_string();
                                    ui.close_menu();
                                }
                            }
                        });
                    });
                    ui.end_row();
                });
                if form.layout {
                    ui.label(RichText::new("The layout's sheet prints at 1:1.").color(theme::TEXT_DIM));
                } else {
                    ui.label(RichText::new(format!("Drawing units: {units}")).color(theme::TEXT_DIM));
                }
                if !form.error.is_empty() {
                    ui.label(RichText::new(&form.error).color(theme::ERROR));
                }
                ui.add_space(10.0);
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    cancelled = ui.button("Cancel").clicked();
                    accepted = ui.button("OK").clicked();
                });
            });
            if accepted || ctx.input(|i| i.key_pressed(Key::Enter)) {
                app.plot_accepted();
            } else if cancelled || modal.should_close() {
                app.dialog = None;
            }
        }
        Dialog::EditBlock(typed) => {
            let mut names: Vec<String> =
                app.controller.document().blocks.values().filter(|b| b.0.xref.is_none()).map(|b| b.0.name.clone()).collect();
            names.sort();
            let (mut open, mut cancel) = (None, false);
            let modal = egui::Modal::new(Id::new("edit_block")).show(ctx, |ui| {
                ui.set_width(320.0);
                ui.label(RichText::new("Edit Block Definition").strong().size(15.0));
                ui.add_space(8.0);
                ui.add(TextEdit::singleline(typed).hint_text("Block to create or edit").desired_width(290.0));
                egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                    for name in &names {
                        if ui.selectable_label(typed.eq_ignore_ascii_case(name), name).clicked() {
                            *typed = name.clone();
                        }
                    }
                    if names.is_empty() {
                        ui.label(RichText::new("The drawing has no blocks yet.").color(theme::TEXT_DIM));
                    }
                });
                ui.add_space(10.0);
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    if ui.add_enabled(names.iter().any(|n| n.eq_ignore_ascii_case(typed.trim())), egui::Button::new("OK")).clicked() {
                        open = Some(typed.trim().to_uppercase());
                    }
                });
            });
            if let Some(key) = open
                && let Some(block) = app.controller.document().blocks.get(&key).cloned()
            {
                app.edit_block(block);
            } else if cancel || modal.should_close() {
                app.dialog = None;
            }
        }
        Dialog::CloseBlock => {
            let name = app.controller.document().editing_block().map(|b| b.0.name.clone()).unwrap_or_default();
            let mut choice = None;
            let modal = egui::Modal::new(Id::new("close_block")).show(ctx, |ui| {
                ui.label(RichText::new("Block Editor").strong().size(15.0));
                ui.add_space(8.0);
                ui.label(format!("Save the changes to {name}?"));
                ui.add_space(12.0);
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    if ui.button("Cancel").clicked() {
                        choice = Some(None);
                    }
                    if ui.button("Discard Changes").clicked() {
                        choice = Some(Some(false));
                    }
                    if ui.button("Save Changes").clicked() {
                        choice = Some(Some(true));
                    }
                });
            });
            match choice {
                Some(Some(save)) => app.close_block(save),
                Some(None) => app.dialog = None,
                None if modal.should_close() => app.dialog = None,
                None => {}
            }
        }
        Dialog::Xrefs => {
            let xrefs: Vec<(String, String, usize)> = app
                .controller
                .document()
                .xrefs()
                .iter()
                .map(|b| (b.0.name.clone(), b.0.xref.clone().unwrap_or_default(), b.0.entities.len()))
                .collect();
            let (mut action, mut attach, mut close) = (None, false, false);
            let modal = egui::Modal::new(Id::new("xrefs")).show(ctx, |ui| {
                ui.set_width(460.0);
                ui.label(RichText::new("External References").strong().size(15.0));
                ui.add_space(8.0);
                if xrefs.is_empty() {
                    ui.label(RichText::new("No drawings are attached.").color(theme::TEXT_DIM));
                }
                egui::Grid::new("xref_list").num_columns(3).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
                    for (name, path, count) in &xrefs {
                        ui.label(RichText::new(name).strong());
                        let status = if *count == 0 { "Not found".to_string() } else { format!("{count} objects") };
                        ui.label(RichText::new(status).color(theme::TEXT_DIM)).on_hover_text(path);
                        ui.horizontal(|ui| {
                            if ui.button("Reload").clicked() {
                                action = Some((name.clone(), false));
                            }
                            if ui.button("Detach").clicked() {
                                action = Some((name.clone(), true));
                            }
                        });
                        ui.end_row();
                    }
                });
                ui.add_space(10.0);
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    close = ui.button("Close").clicked();
                    attach = ui.button("Attach DWG/DXF…").clicked();
                });
            });
            if let Some((name, detach)) = action {
                app.xref_action(&name, detach);
            } else if attach {
                app.dialog = None;
                app.actions.push(Action::Command("XATTACH"));
            } else if close || modal.should_close() {
                app.dialog = None;
            }
        }
        Dialog::QuickSelect(form) => {
            let document = app.controller.document();
            let mut kinds: Vec<&'static str> = document.entities().iter().map(|e| e.shape.kind()).collect();
            kinds.sort();
            kinds.dedup();
            let mut values: Vec<String> = document
                .entities()
                .iter()
                .filter(|e| form.kind.is_empty() || e.shape.kind() == form.kind)
                .map(|e| quick_select_value(e, form.property))
                .collect();
            values.sort();
            values.dedup();
            let (mut apply, mut cancel) = (false, false);
            let modal = egui::Modal::new(Id::new("quick_select")).show(ctx, |ui| {
                ui.set_width(360.0);
                ui.label(RichText::new("Quick Select").strong().size(15.0));
                ui.add_space(8.0);
                egui::Grid::new("quick_select_form").num_columns(2).spacing(vec2(12.0, 8.0)).show(ui, |ui| {
                    ui.label("Object type");
                    let shown = if form.kind.is_empty() { "Any".to_string() } else { form.kind.clone() };
                    egui::ComboBox::from_id_salt("qs_kind").width(200.0).selected_text(shown).show_ui(ui, |ui| {
                        ui.selectable_value(&mut form.kind, String::new(), "Any");
                        for kind in &kinds {
                            ui.selectable_value(&mut form.kind, kind.to_string(), *kind);
                        }
                    });
                    ui.end_row();
                    ui.label("Property");
                    egui::ComboBox::from_id_salt("qs_property").width(200.0).selected_text(QUICK_SELECT_PROPERTIES[form.property]).show_ui(
                        ui,
                        |ui| {
                            for (index, name) in QUICK_SELECT_PROPERTIES.iter().enumerate() {
                                ui.selectable_value(&mut form.property, index, *name);
                            }
                        },
                    );
                    ui.end_row();
                    ui.label("Operator");
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut form.equals, true, "= Equals");
                        ui.selectable_value(&mut form.equals, false, "<> Not Equal");
                    });
                    ui.end_row();
                    ui.label("Value");
                    egui::ComboBox::from_id_salt("qs_value").width(200.0).selected_text(form.value.clone()).show_ui(ui, |ui| {
                        for value in &values {
                            ui.selectable_value(&mut form.value, value.clone(), value);
                        }
                    });
                    ui.end_row();
                });
                ui.checkbox(&mut form.append, "Append to current selection set");
                ui.add_space(10.0);
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    apply = ui.button("OK").clicked();
                });
            });
            if apply || ctx.input(|i| i.key_pressed(Key::Enter)) {
                app.quick_select();
            } else if cancel || modal.should_close() {
                app.dialog = None;
            }
        }
        Dialog::Find(form) => {
            let (mut select, mut replace, mut close) = (false, false, false);
            let modal = egui::Modal::new(Id::new("find_replace")).show(ctx, |ui| {
                ui.set_width(380.0);
                ui.label(RichText::new("Find and Replace").strong().size(15.0));
                ui.add_space(8.0);
                egui::Grid::new("find_form").num_columns(2).spacing(vec2(12.0, 8.0)).show(ui, |ui| {
                    ui.label("Find what");
                    ui.add(TextEdit::singleline(&mut form.find).desired_width(250.0));
                    ui.end_row();
                    ui.label("Replace with");
                    ui.add(TextEdit::singleline(&mut form.replace).desired_width(250.0));
                    ui.end_row();
                });
                ui.checkbox(&mut form.match_case, "Match case");
                ui.label(RichText::new("Searches text, block attributes and dimension text.").color(theme::TEXT_DIM));
                if !form.message.is_empty() {
                    ui.label(&form.message);
                }
                ui.add_space(10.0);
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    close = ui.button("Close").clicked();
                    replace = ui.add_enabled(!form.find.is_empty(), egui::Button::new("Replace All")).clicked();
                    select = ui.add_enabled(!form.find.is_empty(), egui::Button::new("Find and Select")).clicked();
                });
            });
            if select {
                app.find_select();
            } else if replace {
                app.find_replace();
            } else if close || modal.should_close() {
                app.dialog = None;
            }
        }
        Dialog::Purge(form) => {
            let (mut purge, mut cancel) = (false, false);
            let modal = egui::Modal::new(Id::new("purge")).show(ctx, |ui| {
                ui.set_width(380.0);
                ui.label(RichText::new("Purge").strong().size(15.0));
                ui.label(RichText::new("Items nothing in the drawing uses.").color(theme::TEXT_DIM));
                ui.add_space(8.0);
                let empty = form.layers.is_empty() && form.styles.is_empty() && form.blocks.is_empty();
                egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    for (title, list) in
                        [("Layers", &mut form.layers), ("Dimension styles", &mut form.styles), ("Blocks", &mut form.blocks)]
                    {
                        if list.is_empty() {
                            continue;
                        }
                        ui.label(RichText::new(title).strong());
                        for (name, on) in list.iter_mut() {
                            ui.checkbox(on, name.as_str());
                        }
                        ui.add_space(6.0);
                    }
                    if empty {
                        ui.label("Nothing is unused: there is nothing to purge.");
                    }
                });
                ui.add_space(10.0);
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    cancel = ui.button("Close").clicked();
                    purge = ui.add_enabled(!empty, egui::Button::new("Purge Checked Items")).clicked();
                });
            });
            if purge {
                app.purge();
            } else if cancel || modal.should_close() {
                app.dialog = None;
            }
        }
        Dialog::Units(form) => {
            let (mut accept, mut cancel) = (false, false);
            let modal = egui::Modal::new(Id::new("units")).show(ctx, |ui| {
                ui.set_width(320.0);
                ui.label(RichText::new("Drawing Units").strong().size(15.0));
                ui.add_space(8.0);
                egui::Grid::new("units_form").num_columns(2).spacing(vec2(12.0, 8.0)).show(ui, |ui| {
                    ui.label("Insertion scale");
                    let shown = UNIT_CHOICES.iter().find(|(c, _)| *c == form.units).map_or("Unitless", |(_, n)| n);
                    egui::ComboBox::from_id_salt("units_choice").width(160.0).selected_text(shown).show_ui(ui, |ui| {
                        for (code, name) in UNIT_CHOICES {
                            ui.selectable_value(&mut form.units, code, name);
                        }
                    });
                    ui.end_row();
                    ui.label("Precision");
                    egui::ComboBox::from_id_salt("units_precision")
                        .width(160.0)
                        .selected_text(format!("{:.*}", form.precision, 0.0))
                        .show_ui(ui, |ui| {
                            for places in 0..=8 {
                                ui.selectable_value(&mut form.precision, places, format!("{:.*}", places, 0.0));
                            }
                        });
                    ui.end_row();
                });
                ui.add_space(10.0);
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    accept = ui.button("OK").clicked();
                });
            });
            if accept || ctx.input(|i| i.key_pressed(Key::Enter)) {
                app.set_units();
            } else if cancel || modal.should_close() {
                app.dialog = None;
            }
        }
        Dialog::TextStyles(form) => {
            let (mut accept, mut cancel) = (false, false);
            let modal = egui::Modal::new(Id::new("text_styles")).show(ctx, |ui| {
                ui.set_width(520.0);
                ui.label(RichText::new("Text Style").strong().size(15.0));
                ui.add_space(8.0);
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(170.0);
                        egui::ScrollArea::vertical().id_salt("text_style_list").max_height(180.0).show(ui, |ui| {
                            for index in 0..form.styles.len() {
                                let name = form.styles[index].name.clone();
                                let shown = if name == form.current { format!("{name} (current)") } else { name };
                                ui.selectable_value(&mut form.selected, index, shown);
                            }
                        });
                        ui.add_space(6.0);
                        if ui.button("Set Current").clicked() {
                            form.current = form.styles[form.selected].name.clone();
                        }
                        ui.add_space(6.0);
                        ui.add(TextEdit::singleline(&mut form.new_name).hint_text("New style name").desired_width(160.0));
                        if ui.button("New").clicked() {
                            let name = form.new_name.trim().to_string();
                            if name.is_empty() {
                                form.error = "Type a name for the new style.".into();
                            } else if form.styles.iter().any(|s| s.name.eq_ignore_ascii_case(&name)) {
                                form.error = format!("There is already a style called {name}.");
                            } else {
                                let copy = crate::model::document::TextStyleDef { name, ..form.styles[form.selected].clone() };
                                form.styles.push(copy);
                                form.selected = form.styles.len() - 1;
                                form.new_name.clear();
                                form.error.clear();
                            }
                        }
                    });
                    ui.add_space(12.0);
                    let style = &mut form.styles[form.selected];
                    egui::Grid::new("text_style_fields").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
                        ui.label("Font");
                        let shown = if style.font.is_empty() { "(interface font)" } else { style.font.as_str() };
                        egui::ComboBox::from_id_salt("text_style_font").width(200.0).height(320.0).selected_text(shown).show_ui(ui, |ui| {
                            for font in &form.fonts {
                                ui.selectable_value(&mut style.font, font.clone(), font);
                            }
                        });
                        ui.end_row();
                        ui.label("Height (0 asks each time)");
                        ui.add(egui::DragValue::new(&mut style.height).speed(0.05).range(0.0..=f64::MAX).max_decimals(4));
                        ui.end_row();
                        ui.label("Width factor");
                        ui.add(egui::DragValue::new(&mut style.width_factor).speed(0.01).range(0.01..=100.0).max_decimals(4));
                        ui.end_row();
                        ui.label("Oblique angle");
                        ui.add(egui::DragValue::new(&mut style.oblique).speed(0.5).range(-85.0..=85.0).max_decimals(2));
                        ui.end_row();
                    });
                });
                if !form.error.is_empty() {
                    ui.label(RichText::new(&form.error).color(theme::ERROR));
                }
                ui.add_space(10.0);
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    accept = ui.button("OK").clicked();
                });
            });
            if accept {
                app.set_text_styles();
            } else if cancel || modal.should_close() {
                app.dialog = None;
            }
        }
        Dialog::Mtext(form) => {
            let (mut accept, mut cancel) = (false, false);
            let modal = egui::Modal::new(Id::new("mtext_editor")).show(ctx, |ui| {
                ui.set_width(640.0);
                ui.label(RichText::new("Text Editor").strong().size(15.0));
                ui.add_space(6.0);
                // What the toolbar shows: the look at the selection's start, or of what is typed next.
                let shown = if form.selection.0 < form.selection.1 {
                    form.styles.get(form.selection.0).cloned().unwrap_or_default()
                } else {
                    form.current.clone()
                };
                ui.horizontal(|ui| {
                    if ui.selectable_label(shown.bold, RichText::new("B").strong()).on_hover_text("Bold").clicked() {
                        form.apply(|s| s.bold = !shown.bold);
                    }
                    if ui.selectable_label(shown.italic, RichText::new("I").italics()).on_hover_text("Italic").clicked() {
                        form.apply(|s| s.italic = !shown.italic);
                    }
                    if ui.selectable_label(shown.underline, RichText::new("U").underline()).on_hover_text("Underline").clicked() {
                        form.apply(|s| s.underline = !shown.underline);
                    }
                    if ui.selectable_label(shown.stack, "a/b").on_hover_text("Stack the selection: 1/2 with a bar, 1^2 without").clicked() {
                        let to = !shown.stack;
                        form.apply(|s| s.stack = to);
                    }
                    ui.add_space(8.0);
                    let mut font = shown.font.clone();
                    egui::ComboBox::from_id_salt("mtext_font")
                        .width(160.0)
                        .height(320.0)
                        .selected_text(font.clone().unwrap_or_else(|| "Style's font".into()))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut font, None, "Style's font");
                            for name in &form.fonts {
                                ui.selectable_value(&mut font, Some(name.clone()), name);
                            }
                        });
                    if font != shown.font {
                        form.apply(|s| s.font = font.clone());
                    }
                    let mut height = shown.height * form.height;
                    ui.label("Height");
                    if ui.add(egui::DragValue::new(&mut height).speed(0.05).range(0.01..=f64::MAX).max_decimals(3)).changed() {
                        let factor = height / form.height;
                        form.apply(|s| s.height = factor);
                    }
                    let mut color = shown.color;
                    egui::ComboBox::from_id_salt("mtext_color")
                        .width(100.0)
                        .selected_text(swatch_text(Some(color.unwrap_or(BY_LAYER)), &color.map_or("ByLayer".to_string(), color_name)))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut color, None, "ByLayer");
                            for (aci, name) in NAMED_COLORS.iter().take(7) {
                                ui.selectable_value(&mut color, Some(*aci), swatch_text(Some(*aci), name));
                            }
                        });
                    if color != shown.color {
                        form.apply(|s| s.color = color);
                    }
                });
                ui.add_space(6.0);
                // The text shows each character's look; bold shows brighter, as the editor has no bold font here.
                let styles = form.styles.clone();
                let base = 15.0_f32;
                let mut layouter = move |ui: &egui::Ui, text: &str, wrap: f32| {
                    let mut job = LayoutJob::default();
                    job.wrap.max_width = wrap;
                    let mut buffer = [0u8; 4];
                    for (index, c) in text.chars().enumerate() {
                        let style = styles.get(index).cloned().unwrap_or_default();
                        let color = style
                            .color
                            .and_then(color_rgb)
                            .map_or(if style.bold { Color32::WHITE } else { theme::TEXT }, |(r, g, b)| Color32::from_rgb(r, g, b));
                        let format = egui::TextFormat {
                            font_id: FontId::proportional(
                                base * (style.height as f32).clamp(0.5, 3.0) * if style.stack { 0.75 } else { 1.0 },
                            ),
                            color,
                            italics: style.italic,
                            underline: if style.underline { Stroke::new(1.0_f32, color) } else { Stroke::NONE },
                            ..Default::default()
                        };
                        job.append(c.encode_utf8(&mut buffer), 0.0, format);
                    }
                    ui.fonts(|f| f.layout_job(job))
                };
                let before = form.text.clone();
                let output = TextEdit::multiline(&mut form.text)
                    .id(Id::new("mtext_editor_text"))
                    .desired_width(f32::INFINITY)
                    .desired_rows(8)
                    .layouter(&mut layouter)
                    .show(ui);
                if form.text != before {
                    form.follow_edit(&before);
                }
                if let Some(range) = output.cursor_range {
                    let (a, b) = (range.primary.ccursor.index, range.secondary.ccursor.index);
                    form.selection = (a.min(b), a.max(b));
                    if form.text != before {
                        form.selection = (a, a);
                    }
                }
                ui.add_space(8.0);
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    accept = ui.button("OK").clicked();
                });
            });
            if accept {
                app.mtext_accepted();
            } else if cancel || modal.should_close() {
                app.dialog = None;
            }
        }
        Dialog::MleaderStyles(form) => {
            let (mut accept, mut cancel) = (false, false);
            let modal = egui::Modal::new(Id::new("mleader_styles")).show(ctx, |ui| {
                ui.set_width(460.0);
                ui.label(RichText::new("Multileader Style Manager").strong().size(15.0));
                ui.add_space(8.0);
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(170.0);
                        for index in 0..form.styles.len() {
                            let name = form.styles[index].name.clone();
                            let shown = if name == form.current { format!("{name} (current)") } else { name };
                            ui.selectable_value(&mut form.selected, index, shown);
                        }
                        ui.add_space(6.0);
                        if ui.button("Set Current").clicked() {
                            form.current = form.styles[form.selected].name.clone();
                        }
                        ui.add_space(6.0);
                        ui.add(TextEdit::singleline(&mut form.new_name).hint_text("New style name").desired_width(160.0));
                        if ui.button("New").clicked() {
                            let name = form.new_name.trim().to_string();
                            if name.is_empty() {
                                form.error = "Type a name for the new style.".into();
                            } else if form.styles.iter().any(|s| s.name.eq_ignore_ascii_case(&name)) {
                                form.error = format!("There is already a style called {name}.");
                            } else {
                                let copy = MleaderStyle { name, ..form.styles[form.selected].clone() };
                                form.styles.push(copy);
                                form.selected = form.styles.len() - 1;
                                form.new_name.clear();
                                form.error.clear();
                            }
                        }
                    });
                    ui.add_space(12.0);
                    let style = &mut form.styles[form.selected];
                    egui::Grid::new("mleader_style_fields").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
                        for (name, value) in [
                            ("Text height", &mut style.text_height),
                            ("Arrow size", &mut style.arrow_size),
                            ("Dogleg length", &mut style.dogleg),
                            ("Landing gap", &mut style.landing_gap),
                        ] {
                            ui.label(name);
                            ui.add(egui::DragValue::new(value).speed(0.01).range(0.0..=f64::MAX).max_decimals(6));
                            ui.end_row();
                        }
                    });
                });
                if !form.error.is_empty() {
                    ui.label(RichText::new(&form.error).color(theme::ERROR));
                }
                ui.add_space(10.0);
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    accept = ui.button("OK").clicked();
                });
            });
            if accept {
                app.set_mleader_styles();
            } else if cancel || modal.should_close() {
                app.dialog = None;
            }
        }
        Dialog::MlineStyles(form) => {
            let (mut accept, mut cancel) = (false, false);
            let modal = egui::Modal::new(Id::new("mline_styles")).show(ctx, |ui| {
                ui.set_width(620.0);
                ui.label(RichText::new("Multiline Style").strong().size(15.0));
                ui.add_space(8.0);
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(170.0);
                        ui.label(RichText::new("Styles").color(theme::TEXT_DIM));
                        for index in 0..form.styles.len() {
                            let name = form.styles[index].name.clone();
                            let shown = if name == form.current { format!("{name} (current)") } else { name };
                            if ui.selectable_value(&mut form.selected, index, shown).clicked() {
                                form.element = 0;
                            }
                        }
                        ui.add_space(6.0);
                        if ui.button("Set Current").clicked() {
                            form.current = form.styles[form.selected].name.clone();
                        }
                        ui.add_space(6.0);
                        ui.add(TextEdit::singleline(&mut form.new_name).hint_text("New style name").desired_width(160.0));
                        ui.horizontal(|ui| {
                            if ui.button("New").clicked() {
                                let name = form.new_name.trim().to_string();
                                if name.is_empty() {
                                    form.error = "Type a name for the new style.".into();
                                } else if name.len() > 31 || name.contains(char::is_whitespace) {
                                    form.error = "A style name has up to 31 characters and no spaces.".into();
                                } else if form.styles.iter().any(|s| s.name.eq_ignore_ascii_case(&name)) {
                                    form.error = format!("There is already a style called {name}.");
                                } else {
                                    let copy = MlineStyle { name, ..form.styles[form.selected].clone() };
                                    form.styles.push(copy);
                                    form.selected = form.styles.len() - 1;
                                    form.new_name.clear();
                                    form.error.clear();
                                }
                            }
                            if ui.button("Delete").clicked() {
                                let name = form.styles[form.selected].name.clone();
                                if name.eq_ignore_ascii_case("Standard") {
                                    form.error = "The Standard style cannot be deleted.".into();
                                } else if name == form.current {
                                    form.error = "The current style cannot be deleted. Set another one current first.".into();
                                } else if form.in_use.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
                                    form.error = format!("{name} is used by multilines in the drawing, so it cannot be deleted.");
                                } else {
                                    form.styles.remove(form.selected);
                                    form.selected = form.selected.saturating_sub(1);
                                    form.element = 0;
                                    form.error.clear();
                                }
                            }
                        });
                    });
                    ui.add_space(12.0);
                    ui.vertical(|ui| mline_style_fields(ui, &mut form.styles[form.selected], &mut form.element));
                });
                if !form.error.is_empty() {
                    ui.label(RichText::new(&form.error).color(theme::ERROR));
                }
                ui.add_space(10.0);
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    accept = ui.button("OK").clicked();
                });
            });
            if accept {
                app.set_mline_styles();
            } else if cancel || modal.should_close() {
                app.dialog = None;
            }
        }
        Dialog::Lineweight(form) => {
            let (mut accept, mut cancel) = (false, false);
            let modal = egui::Modal::new(Id::new("lineweight_settings")).show(ctx, |ui| {
                ui.set_width(420.0);
                ui.label(RichText::new("Lineweight Settings").strong().size(15.0));
                ui.add_space(8.0);
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Lineweights").color(theme::TEXT_DIM));
                        Frame::new().stroke(Stroke::new(1.0_f32, theme::BORDER)).inner_margin(Margin::same(4)).show(ui, |ui| {
                            egui::ScrollArea::vertical().max_height(260.0).min_scrolled_height(260.0).show(ui, |ui| {
                                ui.set_width(170.0);
                                for value in lineweight::choices() {
                                    if lineweight_choice(ui, value, form.current == value).clicked() {
                                        form.current = value;
                                    }
                                }
                            });
                        });
                        ui.label(RichText::new(format!("Current: {}", lineweight::name(form.current))).color(theme::TEXT_DIM));
                    });
                    ui.add_space(12.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Units for listing").color(theme::TEXT_DIM));
                        ui.label("Millimeters (mm)");
                        ui.add_space(10.0);
                        ui.checkbox(&mut form.show, "Display lineweight");
                        ui.add_space(10.0);
                        ui.label(RichText::new("Default").color(theme::TEXT_DIM));
                        egui::ComboBox::from_id_salt("lineweight_default")
                            .width(150.0)
                            .selected_text(lineweight::name(form.default))
                            .show_ui(ui, |ui| {
                                for value in lineweight::STANDARD {
                                    if lineweight_choice(ui, value, form.default == value).clicked() {
                                        form.default = value;
                                    }
                                }
                            });
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new("Objects and layers set to Default\nshow and plot with this.").color(theme::TEXT_DIM).size(12.0),
                        );
                    });
                });
                ui.add_space(10.0);
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    accept = ui.button("OK").clicked();
                });
            });
            if accept || ctx.input(|i| i.key_pressed(Key::Enter)) {
                app.set_lineweights();
            } else if cancel || modal.should_close() {
                app.dialog = None;
            }
        }
        Dialog::Overkill(form) => {
            let (mut accept, mut cancel) = (false, false);
            let modal = egui::Modal::new(Id::new("delete_duplicates")).show(ctx, |ui| {
                ui.set_width(400.0);
                ui.label(RichText::new("Delete Duplicate Objects").strong().size(15.0));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label("Tolerance");
                    ui.add(TextEdit::singleline(&mut form.tolerance).desired_width(120.0));
                });
                ui.add_space(8.0);
                ui.label(RichText::new("Ignore object property").color(theme::TEXT_DIM));
                ui.horizontal(|ui| {
                    ui.checkbox(&mut form.settings.ignore_color, "Color");
                    ui.checkbox(&mut form.settings.ignore_layer, "Layer");
                    ui.checkbox(&mut form.settings.ignore_linetype, "Linetype");
                    ui.checkbox(&mut form.settings.ignore_lineweight, "Lineweight");
                });
                ui.add_space(8.0);
                ui.label(RichText::new("Options").color(theme::TEXT_DIM));
                ui.checkbox(&mut form.settings.optimize_polylines, "Optimize segments within polylines");
                ui.checkbox(&mut form.settings.combine_partial, "Combine co-linear objects that partially overlap");
                ui.checkbox(&mut form.settings.combine_end_to_end, "Combine co-linear objects when aligned end to end");
                if !form.error.is_empty() {
                    ui.add_space(4.0);
                    ui.label(RichText::new(&form.error).color(theme::ERROR));
                }
                ui.add_space(10.0);
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    cancel = ui.button("Cancel").clicked();
                    accept = ui.button("OK").clicked();
                });
            });
            if accept || ctx.input(|i| i.key_pressed(Key::Enter)) {
                match parse_length(&form.tolerance) {
                    Some(tolerance) => {
                        let settings = OverkillSettings { tolerance, ..form.settings.clone() };
                        app.set_overkill(settings);
                    }
                    None => form.error = "Enter a tolerance, zero or greater.".into(),
                }
            } else if cancel || modal.should_close() {
                app.cancel_overkill();
            }
        }
        Dialog::SnapSettings => {
            let snap = &mut app.controller.snap;
            let mut close = false;
            let modal = egui::Modal::new(Id::new("drafting_settings")).show(ctx, |ui| {
                ui.set_width(380.0);
                ui.label(RichText::new("Drafting Settings").strong().size(15.0));
                ui.add_space(8.0);
                ui.checkbox(&mut snap.osnap, "Object Snap On (F3)");
                ui.checkbox(&mut snap.tracking, "Object Snap Tracking On (F11)");
                ui.add_space(6.0);
                ui.label(RichText::new("Object snap modes").color(theme::TEXT_DIM));
                egui::Grid::new("snap_modes").num_columns(2).spacing(vec2(24.0, 6.0)).show(ui, |ui| {
                    for (index, kind) in SnapKind::ALL.iter().enumerate() {
                        let mut on = snap.kinds.contains(kind);
                        if ui.checkbox(&mut on, kind.name()).changed() {
                            if on {
                                snap.kinds.push(*kind);
                            } else {
                                snap.kinds.retain(|k| k != kind);
                            }
                        }
                        if index % 2 == 1 {
                            ui.end_row();
                        }
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button("Select All").clicked() {
                        snap.kinds = SnapKind::ALL.to_vec();
                    }
                    if ui.button("Clear All").clicked() {
                        snap.kinds.clear();
                    }
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.checkbox(&mut snap.polar, "Polar Tracking (F10), every");
                    egui::ComboBox::from_id_salt("polar_increment").selected_text(format!("{}°", snap.polar_increment)).show_ui(ui, |ui| {
                        for angle in [5.0, 10.0, 15.0, 18.0, 22.5, 30.0, 45.0, 90.0] {
                            ui.selectable_value(&mut snap.polar_increment, angle, format!("{angle}°"));
                        }
                    });
                });
                ui.checkbox(&mut snap.dynamic, "Dynamic Input (F12)");
                ui.add_space(10.0);
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| close = ui.button("Close").clicked());
            });
            if close || modal.should_close() || ctx.input(|i| i.key_pressed(Key::Enter)) {
                app.dialog = None;
                app.controller.reapply_cursor();
            }
        }
        Dialog::Color(form) => {
            let (accepted, cancelled, closed) = select_color(ctx, form);
            if accepted {
                app.color_chosen();
            } else if cancelled || closed {
                app.dialog = None;
            }
        }
        Dialog::RenameLayout { index, name, error } => {
            let (index, mut accepted, mut cancelled) = (*index, false, false);
            let modal = egui::Modal::new(Id::new("rename_layout")).show(ctx, |ui| {
                ui.set_width(320.0);
                ui.label(RichText::new("Rename Layout").strong().size(15.0));
                ui.add_space(8.0);
                let field = ui.add(TextEdit::singleline(name).desired_width(280.0));
                field.request_focus();
                if !error.is_empty() {
                    ui.label(RichText::new(error.as_str()).color(theme::ERROR));
                }
                ui.add_space(10.0);
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    cancelled = ui.button("Cancel").clicked();
                    accepted = ui.button("OK").clicked();
                });
            });
            if accepted || ctx.input(|i| i.key_pressed(Key::Enter)) {
                let name = name.clone();
                match app.controller.rename_layout(index, &name) {
                    Ok(()) => app.dialog = None,
                    Err(message) => {
                        if let Some(Dialog::RenameLayout { error, .. }) = &mut app.dialog {
                            *error = message;
                        }
                    }
                }
            } else if cancelled || modal.should_close() {
                app.dialog = None;
            }
        }
        Dialog::DimensionStyles(form) => {
            let (accepted, cancelled, should_close) = dimension_style_manager(ctx, form);
            if accepted {
                app.dimension_styles_accepted();
            } else if cancelled || should_close {
                app.dialog = None;
            }
        }
    }
}

/// DIMSTYLE: the drawing's dimension styles on the left; the chosen one's sizes and units on the right.
/// Returns whether OK or Cancel was pressed, or the dialog was dismissed.
fn dimension_style_manager(ctx: &egui::Context, form: &mut StyleForm) -> (bool, bool, bool) {
    let mut accepted = false;
    let mut cancelled = false;
    let modal = egui::Modal::new(Id::new("dimension_styles")).show(ctx, |ui| {
        ui.set_width(560.0);
        ui.label(RichText::new("Dimension Style Manager").strong().size(15.0));
        ui.label(RichText::new(format!("Current dimension style: {}", form.current)).color(theme::TEXT_DIM));
        ui.add_space(8.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(170.0);
                ui.label(RichText::new("Styles").color(theme::TEXT_DIM));
                egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                    for (index, style) in form.styles.iter().enumerate() {
                        let current = style.name.eq_ignore_ascii_case(&form.current);
                        let label = if current { RichText::new(&style.name).strong() } else { RichText::new(&style.name) };
                        if ui.selectable_label(form.selected == index, label).clicked() {
                            form.selected = index;
                            form.error.clear();
                        }
                    }
                });
                ui.add_space(6.0);
                if ui.button("Set Current").clicked() {
                    form.current = form.styles[form.selected].name.clone();
                }
                let selected_name = form.styles[form.selected].name.clone();
                let deletable = !selected_name.eq_ignore_ascii_case(&form.current)
                    && !selected_name.eq_ignore_ascii_case(STANDARD_STYLE)
                    && !form.used.contains(&selected_name.to_lowercase());
                if ui
                    .add_enabled(deletable, egui::Button::new("Delete"))
                    .on_disabled_hover_text("The current style, Standard and styles in use stay.")
                    .clicked()
                {
                    form.styles.remove(form.selected);
                    form.selected = form.selected.min(form.styles.len() - 1);
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add(TextEdit::singleline(&mut form.new_name).hint_text("New style name").desired_width(110.0));
                    if ui.button("New").clicked() {
                        let name = form.new_name.trim().to_string();
                        if name.is_empty() {
                            form.error = "Type a name for the new style.".into();
                        } else if form.styles.iter().any(|s| s.name.eq_ignore_ascii_case(&name)) {
                            form.error = format!("There is already a style named {name}.");
                        } else {
                            let mut style = form.styles[form.selected].clone();
                            style.name = name.clone();
                            style.look.style = name;
                            form.styles.push(style);
                            form.selected = form.styles.len() - 1;
                            form.new_name.clear();
                            form.error.clear();
                        }
                    }
                });
            });
            ui.add_space(16.0);
            ui.vertical(|ui| {
                let style = &mut form.styles[form.selected];
                ui.label(RichText::new(format!("Modify: {}", style.name)).strong());
                ui.add_space(4.0);
                let look = &mut style.look;
                let size = |ui: &mut egui::Ui, value: &mut f64| {
                    ui.add(egui::DragValue::new(value).speed(0.01).range(0.0..=f64::MAX).max_decimals(6));
                };
                egui::Grid::new("dimension_style_fields").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
                    ui.label("Text height");
                    size(ui, &mut look.text_height);
                    ui.end_row();
                    ui.label("Arrow size");
                    size(ui, &mut look.arrow_size);
                    ui.end_row();
                    ui.label("Arrowhead");
                    egui::ComboBox::from_id_salt("arrowhead").selected_text(arrowhead_name(look.arrowhead)).show_ui(ui, |ui| {
                        for arrowhead in [Arrowhead::Filled, Arrowhead::Tick, Arrowhead::Dot] {
                            ui.selectable_value(&mut look.arrowhead, arrowhead, arrowhead_name(arrowhead));
                        }
                    });
                    ui.end_row();
                    ui.label("Overall scale (DIMSCALE)");
                    size(ui, &mut look.scale);
                    ui.end_row();
                    ui.label("Tolerance");
                    egui::ComboBox::from_id_salt("dimension_tolerance").selected_text(look.tolerance.name()).show_ui(ui, |ui| {
                        for tolerance in Tolerance::ALL {
                            ui.selectable_value(&mut look.tolerance, tolerance, tolerance.name());
                        }
                    });
                    ui.end_row();
                    if look.tolerance != Tolerance::None {
                        ui.label("Upper value (DIMTP)");
                        size(ui, &mut look.tolerance_plus);
                        ui.end_row();
                        if look.tolerance != Tolerance::Symmetric {
                            ui.label("Lower value (DIMTM)");
                            size(ui, &mut look.tolerance_minus);
                            ui.end_row();
                        }
                        ui.label("Tolerance height (DIMTFAC)");
                        size(ui, &mut look.tolerance_height);
                        ui.end_row();
                    }
                    ui.label("Extend beyond dimension line");
                    size(ui, &mut look.extension_overshoot);
                    ui.end_row();
                    ui.label("Offset from origin");
                    size(ui, &mut look.extension_gap);
                    ui.end_row();
                    ui.label("Gap around text");
                    size(ui, &mut look.text_gap);
                    ui.end_row();
                    ui.label("Decimal places");
                    ui.add(egui::DragValue::new(&mut look.decimals).range(0..=8));
                    ui.end_row();
                    ui.label("Angle decimal places");
                    ui.add(egui::DragValue::new(&mut look.angle_decimals).range(0..=8));
                    ui.end_row();
                    ui.label("Measurement scale (DIMLFAC)");
                    size(ui, &mut look.length_factor);
                    ui.end_row();
                    ui.label("Trailing zeros");
                    ui.checkbox(&mut look.trim_zeros, "Leave off");
                    ui.end_row();
                });
                if look.scale <= 0.0 {
                    look.scale = 1.0;
                }
                if look.length_factor <= 0.0 {
                    look.length_factor = 1.0;
                }
            });
        });
        if !form.error.is_empty() {
            ui.label(RichText::new(&form.error).color(theme::ERROR));
        }
        ui.add_space(10.0);
        ui.label(
            RichText::new("Dimensions that follow a changed style change with it, except what was set on them.").color(theme::TEXT_DIM),
        );
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            cancelled = ui.button("Cancel").clicked();
            accepted = ui.button("OK").clicked();
        });
    });
    (accepted, cancelled, modal.should_close())
}

const SWATCH: f32 = 18.0;

/// One clickable color square, outlined when it is the chosen one.
fn color_swatch(ui: &mut egui::Ui, color: i32, chosen: &mut i32, size: f32) {
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    let (r, g, b) = color_rgb(color).unwrap_or((0, 0, 0));
    ui.painter().rect_filled(rect.shrink(1.0), CornerRadius::same(2), Color32::from_rgb(r, g, b));
    if *chosen == color {
        ui.painter().rect_stroke(rect, CornerRadius::same(3), Stroke::new(2.0_f32, theme::TEXT), StrokeKind::Inside);
    } else if response.hovered() {
        ui.painter().rect_stroke(rect, CornerRadius::same(3), Stroke::new(1.0_f32, theme::TEXT_DIM), StrokeKind::Inside);
    }
    let response = response.on_hover_text(color_name(color));
    if response.clicked() {
        *chosen = color;
    }
}

/// AutoCAD's Select Color dialog: the 255 index colors, or any color by red, green and blue. Gives whether OK or
/// Cancel was pressed, or the dialog was dismissed.
fn select_color(ctx: &egui::Context, form: &mut ColorForm) -> (bool, bool, bool) {
    let (mut accepted, mut cancelled) = (false, false);
    let before = form.color;
    let modal = egui::Modal::new(Id::new("select_color")).show(ctx, |ui| {
        ui.set_width(24.0 * SWATCH + 8.0);
        ui.label(RichText::new("Select Color").strong().size(15.0));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.selectable_value(&mut form.true_color_tab, false, "Index Color");
            ui.selectable_value(&mut form.true_color_tab, true, "True Color");
        });
        ui.separator();
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        if form.true_color_tab {
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            let (mut r, mut g, mut b) = color_rgb(form.color).unwrap_or((255, 255, 255));
            let mut moved = false;
            egui::Grid::new("true_color").num_columns(2).spacing(vec2(12.0, 8.0)).show(ui, |ui| {
                for (name, value) in [("Red", &mut r), ("Green", &mut g), ("Blue", &mut b)] {
                    ui.label(name);
                    moved |= ui.add(egui::Slider::new(value, 0..=255)).changed();
                    ui.end_row();
                }
            });
            if moved {
                form.color = true_color(r, g, b);
            }
        } else {
            // 10 to 249 as AutoCAD lays them out: a column per hue, its stronger shades over its paler ones.
            for row in [0, 2, 4, 6, 8, 1, 3, 5, 7, 9] {
                ui.horizontal(|ui| {
                    for hue in 0..24 {
                        color_swatch(ui, 10 + hue * 10 + row, &mut form.color, SWATCH);
                    }
                });
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                for aci in 1..=9 {
                    color_swatch(ui, aci, &mut form.color, SWATCH * 1.4);
                    ui.add_space(3.0);
                }
                ui.add_space(SWATCH);
                for aci in 250..=255 {
                    color_swatch(ui, aci, &mut form.color, SWATCH * 1.4);
                    ui.add_space(3.0);
                }
            });
            if !matches!(form.target, ColorTarget::Layer(_)) {
                ui.add_space(10.0);
                ui.spacing_mut().item_spacing = vec2(8.0, 0.0);
                ui.horizontal(|ui| {
                    if ui.selectable_label(form.color == BY_LAYER, "ByLayer").clicked() {
                        form.color = BY_LAYER;
                    }
                    if ui.selectable_label(form.color == BY_BLOCK, "ByBlock").clicked() {
                        form.color = BY_BLOCK;
                    }
                });
            }
        }
        ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
        ui.add_space(12.0);
        if form.color != before {
            form.typed = color_name(form.color);
        }
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(44.0, 26.0), Sense::hover());
            match color_rgb(form.color) {
                Some((r, g, b)) => {
                    ui.painter().rect_filled(rect, CornerRadius::same(4), Color32::from_rgb(r, g, b));
                }
                None => {
                    ui.painter().rect_stroke(rect, CornerRadius::same(4), Stroke::new(1.0_f32, theme::TEXT_DIM), StrokeKind::Inside);
                }
            }
            ui.label("Color:");
            let field =
                ui.add(TextEdit::singleline(&mut form.typed).desired_width(160.0)).on_hover_text("A name, 1 to 255, 255,128,0 or #FF8000");
            if field.changed()
                && let Some(color) = parse_color(&form.typed)
                && !(matches!(form.target, ColorTarget::Layer(_)) && (color == BY_LAYER || color == BY_BLOCK))
            {
                form.color = color;
                form.true_color_tab = is_true_color(color);
            }
        });
        ui.add_space(10.0);
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            cancelled = ui.button("Cancel").clicked();
            accepted = ui.button("OK").clicked();
        });
    });
    (accepted || ctx.input(|i| i.key_pressed(Key::Enter)), cancelled, modal.should_close())
}

fn arrowhead_name(arrowhead: Arrowhead) -> &'static str {
    match arrowhead {
        Arrowhead::Filled => "Closed filled",
        Arrowhead::Tick => "Architectural tick",
        Arrowhead::Dot => "Dot",
    }
}
