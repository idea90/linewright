use std::cell::RefCell;
use std::rc::Rc;

use super::defaults::Defaults;
use super::prompts::{Command, Io};
use super::{
    blocks, curves, dimension, drafting, draw, edit, groups, inquiry, layer_tools, layout, measuring, modify, qdim, reshape, shapes,
};
use crate::model::entities::Entity;

/// Every command, in the order the toolbar shows them, with the label it shows.
pub const COMMANDS: &[(&str, &str)] = &[
    ("LINE", "Line"),
    ("PLINE", "Polyline"),
    ("RECTANGLE", "Rectangle"),
    ("CIRCLE", "Circle"),
    ("ARC", "Arc"),
    ("TEXT", "Text"),
    ("ERASE", "Erase"),
    ("MOVE", "Move"),
    ("COPY", "Copy"),
    ("ROTATE", "Rotate"),
    ("SCALE", "Scale"),
    ("MIRROR", "Mirror"),
    ("OFFSET", "Offset"),
    ("TRIM", "Trim"),
    ("EXTEND", "Extend"),
    ("FILLET", "Fillet"),
    ("DIMLINEAR", "Dimension"),
    ("DIMALIGNED", "Aligned dimension"),
    ("DIMSCALE", "Dimension scale"),
    ("DRAWORDER", "Draw order"),
    ("TABLE", "Table"),
    ("TABLEDIT", "Edit table cell"),
    ("POLYGON", "Polygon"),
    ("XLINE", "Construction line"),
    ("RAY", "Ray"),
    ("DONUT", "Donut"),
    ("REVCLOUD", "Revision cloud"),
    ("WIPEOUT", "Wipeout"),
    ("MLINE", "Multiline"),
    ("MTEXT", "Multiline text"),
    ("DIVIDE", "Divide"),
    ("MEASURE", "Measure"),
    ("LENGTHEN", "Lengthen"),
    ("ALIGN", "Align"),
    ("PEDIT", "Edit polyline"),
    ("DIMCONTINUE", "Continue dimension"),
    ("DIMBASELINE", "Baseline dimension"),
    ("LAYISO", "Isolate layers"),
    ("LAYUNISO", "Unisolate layers"),
    ("LAYOFF", "Layer off"),
    ("LAYON", "All layers on"),
    ("LAYFRZ", "Freeze layer"),
    ("LAYTHW", "Thaw all layers"),
    ("LAYMCUR", "Make object's layer current"),
    ("LAYLCK", "Lock layer"),
    ("LAYULK", "Unlock layer"),
    ("GROUP", "Group"),
    ("UNGROUP", "Ungroup"),
    ("GROUPEDIT", "Group edit"),
    ("LAYOUT", "Layout"),
    ("MVIEW", "Viewport"),
    ("QDIM", "Quick dimension"),
    ("SPLINEDIT", "Edit spline"),
    ("GRADIENT", "Gradient fill"),
    ("HATCHEDIT", "Edit hatch"),
    ("MLEADER", "Multileader"),
    ("MLEADERADD", "Add leader to multileader"),
    ("MLEADERREMOVE", "Remove leader from multileader"),
    ("DIMORDINATE", "Ordinate dimension"),
    ("DIMARC", "Arc length dimension"),
    ("DIMJOGGED", "Jogged radius dimension"),
    ("DIMTEDIT", "Move dimension text"),
    ("VPLAYER", "Freeze layers in viewports"),
    ("DIMRADIUS", "Radius dimension"),
    ("DIMDIAMETER", "Diameter dimension"),
    ("DIMANGULAR", "Angular dimension"),
    ("QLEADER", "Leader"),
    ("LEADER", "Leader with text"),
    ("DIMTXT", "Dimension text height"),
    ("DIMASZ", "Dimension arrow size"),
    ("-DIMSTYLE", "Dimension style options"),
    ("DIMSTYLE", "Dimension style manager"),
    ("DISTANCE", "Distance"),
    ("AREA", "Area"),
    ("ELLIPSE", "Ellipse"),
    ("SPLINE", "Spline"),
    ("POINT", "Point"),
    ("HATCH", "Hatch"),
    ("BOUNDARY", "Boundary polyline"),
    ("EXPLODE", "Explode"),
    ("STRETCH", "Stretch"),
    ("ARRAY", "Array"),
    ("CHAMFER", "Chamfer"),
    ("BREAK", "Break"),
    ("JOIN", "Join"),
    ("OVERKILL", "Delete duplicate objects"),
    ("-OVERKILL", "Delete duplicate objects with command line options"),
    ("MATCHPROP", "Match properties"),
    ("INSERT", "Insert block"),
    ("BLOCK", "Create block"),
    ("PASTECLIP", "Paste"),
    ("LIST", "List"),
    ("ID", "Point coordinates"),
    ("ZOOM", "Zoom"),
    ("TEXTEDIT", "Edit text"),
];

