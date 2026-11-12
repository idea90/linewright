//! Commands that change the program's own settings rather than the drawing. The window keeps what they set; see
//! `ui::settings` for the file it is kept in.

use std::cell::RefCell;
use std::rc::Rc;

use super::defaults::{Defaults, MAX_SAVETIME};
use super::prompts::{Answer, Io, Prompt, PromptKind, Step};

/// SAVETIME: asks for the autosave interval in minutes, as AutoCAD does, until a whole number from 0 to 600 is typed.
/// The command leaves the value in `defaults` for the window to set and keep.
pub async fn savetime_command(io: Io, defaults: Rc<RefCell<Defaults>>) {
    let current = defaults.borrow().autosave_minutes;
    loop {
        let prompt = Prompt::new(PromptKind::Number, format!("Enter new value for SAVETIME <{current}>:"))
            .default(Answer::Number(f64::from(current)));
        let Some(Answer::Number(value)) = io.ask(prompt).await else { return };
        if value.fract() == 0.0 && (0.0..=f64::from(MAX_SAVETIME)).contains(&value) {
            let minutes = value as u32;
            defaults.borrow_mut().autosave_request = Some(minutes);
            let message = if minutes == 0 { "Autosave is off.".to_string() } else { format!("Autosave every {minutes} minutes.") };
            io.step(Step::Report(message));
            return;
        }
        io.step(Step::Report(format!("Enter a whole number of minutes from 0 to {MAX_SAVETIME}.")));
    }
}

/// CANNOSCALE: the annotation scale annotative objects in model space are drawn at, from the scale list or typed
/// as 1:50. `?` lists the scales.
pub async fn cannoscale_command(io: Io, defaults: Rc<RefCell<Defaults>>) {
    let (current, list) = {
        let d = defaults.borrow();
        (d.annotation_scale.clone(), d.annotation_scales.clone())
    };
    loop {
        let prompt = Prompt::new(PromptKind::Text, format!("Enter new value for CANNOSCALE, or ? <\"{}\">:", current.name))
            .default(Answer::Text(current.name.clone()));
        let Some(Answer::Text(typed)) = io.ask(prompt).await else { return };
        if typed.trim() == "?" {
            let names: Vec<&str> = list.iter().map(|s| s.name.as_str()).collect();
            io.step(Step::Report(format!("Scales: {}", names.join(", "))));
            continue;
        }
        match crate::model::annotative::parse_scale(&typed, &list) {
            Some(scale) => {
                io.step(Step::AnnotationScale(scale));
                return;
            }
            None => io.step(Step::Report("Enter a scale from the list, or as paper units:drawing units such as 1:50.".into())),
        }
    }
}
