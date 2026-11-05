//! GROUP, UNGROUP and GROUPEDIT: named sets of objects that select together.

use std::cell::RefCell;
use std::rc::Rc;

use super::defaults::Defaults;
use super::edit::objects_to_edit;
use super::prompts::{Answer, Io, Prompt, PromptKind, Step};
use crate::model::document::ObjectGroup;
use crate::model::entities::{Entity, EntityId};

const DESCRIPTION: &str = "D";
const ADD: &str = "A";
const REMOVE: &str = "R";
const RENAME: &str = "REN";

fn unnamed_name(groups: &[ObjectGroup]) -> String {
    (1..).map(|n| format!("*A{n}")).find(|n| !groups.iter().any(|g| g.name.eq_ignore_ascii_case(n))).unwrap_or_default()
}

fn ids(objects: &[Entity]) -> Vec<EntityId> {
    objects.iter().map(|e| e.id).collect()
}

/// GROUP: the selected objects become a group, named or unnamed (*A1…), with a description if wanted.
pub async fn group(io: Io, selected: Vec<Entity>, defaults: Rc<RefCell<Defaults>>) {
    let objects = objects_to_edit(&io, selected).await;
    if objects.is_empty() {
        return;
    }
    let mut groups = defaults.borrow().groups.clone();
    let prompt = Prompt::new(PromptKind::Text, "Enter a group name or [Description] <unnamed>:").default(Answer::Text(String::new()));
    let Some(Answer::Text(typed)) = io.ask(prompt).await else { return };
    let mut description = String::new();
    let mut name = typed.trim().to_string();
    if name.eq_ignore_ascii_case(DESCRIPTION) {
        let prompt = Prompt::new(PromptKind::Text, "Enter group description:").default(Answer::Text(String::new()));
        let Some(Answer::Text(text)) = io.ask(prompt).await else { return };
        description = text.trim().to_string();
        let prompt = Prompt::new(PromptKind::Text, "Enter a group name <unnamed>:").default(Answer::Text(String::new()));
        let Some(Answer::Text(text)) = io.ask(prompt).await else { return };
        name = text.trim().to_string();
    }
    if groups.iter().any(|g| g.name.eq_ignore_ascii_case(&name)) {
        io.step(Step::Report(format!("A group named \"{name}\" already exists.")));
        return;
    }
    let unnamed = name.is_empty();
    if unnamed {
        name = unnamed_name(&groups);
    }
    groups.push(ObjectGroup { name: name.clone(), description, unnamed, selectable: true, members: ids(&objects) });
    io.step(Step::Groups(groups));
    io.step(Step::Report(format!("Group \"{name}\" has been created with {} object(s).", objects.len())));
}

/// UNGROUP: the groups the selected objects belong to are dissolved; the objects stay.
pub async fn ungroup(io: Io, selected: Vec<Entity>, defaults: Rc<RefCell<Defaults>>) {
    let objects = objects_to_edit(&io, selected).await;
    let picked = ids(&objects);
    let groups = defaults.borrow().groups.clone();
    let (gone, kept): (Vec<ObjectGroup>, Vec<ObjectGroup>) = groups.into_iter().partition(|g| g.members.iter().any(|m| picked.contains(m)));
    if gone.is_empty() {
        if !objects.is_empty() {
            io.step(Step::Report("No group found among the objects selected.".into()));
        }
        return;
    }
    io.step(Step::Groups(kept));
    for group in gone {
        io.step(Step::Report(format!("Group \"{}\" has been exploded.", group.name)));
    }
}

/// GROUPEDIT: pick a member of a group, then add objects to it, remove some, or rename it.
pub async fn groupedit(io: Io, defaults: Rc<RefCell<Defaults>>) {
    let mut groups = defaults.borrow().groups.clone();
    let Some(Answer::Picked(picked)) = io.ask(Prompt::new(PromptKind::Pick, "Select group:")).await else { return };
    let Some(index) = groups.iter().position(|g| g.members.contains(&picked.entity.id)) else {
        io.step(Step::Report("The object picked is not in a group.".into()));
        return;
    };
    let prompt = Prompt::point(format!("Group \"{}\". Enter an option [Add objects/Remove objects/REName]:", groups[index].name))
        .options(&[ADD, REMOVE, RENAME]);
    match io.ask(prompt).await {
        Some(answer) if answer.is(ADD) || answer.is(REMOVE) => {
            let adding = answer.is(ADD);
            let message = if adding { "Select objects to add to the group:" } else { "Select objects to remove from the group:" };
            let Some(Answer::Objects(objects)) = io.ask(Prompt::new(PromptKind::Select, message)).await else { return };
            let chosen = ids(&objects);
            let members = &mut groups[index].members;
            if adding {
                members.extend(chosen.into_iter().filter(|id| !members.contains(id)).collect::<Vec<_>>());
            } else {
                members.retain(|id| !chosen.contains(id));
            }
            let count = members.len();
            io.step(Step::Groups(groups));
            io.step(Step::Report(format!("The group now has {count} object(s).")));
        }
        Some(answer) if answer.is(RENAME) => {
            let Some(Answer::Text(name)) = io.ask(Prompt::new(PromptKind::Text, "Enter new name for group:")).await else { return };
            let name = name.trim().to_string();
            if name.is_empty() || groups.iter().enumerate().any(|(i, g)| i != index && g.name.eq_ignore_ascii_case(&name)) {
                io.step(Step::Report(format!("\"{name}\" cannot name this group.")));
                return;
            }
            (groups[index].name, groups[index].unnamed) = (name, false);
            io.step(Step::Groups(groups));
        }
        _ => {}
    }
}