pub const ALIASES: &[(&str, &str)] = &[
    ("L", "LINE"),
    ("PL", "PLINE"),
    ("POLYLINE", "PLINE"),
    ("REC", "RECTANGLE"),
    ("RECTANG", "RECTANGLE"),
    ("C", "CIRCLE"),
    ("A", "ARC"),
    ("T", "TEXT"),
    ("E", "ERASE"),
    ("M", "MOVE"),
    ("CO", "COPY"),
    ("CP", "COPY"),
    ("RO", "ROTATE"),
    ("SC", "SCALE"),
    ("MI", "MIRROR"),
    ("O", "OFFSET"),
    ("TR", "TRIM"),
    ("EX", "EXTEND"),
    ("F", "FILLET"),
    ("DLI", "DIMLINEAR"),
    ("DAL", "DIMALIGNED"),
    ("DS", "DIMSCALE"),
    ("G", "GROUP"),
    ("DR", "DRAWORDER"),
    ("ATT", "ATTDEF"),
    ("TB", "TABLE"),
    ("POL", "POLYGON"),
    ("XL", "XLINE"),
    ("DO", "DONUT"),
    ("ML", "MLINE"),
    ("DOUGHNUT", "DONUT"),
    ("MT", "MTEXT"),
    ("DIV", "DIVIDE"),
    ("ME", "MEASURE"),
    ("LEN", "LENGTHEN"),
    ("AL", "ALIGN"),
    ("PE", "PEDIT"),
    ("DCO", "DIMCONTINUE"),
    ("DBA", "DIMBASELINE"),
    ("LO", "LAYOUT"),
    ("MV", "MVIEW"),
    ("DOR", "DIMORDINATE"),
    ("SPE", "SPLINEDIT"),
    ("GD", "GRADIENT"),
    ("HE", "HATCHEDIT"),
    ("MLD", "MLEADER"),
    ("MLA", "MLEADERADD"),
    ("MLR", "MLEADERREMOVE"),
    ("DIMORD", "DIMORDINATE"),
    ("DAR", "DIMARC"),
    ("DJO", "DIMJOGGED"),
    ("JOG", "DIMJOGGED"),
    ("DIMTED", "DIMTEDIT"),
    ("DRA", "DIMRADIUS"),
    ("DDI", "DIMDIAMETER"),
    ("DAN", "DIMANGULAR"),
    ("LE", "QLEADER"),
    ("LEAD", "LEADER"),
    ("MLEADER", "QLEADER"),
    ("MLD", "QLEADER"),
    ("D", "DIMSTYLE"),
    ("DST", "DIMSTYLE"),
    ("DDIM", "DIMSTYLE"),
    ("DIMUPDATE", "-DIMSTYLE"),
    ("DI", "DISTANCE"),
    ("DIST", "DISTANCE"),
    ("AA", "AREA"),
    ("EL", "ELLIPSE"),
    ("SPL", "SPLINE"),
    ("PO", "POINT"),
    ("H", "HATCH"),
    ("BH", "HATCH"),
    ("BO", "BOUNDARY"),
    ("BPOLY", "BOUNDARY"),
    ("X", "EXPLODE"),
    ("S", "STRETCH"),
    ("AR", "ARRAY"),
    ("ARRAYRECT", "ARRAY"),
    ("ARRAYPOLAR", "ARRAY"),
    ("CHA", "CHAMFER"),
    ("BR", "BREAK"),
    ("J", "JOIN"),
    ("OV", "OVERKILL"),
    ("MA", "MATCHPROP"),
    ("PAINTER", "MATCHPROP"),
    ("I", "INSERT"),
    ("DDINSERT", "INSERT"),
    ("B", "BLOCK"),
    ("PASTE", "PASTECLIP"),
    ("LI", "LIST"),
    ("LS", "LIST"),
    ("Z", "ZOOM"),
    ("ED", "TEXTEDIT"),
    ("DDEDIT", "TEXTEDIT"),
    ("ATTEDIT", "TEXTEDIT"),
    ("ATE", "TEXTEDIT"),
];

