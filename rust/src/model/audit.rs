//! AUDIT and RECOVER: what is wrong with a drawing, and the changes that put it right. Objects that cannot be drawn,
//! and references to layers, linetypes, blocks and groups that the drawing does not have.

use std::collections::{BTreeMap, HashSet};

use super::document::{DEFAULT_LINETYPE, Document, Layer, ObjectGroup};
use super::entities::{DEFAULT_LAYER, Entity, EntityId, Shape};
use super::geometry::Point;
use super::parts::BYBLOCK_LINETYPE;
use super::styles::{BYLAYER_LINETYPE, LINETYPES};

/// Where a finding is: in one space of the drawing (None for model space), among the drawing's own tables, or in a
/// block definition.
#[derive(Clone, Debug, PartialEq)]
pub enum Place {
    Space(Option<usize>),
    Drawing,
    Block(String),
}

/// How a finding is put right.
#[derive(Clone, Debug, PartialEq)]
pub enum Fix {
    /// Delete the object.
    Delete(EntityId),
    /// Give the object the ByLayer linetype.
    LinetypeByLayer(EntityId),
    /// Give the text a height of 1.
    TextHeight(EntityId),
    /// Add the layer, with its default settings.
    CreateLayer(String),
    /// Give the layer the Continuous linetype.
    LayerLinetype(String),
    /// Make layer 0 the current layer.
    CurrentLayer,
    /// Take out the objects that are gone from the groups, and delete the groups left empty.
    Groups,
}

impl Fix {
    /// What the fix did, for the report.
    pub fn outcome(&self) -> &'static str {
        match self {
            Fix::Delete(_) => "deleted",
            Fix::LinetypeByLayer(_) => "linetype set to ByLayer",
            Fix::TextHeight(_) => "height set to 1",
            Fix::CreateLayer(_) => "layer created",
            Fix::LayerLinetype(_) => "linetype set to Continuous",
            Fix::CurrentLayer => "current layer set to 0",
            Fix::Groups => "groups repaired",
        }
    }
}

/// One thing AUDIT found: the kind of object, what is wrong with it, where it is, and how it is put right. `fix` is
/// None where a change could not be made, such as in a block definition.
#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    pub object: String,
    pub problem: String,
    pub place: Place,
    pub fix: Option<Fix>,
}

/// What a problem with a shape calls for: the object goes, or only its text height is set.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Remedy {
    Delete,
    Height,
}

/// The changes that put findings right, worked out from the drawing as it is. Objects in the lists are the drawing's
/// own, as they are now, and a finding whose object is already gone is left out.
#[derive(Clone, Debug, Default)]
pub struct Repairs {
    /// Objects to delete.
    pub removed: Vec<Entity>,
    /// Objects whose values change, keeping their identity.
    pub changed: Vec<Entity>,
    /// The layers, when they change.
    pub layers: Option<Vec<Layer>>,
    /// The current layer, when it changes.
    pub current_layer: Option<String>,
    /// The groups, when they change.
    pub groups: Option<Vec<ObjectGroup>>,
}

impl Repairs {
    /// Make the repairs in the drawing directly. Only the open space's objects can be deleted, so the caller opens
    /// the space the findings are in first.
    pub fn apply(&self, document: &mut Document) {
        let ids: Vec<EntityId> = self.removed.iter().map(|e| e.id).collect();
        document.remove_all(&ids);
        for entity in &self.changed {
            document.modify(entity);
        }
        if let Some(layers) = &self.layers {
            document.layers = layers.clone();
        }
        if let Some(current) = &self.current_layer {
            document.current_layer = current.clone();
        }
        if let Some(groups) = &self.groups {
            document.groups = groups.clone();
        }
    }
}

/// The spaces a drawing has: model space (None), then each layout.
fn spaces(document: &Document) -> Vec<Option<usize>> {
    std::iter::once(None).chain((0..document.layouts.len()).map(Some)).collect()
}

