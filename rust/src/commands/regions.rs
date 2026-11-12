//! REGION and the Boolean commands UNION, SUBTRACT and INTERSECT, which make regions of closed objects and combine
//! them. The geometry is `model::region`.

use super::edit::objects_to_edit;
use super::prompts::{Answer, Io, Prompt, PromptKind, Step};
use crate::model::editing::join;
use crate::model::entities::{Entity, Shape, Vertex};
use crate::model::region::{Boolean, combine, loop_of, union_all};

/// The loops of a region, or of a closed object as a region would have it.
fn loops(entity: &Entity) -> Option<Vec<Vec<Vertex>>> {
    match &entity.shape {
        Shape::Region { loops } => Some(loops.clone()),
        _ => None,
    }
}

/// REGION: each closed object becomes a region in its place, and so does each closed chain of lines, arcs and open
/// polylines meeting end to end. Open objects that close nothing are left.
pub async fn region_command(io: Io, selected: Vec<Entity>) {
    let objects = objects_to_edit(&io, selected).await;
    if objects.is_empty() {
        return;
    }
    let (closed, open): (Vec<&Entity>, Vec<&Entity>) =
        objects.iter().filter(|e| !matches!(e.shape, Shape::Region { .. })).partition(|e| loop_of(e).is_some());
    let mut used: Vec<Entity> = closed.iter().map(|e| (*e).clone()).collect();
    let mut regions: Vec<Entity> = closed.iter().filter_map(|e| Some(e.derive(Shape::Region { loops: vec![loop_of(e)?] }))).collect();
    let open: Vec<Entity> = open.into_iter().cloned().collect();
    if let Some((joined, made)) = join(&open) {
        // A chain that closes is a loop: its pieces go, whatever else was joined stays as it was.
        let chains: Vec<Entity> = made.iter().filter(|e| loop_of(e).is_some()).cloned().collect();
        if !chains.is_empty() {
            for chain in &chains {
                regions.push(chain.derive(Shape::Region { loops: vec![loop_of(chain).expect("closed")] }));
            }
            // The pieces a chain was made of are those lying wholly on it.
            let pieces = |chain: &Entity| {
                let size = crate::model::index::entity_extent(chain).diagonal().max(1e-9);
                let on = |p: crate::model::geometry::Point| crate::selection::picking::entity_distance(chain, p) <= size * 1e-6;
                let chain_pieces: Vec<Entity> =
                    joined.iter().filter(|piece| crate::model::editing::grip_points(&piece.shape).into_iter().all(on)).cloned().collect();
                chain_pieces
            };
            used.extend(chains.iter().flat_map(pieces));
        }
    }
    if regions.is_empty() {
        io.step(Step::Report("0 loops extracted. 0 Regions created.".into()));
        return;
    }
    let count = regions.len();
    io.step(Step::Compound(vec![Step::Remove(used), Step::Commit(regions)]));
    io.step(Step::Report(format!("{count} loop(s) extracted. {count} Region(s) created.")));
}

/// Ask for regions, saying how many of what was chosen are not regions.
async fn regions(io: &Io, message: &str, selected: Vec<Entity>) -> Option<Vec<Entity>> {
    let chosen = if selected.is_empty() {
        match io.ask(Prompt::new(PromptKind::Select, message)).await {
            Some(Answer::Objects(chosen)) => chosen,
            _ => return None,
        }
    } else {
        selected
    };
    let (regions, others): (Vec<Entity>, Vec<Entity>) = chosen.into_iter().partition(|e| loops(e).is_some());
    if !others.is_empty() {
        io.step(Step::Report(format!("{} object(s) are not regions and are left out. Make regions of them with REGION.", others.len())));
    }
    Some(regions)
}

/// The result of a Boolean in place of the regions it was made from: one region, on the first one's layer and
/// with its look, or nothing when nothing is left.
fn replace(io: &Io, used: Vec<Entity>, loops: Vec<Vec<Vertex>>) {
    let Some(first) = used.first().cloned() else { return };
    if loops.is_empty() {
        io.step(Step::Remove(used));
        io.step(Step::Report("Null region created - deleted.".into()));
        return;
    }
    io.step(Step::Compound(vec![Step::Remove(used), Step::Commit(vec![first.derive(Shape::Region { loops })])]));
}

/// UNION: regions become one, their areas joined.
pub async fn union_command(io: Io, selected: Vec<Entity>) {
    let Some(chosen) = regions(&io, "Select objects:", selected).await else { return };
    if chosen.len() < 2 {
        io.step(Step::Report("At least 2 regions are required.".into()));
        return;
    }
    let sets: Vec<Vec<Vec<Vertex>>> = chosen.iter().filter_map(loops).collect();
    replace(&io, chosen, union_all(&sets));
}

/// SUBTRACT: the regions of the second selection are taken out of those of the first, joined.
pub async fn subtract_command(io: Io, selected: Vec<Entity>) {
    let Some(from) = regions(&io, "Select solids, surfaces, and regions to subtract from ..", selected).await else { return };
    if from.is_empty() {
        return;
    }
    let ids: Vec<_> = from.iter().map(|e| e.id).collect();
    let Some(taken) = regions(&io, "Select solids, surfaces, and regions to subtract ..", vec![]).await else { return };
    let taken: Vec<Entity> = taken.into_iter().filter(|e| !ids.contains(&e.id)).collect();
    let kept = union_all(&from.iter().filter_map(loops).collect::<Vec<_>>());
    let removed = union_all(&taken.iter().filter_map(loops).collect::<Vec<_>>());
    let result = if removed.is_empty() { kept } else { combine(&kept, &removed, Boolean::Subtract) };
    let mut used = from;
    used.extend(taken);
    replace(&io, used, result);
}

/// INTERSECT: what all the regions have in common.
pub async fn intersect_command(io: Io, selected: Vec<Entity>) {
    let Some(chosen) = regions(&io, "Select objects:", selected).await else { return };
    if chosen.len() < 2 {
        io.step(Step::Report("At least 2 regions are required.".into()));
        return;
    }
    let mut sets = chosen.iter().filter_map(loops);
    let first = sets.next().unwrap_or_default();
    let common = sets.fold(first, |all, set| if all.is_empty() { all } else { combine(&all, &set, Boolean::Intersect) });
    replace(&io, chosen, common);
}