pub fn label(name: &str) -> &'static str {
    COMMANDS.iter().chain(WINDOW_COMMANDS).find(|(command, _)| *command == name).map_or("", |(_, label)| label)
}

/// Builds the named command. Commands that edit get the objects already selected when they started; commands that
/// remember settings share `defaults`.
pub fn create(name: &str, selected: Vec<Entity>, defaults: Rc<RefCell<Defaults>>) -> impl FnOnce(Io) -> Command {
    let name = name.to_string();
    move |io| -> Command {
        match name.as_str() {
            "LINE" => Box::pin(draw::line_command(io)),
            "PLINE" => Box::pin(draw::polyline_command(io)),
            "RECTANGLE" => Box::pin(draw::rectangle_command(io)),
            "CIRCLE" => Box::pin(curves::circle_command(io)),
            "ARC" => Box::pin(curves::arc_command(io)),
            "TEXT" => Box::pin(draw::text_command(io, defaults)),
            "DISTANCE" => Box::pin(measuring::distance(io)),
            "AREA" => Box::pin(measuring::area(io)),
            "ERASE" => Box::pin(edit::erase(io, selected)),
            "MOVE" => Box::pin(edit::move_command(io, selected)),
            "COPY" => Box::pin(edit::copy(io, selected)),
            "ROTATE" => Box::pin(edit::rotate(io, selected)),
            "SCALE" => Box::pin(edit::scale(io, selected)),
            "MIRROR" => Box::pin(edit::mirror(io, selected)),
            "OFFSET" => Box::pin(edit::offset(io)),
            "TRIM" => Box::pin(edit::trim(io, selected)),
            "EXTEND" => Box::pin(edit::extend(io, selected)),
            "FILLET" => Box::pin(edit::fillet(io, defaults)),
            "DIMLINEAR" => Box::pin(dimension::linear(io, defaults)),
            "DIMALIGNED" => Box::pin(dimension::aligned(io, defaults)),
            "DIMSCALE" => Box::pin(dimension::set_scale(io, defaults)),
            "DIMTXT" => Box::pin(dimension::set_text_height(io, defaults)),
            "DRAWORDER" => Box::pin(modify::draworder_command(io, selected)),
            "TABLE" => Box::pin(drafting::table_command(io)),
            "TABLEDIT" => Box::pin(drafting::tabledit_command(io, defaults)),
            "ATTDEF" => Box::pin(blocks::attdef_command(io)),
            // IMAGEATTACH chooses the file in the window, then places it with this.
            "IMAGEPLACE" => Box::pin(drafting::place_image(io, defaults)),
            "POLYGON" => Box::pin(drafting::polygon_command(io)),
            "XLINE" => Box::pin(drafting::xline_command(io)),
            "RAY" => Box::pin(drafting::ray_command(io)),
            "DONUT" => Box::pin(drafting::donut_command(io)),
            "REVCLOUD" => Box::pin(drafting::revcloud_command(io, defaults)),
            "WIPEOUT" => Box::pin(drafting::wipeout_command(io)),
            "MLINE" => Box::pin(drafting::mline_command(io, defaults)),
            "MTEXT" | "-MTEXT" => Box::pin(drafting::mtext_command(io)),
            "MTEXTBOX" => Box::pin(drafting::mtext_box_command(io, defaults)),
            "DIVIDE" => Box::pin(drafting::divide_command(io)),
            "MEASURE" => Box::pin(drafting::measure_command(io)),
            "LENGTHEN" => Box::pin(reshape::lengthen_command(io, defaults)),
            "ALIGN" => Box::pin(reshape::align_command(io, selected)),
            "PEDIT" => Box::pin(reshape::pedit_command(io)),
            "DIMCONTINUE" => Box::pin(dimension::continue_dimension(io, defaults)),
            "DIMBASELINE" => Box::pin(dimension::baseline_dimension(io, defaults)),
            "LAYISO" => Box::pin(layer_tools::isolate(io, selected, defaults)),
            "LAYUNISO" => Box::pin(layer_tools::unisolate(io, defaults)),
            "LAYOFF" => Box::pin(layer_tools::layer_off(io, defaults)),
            "LAYON" => Box::pin(layer_tools::layer_on(io, defaults)),
            "LAYFRZ" => Box::pin(layer_tools::freeze(io, defaults)),
            "LAYTHW" => Box::pin(layer_tools::thaw(io, defaults)),
            "LAYMCUR" => Box::pin(layer_tools::make_current(io, defaults)),
            "LAYLCK" => Box::pin(layer_tools::lock(io, defaults)),
            "LAYULK" => Box::pin(layer_tools::unlock(io, defaults)),
            "GROUP" => Box::pin(groups::group(io, selected, defaults)),
            "UNGROUP" => Box::pin(groups::ungroup(io, selected, defaults)),
            "GROUPEDIT" => Box::pin(groups::groupedit(io, defaults)),
            "LAYOUT" => Box::pin(layout::layout(io, defaults)),
            "MVIEW" => Box::pin(layout::mview(io, defaults)),
            "QDIM" => Box::pin(qdim::qdim(io, selected, defaults)),
            "SPLINEDIT" => Box::pin(reshape::splinedit_command(io)),
            "GRADIENT" => Box::pin(shapes::gradient_command(io, selected)),
            "HATCHEDIT" => Box::pin(shapes::hatchedit_command(io, selected)),
            "MLEADER" => Box::pin(dimension::mleader(io, defaults)),
            "MLEADERADD" => Box::pin(dimension::mleader_add(io)),
            "MLEADERREMOVE" => Box::pin(dimension::mleader_remove(io)),
            "DIMORDINATE" => Box::pin(dimension::ordinate(io, defaults)),
            "DIMARC" => Box::pin(dimension::arc_length(io, defaults)),
            "DIMJOGGED" => Box::pin(dimension::jogged(io, defaults)),
            "DIMTEDIT" => Box::pin(dimension::text_edit(io, selected)),
            "VPLAYER" => Box::pin(layout::vplayer(io, defaults)),
            "DIMRADIUS" => Box::pin(dimension::radius(io, defaults)),
            "DIMDIAMETER" => Box::pin(dimension::diameter(io, defaults)),
            "DIMANGULAR" => Box::pin(dimension::angular(io, defaults)),
            "QLEADER" | "LEADER" => Box::pin(dimension::leader(io, defaults)),
            "DIMASZ" => Box::pin(dimension::set_arrow_size(io, defaults)),
            // The window opens the manager for DIMSTYLE; typed where it cannot, it runs as -DIMSTYLE.
            "-DIMSTYLE" | "DIMSTYLE" => Box::pin(dimension::style(io, selected, defaults)),
            "ELLIPSE" => Box::pin(shapes::ellipse_command(io)),
            "SPLINE" => Box::pin(shapes::spline_command(io)),
            "POINT" => Box::pin(shapes::point_command(io)),
            "HATCH" => Box::pin(shapes::hatch_command(io, selected, defaults)),
            "BOUNDARY" => Box::pin(shapes::boundary_command(io)),
            "EXPLODE" => Box::pin(modify::explode_command(io, selected)),
            "STRETCH" => Box::pin(modify::stretch_command(io)),
            "ARRAY" => Box::pin(modify::array_command(io, selected)),
            "CHAMFER" => Box::pin(modify::chamfer_command(io, defaults)),
            "BREAK" => Box::pin(modify::break_command(io)),
            "JOIN" => Box::pin(modify::join_command(io, selected)),
            "OVERKILL" => Box::pin(modify::overkill_dialog_command(io, selected, defaults)),
            "-OVERKILL" => Box::pin(modify::overkill_command(io, selected, defaults)),
            "MATCHPROP" => Box::pin(modify::matchprop_command(io)),
            "INSERT" => Box::pin(blocks::insert_command(io, defaults)),
            "BLOCK" => Box::pin(blocks::block_command(io, selected, defaults)),
            "PASTECLIP" => Box::pin(blocks::paste_command(io, defaults)),
            "LIST" => Box::pin(inquiry::list_command(io, selected)),
            "ID" => Box::pin(inquiry::id_command(io)),
            "ZOOM" => Box::pin(inquiry::zoom_command(io)),
            "TEXTEDIT" => Box::pin(inquiry::textedit_command(io, selected)),
            other => panic!("unknown command {other}"),
        }
    }
}