/// The linetypes the drawing has: Linewright's own, and those its file's linetype table defines.
fn known_linetypes(document: &Document) -> HashSet<String> {
    let mut known: HashSet<String> = LINETYPES.iter().map(|name| name.to_uppercase()).collect();
    known.insert(BYLAYER_LINETYPE.to_uppercase());
    known.insert(BYBLOCK_LINETYPE.to_uppercase());
    if let Some(source) = &document.source {
        known.extend(source.groups.iter().filter(|g| g.kind() == "LTYPE").filter_map(|g| g.get(2)).map(|n| n.trim().to_uppercase()));
    }
    known
}

/// Whether the drawing defines the block an insert refers to. Anonymous blocks belong to the objects using them.
fn block_defined(document: &Document, name: &str) -> bool {
    name.starts_with('*') || document.blocks.contains_key(&name.to_uppercase())
}

fn objects(count: usize) -> String {
    if count == 1 { "1 object".into() } else { format!("{count} objects") }
}

/// Every problem in the drawing: in every space, in the drawing's own tables and in each block definition. Objects in
/// blocks are only reported, since a change to a block could not be undone with the rest.
pub fn audit(document: &Document) -> Vec<Finding> {
    let known = known_linetypes(document);
    let mut findings = vec![];
    let mut missing_layers: BTreeMap<String, usize> = BTreeMap::new();
    for space in spaces(document) {
        for entity in document.space_entities(space) {
            check_entity(document, entity, &Place::Space(space), &known, &mut findings, &mut missing_layers);
        }
    }
    for block in document.blocks.values() {
        for entity in &block.0.entities {
            check_entity(document, entity, &Place::Block(block.0.name.clone()), &known, &mut findings, &mut missing_layers);
        }
    }
    for (name, count) in missing_layers {
        let problem = format!("is used by {} but is not defined", objects(count));
        findings.push(Finding { object: format!("Layer {name}"), problem, place: Place::Drawing, fix: Some(Fix::CreateLayer(name)) });
    }
    if document.layer(&document.current_layer).is_none() {
        let problem = format!("current layer {} does not exist", document.current_layer);
        findings.push(Finding { object: "Layer".into(), problem, place: Place::Drawing, fix: Some(Fix::CurrentLayer) });
    }
    for layer in &document.layers {
        if !known.contains(&layer.linetype.to_uppercase()) {
            let problem = format!("uses linetype {}, which the drawing does not have", layer.linetype);
            let fix = Some(Fix::LayerLinetype(layer.name.clone()));
            findings.push(Finding { object: format!("Layer {}", layer.name), problem, place: Place::Drawing, fix });
        }
    }
    for group in &document.groups {
        if group.members.is_empty() {
            let problem = format!("{} has no objects", group.name);
            findings.push(Finding { object: "Group".into(), problem, place: Place::Drawing, fix: Some(Fix::Groups) });
            continue;
        }
        let gone = group.members.iter().filter(|id| document.find_anywhere(**id).is_none()).count();
        if gone > 0 {
            let problem = format!("{} lists {} that are no longer in the drawing", group.name, objects(gone));
            findings.push(Finding { object: "Group".into(), problem, place: Place::Drawing, fix: Some(Fix::Groups) });
        }
    }
    findings
}

/// One object's problems: what makes its shape unusable, then a linetype the drawing does not have. Its layer is noted
/// in `missing_layers` when the drawing does not have it.
fn check_entity(
    document: &Document,
    entity: &Entity,
    place: &Place,
    known: &HashSet<String>,
    findings: &mut Vec<Finding>,
    missing_layers: &mut BTreeMap<String, usize>,
) {
    if document.layer(&entity.layer).is_none() {
        *missing_layers.entry(entity.layer.clone()).or_default() += 1;
    }
    let fixable = matches!(place, Place::Space(_));
    let object = entity.shape.kind().to_string();
    if let Some((problem, remedy)) = shape_problem(document, &entity.shape) {
        let fix = fixable.then_some(match remedy {
            Remedy::Delete => Fix::Delete(entity.id),
            Remedy::Height => Fix::TextHeight(entity.id),
        });
        findings.push(Finding { object: object.clone(), problem, place: place.clone(), fix });
        if remedy == Remedy::Delete {
            return;
        }
    }
    if !known.contains(&entity.linetype.to_uppercase()) {
        let problem = format!("uses linetype {}, which the drawing does not have", entity.linetype);
        let fix = fixable.then_some(Fix::LinetypeByLayer(entity.id));
        findings.push(Finding { object, problem, place: place.clone(), fix });
    }
}

