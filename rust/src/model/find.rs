//! FIND: text in texts, block attributes and dimension text, found and replaced.

use super::entities::{Entity, Shape};

/// Every place an object holds text that can be searched: its text, its attributes, its typed dimension text.
fn texts_mut(shape: &mut Shape) -> Vec<&mut String> {
    match shape {
        Shape::Text { text, .. } | Shape::Multileader { text, .. } => vec![text],
        Shape::Insert { attributes, .. } => {
            attributes.iter_mut().filter_map(|a| if let Shape::Text { text, .. } = &mut a.text { Some(text) } else { None }).collect()
        }
        Shape::Dimension { look, .. } | Shape::Annotation { look, .. } => vec![&mut look.text],
        _ => vec![],
    }
}

fn find(haystack: &str, needle: &str, match_case: bool) -> Option<usize> {
    if match_case { haystack.find(needle) } else { haystack.to_lowercase().find(&needle.to_lowercase()) }
}

/// Whether the object holds the text looked for.
pub fn holds(entity: &Entity, needle: &str, match_case: bool) -> bool {
    !needle.is_empty() && texts_mut(&mut entity.shape.clone()).iter().any(|t| find(t, needle, match_case).is_some())
}

fn replace_all(text: &str, needle: &str, with: &str, match_case: bool) -> String {
    if match_case {
        return text.replace(needle, with);
    }
    // Lower-casing can change lengths for some scripts, so match character by character.
    let (chars, wanted): (Vec<char>, Vec<char>) = (text.chars().collect(), needle.to_lowercase().chars().collect());
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let here: String = chars[i..(i + wanted.len()).min(chars.len())].iter().collect();
        if here.to_lowercase().chars().eq(wanted.iter().copied()) {
            out.push_str(with);
            i += wanted.len();
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// The object with every match replaced, or None when it held none.
pub fn replaced(entity: &Entity, needle: &str, with: &str, match_case: bool) -> Option<Entity> {
    if !holds(entity, needle, match_case) {
        return None;
    }
    let mut shape = entity.shape.clone();
    for text in texts_mut(&mut shape) {
        *text = replace_all(text, needle, with, match_case);
    }
    Some(entity.with_shape(shape))
}

/// Whether a name matches a pattern where * stands for any run of characters and ? for any one, as AutoCAD's
/// layer name filters do. Matches whole names, in the case given.
pub fn wildcard_match(pattern: &str, text: &str) -> bool {
    let (pattern, text): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    // matched[j]: the pattern so far matches the first j characters.
    let mut matched = vec![false; text.len() + 1];
    matched[0] = true;
    for p in pattern {
        let mut next = vec![false; text.len() + 1];
        for j in 0..=text.len() {
            next[j] = match p {
                '*' => matched[j] || (j > 0 && next[j - 1]),
                '?' => j > 0 && matched[j - 1],
                c => j > 0 && matched[j - 1] && text[j - 1] == c,
            };
        }
        matched = next;
    }
    matched[text.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcards_match_whole_names() {
        assert!(wildcard_match("dim*", "dimensions"));
        assert!(wildcard_match("*", ""));
        assert!(wildcard_match("a?c", "abc"));
        assert!(!wildcard_match("a?c", "abbc"));
        assert!(!wildcard_match("wall", "walls"));
        assert!(wildcard_match("*-text", "a-text"));
    }
    use crate::model::entities::text;
    use crate::model::geometry::pt;

    #[test]
    fn text_is_found_and_replaced_with_or_without_case() {
        let note = text(pt(0.0, 0.0), "Door D1, door D2", 2.5, 0.0);
        assert!(holds(&note, "DOOR", false) && !holds(&note, "DOOR", true));
        let Shape::Text { text, .. } = replaced(&note, "door", "Window", false).unwrap().shape else { panic!() };
        assert_eq!(text, "Window D1, Window D2");
        assert!(replaced(&note, "wall", "x", false).is_none());
    }
}