/// Drag a grip of a selected object, a command of its own while it runs.
pub fn grip(target: Entity, index: usize) -> impl FnOnce(Io) -> Command {
    move |io| -> Command { Box::pin(inquiry::grip_command(io, target, index)) }
}

/// Commands the window carries out rather than the drawing, so AutoComplete offers them too.
pub const WINDOW_COMMANDS: &[(&str, &str)] = &[
    ("QSAVE", "Save"),
    ("SAVEAS", "Save as"),
    ("OPEN", "Open drawing"),
    ("NEW", "New drawing"),
    ("PLOT", "Plot to PDF"),
    ("COLOR", "Color for new objects"),
    ("PICKSTYLE", "Group selection on or off"),
    ("OSNAP", "Drafting settings: object snaps"),
    ("QSELECT", "Quick select"),
    ("IMAGEATTACH", "Attach image"),
    ("ATTDEF", "Define attribute"),
    ("XATTACH", "Attach external reference"),
    ("BEDIT", "Block editor"),
    ("BCLOSE", "Close block editor"),
    ("XREF", "External references"),
    ("SELECTSIMILAR", "Select similar objects"),
    ("FIND", "Find and replace text"),
    ("PURGE", "Purge unused items"),
    ("UNITS", "Drawing units"),
    ("LWEIGHT", "Lineweight settings"),
    ("STYLE", "Text style manager"),
    ("MLEADERSTYLE", "Multileader style manager"),
    ("LWDISPLAY", "Show or hide lineweights"),
    ("MSPACE", "Model space through a viewport"),
    ("PSPACE", "Paper space"),
];