/// What is wrong with a shape that stops it being drawn or makes it meaningless, and what to do about it.
fn shape_problem(document: &Document, shape: &Shape) -> Option<(String, Remedy)> {
    let delete = |problem: &str| Some((problem.to_string(), Remedy::Delete));
    if numbers(shape).iter().any(|n| !n.is_finite()) {
        return delete("has a coordinate that is not a number, so it cannot be drawn");
    }
    match shape {
        Shape::Circle { radius, .. } | Shape::Arc { radius, .. } if *radius <= 0.0 => {
            Some((format!("has a radius of {radius}, which is not positive"), Remedy::Delete))
        }
        Shape::Polyline { vertices, .. } if vertices.len() < 2 => delete("has fewer than 2 vertices"),
        Shape::Mline { vertices, .. } if vertices.len() < 2 => delete("has fewer than 2 vertices"),
        Shape::Spline { control, .. } if control.len() < 2 => delete("has fewer than 2 control points"),
        Shape::Hatch { loops, .. } if loops.iter().all(|l| l.len() < 3) => delete("has no boundary loops"),
        Shape::Wipeout { corners } if corners.len() < 3 => delete("has fewer than 3 corners"),
        Shape::Text { height, .. } if *height <= 0.0 => Some((format!("has a height of {height}, which is not positive"), Remedy::Height)),
        Shape::Insert { block, .. } if !block_defined(document, &block.0.name) => {
            Some((format!("refers to block {}, which the drawing does not define", block.0.name), Remedy::Delete))
        }
        _ => None,
    }
}

/// The coordinates of points.
fn coords(points: &[Point]) -> impl Iterator<Item = f64> + '_ {
    points.iter().flat_map(|p| [p.x, p.y])
}

/// Every number that places or sizes a shape, so that one which is not finite is found.
fn numbers(shape: &Shape) -> Vec<f64> {
    match shape {
        Shape::Line { start, end } => coords(&[*start, *end]).collect(),
        Shape::Circle { center, radius } => coords(&[*center]).chain([*radius]).collect(),
        Shape::Arc { center, radius, start_angle, end_angle } => coords(&[*center]).chain([*radius, *start_angle, *end_angle]).collect(),
        Shape::Polyline { vertices, .. } => vertices.iter().flat_map(|v| [v.x, v.y, v.bulge, v.start_width, v.end_width]).collect(),
        Shape::Text { position, height, rotation, line_spacing, width_factor, oblique, .. } => {
            coords(&[*position]).chain([*height, *rotation, *line_spacing, *width_factor, *oblique]).collect()
        }
        Shape::Dimension { start, end, line_point, angle, .. } => coords(&[*start, *end, *line_point]).chain([*angle]).collect(),
        Shape::Annotation { points, .. } => coords(points).collect(),
        Shape::Table { position, columns, rows, text_height, .. } => {
            coords(&[*position]).chain(columns.iter().chain(rows).copied()).chain([*text_height]).collect()
        }
        Shape::Image { position, u, v, .. } => coords(&[*position, *u, *v]).collect(),
        Shape::XLine { base, direction, .. } => coords(&[*base, *direction]).collect(),
        Shape::Viewport { center, width, height, view_center, view_height, .. } => {
            coords(&[*center, *view_center]).chain([*width, *height, *view_height]).collect()
        }
        Shape::Multileader { leaders, landing, dogleg, text_position, text_height, arrow_size, .. } => leaders
            .iter()
            .flatten()
            .copied()
            .chain([*landing, *text_position])
            .flat_map(|p| [p.x, p.y])
            .chain([*dogleg, *text_height, *arrow_size])
            .collect(),
        Shape::Ellipse { center, major, ratio, start, end } => coords(&[*center, *major]).chain([*ratio, *start, *end]).collect(),
        Shape::Spline { control, knots, weights, .. } => coords(control).chain(knots.iter().chain(weights).copied()).collect(),
        Shape::Point { position } => coords(&[*position]).collect(),
        Shape::Solid { corners } | Shape::Wipeout { corners } => coords(corners).collect(),
        Shape::Mline { vertices, scale, .. } => coords(vertices).chain([*scale]).collect(),
        Shape::Hatch { loops, pattern, scale, angle, gradient, .. } => {
            let mut all: Vec<f64> = loops.iter().flatten().flat_map(|p| [p.x, p.y]).collect();
            for line in pattern {
                all.extend([line.angle, line.origin.x, line.origin.y, line.offset.x, line.offset.y]);
                all.extend(&line.dashes);
            }
            all.extend([*scale, *angle]);
            all.extend(gradient.iter().map(|g| g.angle));
            all
        }
        Shape::Insert { position, scale_x, scale_y, rotation, attributes, .. } => {
            coords(&[*position]).chain([*scale_x, *scale_y, *rotation]).chain(attributes.iter().flat_map(|a| numbers(&a.text))).collect()
        }
    }
}

/// The changes that put these findings right, against the drawing as it is now. Groups are always brought up to date:
/// objects that are gone, deleted here or before, leave them.
pub fn repairs(document: &Document, findings: &[Finding]) -> Repairs {
    let known = known_linetypes(document);
    let mut removed: Vec<Entity> = vec![];
    let mut doomed: HashSet<EntityId> = HashSet::new();
    let mut changed: Vec<Entity> = vec![];
    let mut layers = document.layers.clone();
    let mut layers_changed = false;
    let mut current_layer = None;
    for fix in findings.iter().filter_map(|f| f.fix.as_ref()) {
        match fix {
            Fix::Delete(id) => {
                if let Some(entity) = document.find_anywhere(*id)
                    && doomed.insert(*id)
                {
                    removed.push(entity.clone());
                }
            }
            Fix::LinetypeByLayer(id) => {
                if let Some(entity) = document.find_anywhere(*id) {
                    changed_entry(&mut changed, entity).linetype = BYLAYER_LINETYPE.into();
                }
            }
            Fix::TextHeight(id) => {
                if let Some(entity) = document.find_anywhere(*id)
                    && let Shape::Text { height, .. } = &mut changed_entry(&mut changed, entity).shape
                {
                    *height = 1.0;
                }
            }
            Fix::CreateLayer(name) => {
                if layers.iter().all(|layer| layer.name != *name) {
                    layers.push(Layer::new(name));
                    layers_changed = true;
                }
            }
            Fix::LayerLinetype(name) => {
                if let Some(layer) = layers.iter_mut().find(|layer| layer.name == *name)
                    && !known.contains(&layer.linetype.to_uppercase())
                {
                    layer.linetype = DEFAULT_LINETYPE.into();
                    layers_changed = true;
                }
            }
            Fix::CurrentLayer => {
                if document.layer(&document.current_layer).is_none() {
                    current_layer = Some(DEFAULT_LAYER.to_string());
                    if layers.iter().all(|layer| layer.name != DEFAULT_LAYER) {
                        layers.push(Layer::new(DEFAULT_LAYER));
                        layers_changed = true;
                    }
                }
            }
            Fix::Groups => {}
        }
    }
    changed.retain(|entity| !doomed.contains(&entity.id));
    let groups: Vec<ObjectGroup> = document
        .groups
        .iter()
        .filter_map(|group| {
            let members: Vec<EntityId> =
                group.members.iter().copied().filter(|id| !doomed.contains(id) && document.find_anywhere(*id).is_some()).collect();
            (!members.is_empty()).then(|| ObjectGroup { members, ..group.clone() })
        })
        .collect();
    Repairs {
        removed,
        changed,
        layers: layers_changed.then_some(layers),
        current_layer,
        groups: (groups != document.groups).then_some(groups),
    }
}