/// A command AutoComplete offers for what is typed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Suggestion {
    pub command: &'static str,
    pub label: &'static str,
    pub alias: &'static str,
}

/// AutoCAD's AutoComplete: the commands that what is typed may mean, best first. An alias typed in full comes
/// first, then commands and aliases that start with it (shorter first), then commands whose name or label holds it.
pub fn suggestions(typed: &str, limit: usize) -> Vec<Suggestion> {
    let typed = typed.trim().to_uppercase();
    if typed.is_empty() || typed.contains(char::is_whitespace) {
        return vec![];
    }
    let every = || COMMANDS.iter().chain(WINDOW_COMMANDS);
    let mut by_prefix: Vec<&str> = every().map(|(c, _)| *c).filter(|c| c.starts_with(&typed)).collect();
    by_prefix.extend(ALIASES.iter().filter(|(a, _)| a.starts_with(&typed)).map(|(_, c)| *c));
    by_prefix.sort_by_key(|c| (c.len(), *c));
    let exact = ALIASES.iter().filter(|(a, _)| *a == typed).map(|(_, c)| *c);
    let inside = every().filter(|(c, l)| c.contains(&typed) || l.to_uppercase().contains(&typed)).map(|(c, _)| *c);
    let mut seen = vec![];
    for command in exact.chain(by_prefix).chain(inside) {
        if !seen.contains(&command) {
            seen.push(command);
        }
    }
    seen.into_iter()
        .take(limit)
        .filter_map(|command| {
            let label = every().find(|(c, _)| *c == command)?.1;
            Some(Suggestion { command, label, alias: shortest_alias(command) })
        })
        .collect()
}

pub fn shortest_alias(name: &str) -> &'static str {
    ALIASES.iter().filter(|(_, command)| *command == name).map(|(alias, _)| *alias).min_by_key(|alias| alias.len()).unwrap_or("")
}

/// Canonical command name for a typed name or alias, or None.
pub fn resolve(word: &str) -> Option<&'static str> {
    let word = word.to_uppercase();
    let word = ALIASES.iter().find(|(alias, _)| *alias == word).map_or(word.as_str(), |(_, command)| command);
    COMMANDS.iter().find(|(command, _)| *command == word).map(|(command, _)| *command)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_aliases_resolve() {
        assert_eq!(resolve("l"), Some("LINE"));
        assert_eq!(resolve("Circle"), Some("CIRCLE"));
        assert_eq!(resolve("polyline"), Some("PLINE"));
        assert_eq!(resolve("nonsense"), None);
        assert_eq!(shortest_alias("COPY"), "CO");
        assert_eq!(label("DIMLINEAR"), "Dimension");
    }

    #[test]
    fn autocomplete_offers_aliases_then_prefixes_then_the_rest() {
        let names = |typed: &str| suggestions(typed, 8).into_iter().map(|s| s.command).collect::<Vec<_>>();
        assert_eq!(names("l")[0], "LINE", "an alias typed in full wins");
        let dim = names("dim");
        assert!(dim.starts_with(&["DIMTXT", "DIMASZ"]) || dim[0].starts_with("DIM"), "{dim:?}");
        assert!(dim.iter().all(|c| c.starts_with("DIM") || c.contains("DIM")));
        assert!(names("rad").contains(&"DIMRADIUS"), "found inside a name");
        assert!(names("sav").contains(&"QSAVE"), "window commands too");
        assert!(names("").is_empty());
        let first = suggestions("c", 8)[0];
        assert_eq!((first.command, first.alias), ("CIRCLE", "C"));
    }
}