/// The entry for an object in the changed list, added from the drawing the first time.
fn changed_entry<'a>(changed: &'a mut Vec<Entity>, entity: &Entity) -> &'a mut Entity {
    let at = match changed.iter().position(|e| e.id == entity.id) {
        Some(at) => at,
        None => {
            changed.push(entity.clone());
            changed.len() - 1
        }
    };
    &mut changed[at]
}

/// RECOVER: find every problem and put right what can be, in every space and then in the drawing's own tables, as
/// the drawing is read. Returns what was found; each finding with a fix has been put right.
pub fn repair_all(document: &mut Document) -> Vec<Finding> {
    let findings = audit(document);
    let original = document.active_layout();
    for space in spaces(document) {
        document.activate(space);
        let scope: Vec<Finding> = findings.iter().filter(|f| f.place == Place::Space(space)).cloned().collect();
        repairs(document, &scope).apply(document);
    }
    document.activate(original);
    let tables: Vec<Finding> = findings.iter().filter(|f| f.place == Place::Drawing).cloned().collect();
    repairs(document, &tables).apply(document);
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::entities::{BlockDef, BlockRef, circle, line, polyline_from, text};
    use crate::model::geometry::pt;
    use crate::model::mline::{Justification, MlineStyle};
    use std::sync::Arc;

    fn kinds(findings: &[Finding]) -> Vec<String> {
        findings.iter().map(|f| f.problem.clone()).collect()
    }

    /// Put the findings' fixes into the drawing, as AUDIT with Yes does.
    fn fix_all(document: &mut Document) -> Vec<Finding> {
        let findings = audit(document);
        let scope: Vec<Finding> = findings.iter().filter(|f| f.place == Place::Space(None) || f.place == Place::Drawing).cloned().collect();
        repairs(document, &scope).apply(document);
        findings
    }

    fn block(name: &str) -> BlockRef {
        BlockRef(Arc::new(BlockDef { name: name.into(), base: pt(0.0, 0.0), entities: vec![], attribute_defs: vec![], xref: None }))
    }

    #[test]
    fn objects_with_coordinates_that_are_not_numbers_are_deleted() {
        let bad = line(pt(f64::NAN, 0.0), pt(1.0, 1.0));
        let far = line(pt(0.0, 0.0), pt(f64::INFINITY, 1.0));
        let good = line(pt(0.0, 0.0), pt(1.0, 1.0));
        let mut document = Document::with_entities(vec![bad.clone(), far.clone(), good.clone()]);
        let findings = audit(&document);
        assert_eq!(findings.len(), 2);
        assert!(findings.iter().all(|f| f.object == "Line" && f.fix.is_some()));
        fix_all(&mut document);
        assert_eq!(document.entities().iter().map(|e| e.id).collect::<Vec<_>>(), vec![good.id]);
        assert!(audit(&document).is_empty());
    }

    #[test]
    fn a_circle_or_arc_needs_a_positive_radius() {
        let zero = circle(pt(0.0, 0.0), 0.0);
        let negative = crate::model::entities::arc(pt(0.0, 0.0), -2.0, 0.0, 90.0);
        let document = Document::with_entities(vec![zero.clone(), negative.clone()]);
        let findings = audit(&document);
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].fix, Some(Fix::Delete(zero.id)));
        assert_eq!(findings[1].fix, Some(Fix::Delete(negative.id)));
        assert!(kinds(&findings)[0].contains("radius of 0"));
    }

    #[test]
    fn shapes_too_small_to_draw_are_deleted() {
        let short_polyline = polyline_from(&[pt(0.0, 0.0)], false);
        let spline = Entity::new(Shape::Spline { degree: 3, control: vec![pt(0.0, 0.0)], knots: vec![], weights: vec![], closed: false });
        let empty_hatch = Entity::new(Shape::Hatch {
            loops: vec![],
            solid: true,
            pattern: vec![],
            name: "SOLID".into(),
            scale: 1.0,
            angle: 0.0,
            gradient: None,
            association: None,
        });
        let wipeout = Entity::new(Shape::Wipeout { corners: vec![pt(0.0, 0.0), pt(1.0, 0.0)] });
        let mline = Entity::new(Shape::Mline {
            vertices: vec![pt(0.0, 0.0)],
            closed: false,
            justification: Justification::Top,
            scale: 1.0,
            style: MlineStyle::standard(),
        });
        let ids = [short_polyline.id, spline.id, empty_hatch.id, wipeout.id, mline.id];
        let document = Document::with_entities(vec![short_polyline, spline, empty_hatch, wipeout, mline]);
        let findings = audit(&document);
        assert_eq!(findings.len(), 5, "{findings:?}");
        let fixes: Vec<Fix> = findings.iter().filter_map(|f| f.fix.clone()).collect();
        assert_eq!(fixes, ids.map(Fix::Delete).to_vec());
        assert_eq!(findings.iter().map(|f| f.object.as_str()).collect::<Vec<_>>(), ["Polyline", "Spline", "Hatch", "Wipeout", "MLine"]);
    }

    #[test]
    fn a_hatch_with_a_loop_or_a_wipeout_with_three_corners_is_kept() {
        let triangle = vec![pt(0.0, 0.0), pt(4.0, 0.0), pt(0.0, 4.0)];
        let hatch = Entity::new(Shape::Hatch {
            loops: vec![triangle.clone()],
            solid: true,
            pattern: vec![],
            name: "SOLID".into(),
            scale: 1.0,
            angle: 0.0,
            gradient: None,
            association: None,
        });
        let wipeout = Entity::new(Shape::Wipeout { corners: triangle });
        let document = Document::with_entities(vec![hatch, wipeout]);
        assert!(audit(&document).is_empty());
    }

    #[test]
    fn text_with_no_height_gets_a_height_of_one() {
        let note = text(pt(0.0, 0.0), "Note", 0.0, 0.0);
        let mut document = Document::with_entities(vec![note.clone()]);
        let findings = audit(&document);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].fix, Some(Fix::TextHeight(note.id)));
        fix_all(&mut document);
        let Shape::Text { height, .. } = &document.get(note.id).unwrap().shape else { panic!("still text") };
        assert_eq!(*height, 1.0);
    }

    #[test]
    fn objects_on_a_missing_layer_create_it_once() {
        let mut first = line(pt(0.0, 0.0), pt(1.0, 0.0));
        first.layer = "Walls".into();
        let mut second = line(pt(0.0, 1.0), pt(1.0, 1.0));
        second.layer = "Walls".into();
        let mut document = Document::with_entities(vec![first, second]);
        let findings = audit(&document);
        assert_eq!(findings.len(), 1, "one finding for the layer, not one for each object");
        assert_eq!(findings[0].fix, Some(Fix::CreateLayer("Walls".into())));
        assert_eq!(findings[0].problem, "is used by 2 objects but is not defined");
        fix_all(&mut document);
        assert!(document.layer("Walls").is_some());
        assert!(audit(&document).is_empty());
    }

    #[test]
    fn an_unknown_linetype_becomes_bylayer() {
        let mut odd = line(pt(0.0, 0.0), pt(1.0, 0.0));
        odd.linetype = "BORDER".into();
        let mut dashed = line(pt(0.0, 1.0), pt(1.0, 1.0));
        dashed.linetype = "dashed".into();
        let mut document = Document::with_entities(vec![odd.clone(), dashed]);
        let findings = audit(&document);
        assert_eq!(findings.len(), 1, "Dashed is one the drawing knows, in any case");
        assert_eq!(findings[0].fix, Some(Fix::LinetypeByLayer(odd.id)));
        fix_all(&mut document);
        assert_eq!(document.get(odd.id).unwrap().linetype, BYLAYER_LINETYPE);
    }

    #[test]
    fn a_layer_with_an_unknown_linetype_gets_continuous() {
        let mut document = Document::new();
        document.layers[0].linetype = "NOPE".into();
        let findings = audit(&document);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].fix, Some(Fix::LayerLinetype("0".into())));
        fix_all(&mut document);
        assert_eq!(document.layers[0].linetype, DEFAULT_LINETYPE);
    }

    #[test]
    fn an_insert_of_an_undefined_block_is_deleted_but_anonymous_ones_are_not() {
        let mut document = Document::new();
        let insert = |block: BlockRef| {
            Entity::new(Shape::Insert { block, position: pt(0.0, 0.0), scale_x: 1.0, scale_y: 1.0, rotation: 0.0, attributes: vec![] })
        };
        let missing = insert(block("GONE"));
        let anonymous = insert(block("*U1"));
        document.blocks.insert("KEPT".into(), block("KEPT"));
        let kept = insert(block("KEPT"));
        document.set_entities(vec![missing.clone(), anonymous, kept]);
        let findings = audit(&document);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].fix, Some(Fix::Delete(missing.id)));
        assert!(findings[0].problem.contains("GONE"));
    }

    #[test]
    fn groups_drop_objects_that_are_gone_and_empty_groups_go() {
        let kept = line(pt(0.0, 0.0), pt(1.0, 0.0));
        let bad = line(pt(f64::NAN, 0.0), pt(1.0, 0.0));
        let mut document = Document::with_entities(vec![kept.clone(), bad.clone()]);
        let gone_id = 999_999_999;
        document.groups = vec![
            ObjectGroup {
                name: "*A1".into(),
                description: String::new(),
                unnamed: true,
                selectable: true,
                members: vec![kept.id, gone_id],
            },
            ObjectGroup { name: "Empty".into(), description: String::new(), unnamed: false, selectable: true, members: vec![] },
            ObjectGroup { name: "Bad".into(), description: String::new(), unnamed: false, selectable: true, members: vec![bad.id] },
        ];
        let findings = audit(&document);
        assert_eq!(findings.iter().filter(|f| f.object == "Group").count(), 2);
        fix_all(&mut document);
        assert_eq!(document.groups.len(), 1, "the empty one and the one of only a deleted object go");
        assert_eq!(document.groups[0].members, vec![kept.id]);
    }

    #[test]
    fn a_missing_current_layer_becomes_layer_zero() {
        let mut document = Document::new();
        document.current_layer = "Gone".into();
        let findings = audit(&document);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].fix, Some(Fix::CurrentLayer));
        fix_all(&mut document);
        assert_eq!(document.current_layer, "0");
    }

    #[test]
    fn block_definitions_are_checked_and_reported_without_a_fix() {
        let mut document = Document::new();
        let bad = line(pt(f64::NAN, 0.0), pt(1.0, 0.0));
        let definition = BlockDef {
            name: "PART".into(),
            base: pt(0.0, 0.0),
            entities: vec![bad, circle(pt(0.0, 0.0), 0.0)],
            attribute_defs: vec![],
            xref: None,
        };
        document.blocks.insert("PART".into(), BlockRef(Arc::new(definition)));
        let findings = audit(&document);
        assert_eq!(findings.len(), 2);
        assert!(findings.iter().all(|f| f.place == Place::Block("PART".into()) && f.fix.is_none()));
    }

    #[test]
    fn recover_repairs_every_space_and_reports_what_it_found() {
        let mut document = Document::with_entities(vec![line(pt(0.0, 0.0), pt(1.0, 0.0))]);
        let layout_bad = line(pt(f64::NAN, 0.0), pt(1.0, 0.0));
        let layout_good = line(pt(0.0, 0.0), pt(2.0, 0.0));
        document.set_space_entities(Some(0), vec![layout_bad.clone(), layout_good.clone()]);
        document.current_layer = "Gone".into();
        let findings = repair_all(&mut document);
        assert_eq!(findings.len(), 2);
        assert!(document.active_layout().is_none(), "model space is open again");
        assert_eq!(document.space_entities(Some(0)).iter().map(|e| e.id).collect::<Vec<_>>(), vec![layout_good.id]);
        assert_eq!(document.current_layer, "0");
        assert!(audit(&document).is_empty());
    }
}
