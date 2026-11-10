//! A DXF file as it is on disk: code and value pairs, grouped from one code 0 to the next, in the file's text
//! encoding. Nothing here knows what the groups mean; `dxf` turns them into a drawing and back. The document keeps
//! the file it was opened from, so this part depends on nothing else in the program but `Point`.

use crate::model::geometry::{Point, pt};

const TEMPLATE: &str = include_str!("../template.dxf");

#[derive(Clone, Debug, PartialEq)]
pub struct Pair {
    pub code: i32,
    pub value: String,
}

pub(super) fn pair(code: i32, value: impl ToString) -> Pair {
    Pair { code, value: value.to_string() }
}

/// One group: a code 0 pair and everything up to the next one.
#[derive(Clone, Debug, PartialEq)]
pub struct Group(pub Vec<Pair>);

impl Group {
    pub fn kind(&self) -> &str {
        self.0.first().filter(|p| p.code == 0).map_or("", |p| p.value.trim())
    }

    /// The first value with this code, before any extended data.
    pub fn get(&self, code: i32) -> Option<&str> {
        self.0.iter().take_while(|p| p.code < 1000).find(|p| p.code == code).map(|p| p.value.as_str())
    }

    pub(super) fn text(&self, code: i32) -> Option<String> {
        self.get(code).map(|v| v.to_string())
    }

    pub(super) fn float(&self, code: i32) -> Option<f64> {
        self.get(code).and_then(|v| v.trim().parse().ok())
    }

    pub(super) fn int(&self, code: i32) -> Option<i64> {
        self.get(code).and_then(|v| v.trim().parse::<f64>().ok()).map(|v| v as i64)
    }

    pub(super) fn point(&self, x_code: i32) -> Option<Point> {
        Some(pt(self.float(x_code)?, self.float(x_code + 10)?))
    }

    pub(super) fn set(&mut self, code: i32, value: String) {
        match self.0.iter_mut().take_while(|p| p.code < 1000).find(|p| p.code == code) {
            Some(existing) => existing.value = value,
            None => {
                let at = self.0.iter().position(|p| p.code >= 1000).unwrap_or(self.0.len());
                self.0.insert(at, pair(code, value));
            }
        }
    }

    pub(super) fn handle(&self) -> Option<&str> {
        self.get(5).or_else(|| self.get(105))
    }

    /// The pairs of one subclass (after its code 100 marker, up to the next), to look codes up in that part only.
    pub(super) fn subclass(&self, name: &str) -> Group {
        let pairs = self.0.iter().skip_while(|p| !(p.code == 100 && p.value.trim() == name)).skip(1);
        Group(std::iter::once(pair(0, self.kind())).chain(pairs.take_while(|p| p.code != 100).cloned()).collect())
    }

    /// Set a code within one subclass, adding it at the subclass's end when missing.
    pub(super) fn set_in(&mut self, subclass: &str, code: i32, value: String) {
        let Some(start) = self.0.iter().position(|p| p.code == 100 && p.value.trim() == subclass) else { return };
        let end = self.0[start + 1..].iter().position(|p| p.code == 100 || p.code >= 1000).map_or(self.0.len(), |i| start + 1 + i);
        match self.0[start + 1..end].iter().position(|p| p.code == code) {
            Some(i) => self.0[start + 1 + i].value = value,
            None => self.0.insert(end, pair(code, value)),
        }
    }

    /// Mark an entity as on paper space, where AutoCAD looks for code 67 just before its layer.
    pub(super) fn on_paper(&mut self) {
        if self.get(67).is_none() {
            let at = self.0.iter().position(|p| p.code == 8).unwrap_or(self.0.len().min(2));
            self.0.insert(at, pair(67, 1));
        }
    }
}

/// A DXF file as groups of code pairs.
#[derive(Clone, Debug, Default)]
pub struct DxfFile {
    pub groups: Vec<Group>,
}

#[derive(Debug)]
pub struct DxfError(pub String);

impl std::fmt::Display for DxfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The text encoding of a file: UTF-8 from AutoCAD 2007 (AC1021) on, the Windows code page its header names before.
fn file_encoding(version: &str, code_page: Option<&str>) -> &'static encoding_rs::Encoding {
    if version >= "AC1021" {
        return encoding_rs::UTF_8;
    }
    code_page.map_or(encoding_rs::WINDOWS_1252, code_page_encoding)
}

/// "ANSI_1251", "ANSI_936" and so on, as $DWGCODEPAGE names them.
fn code_page_encoding(name: &str) -> &'static encoding_rs::Encoding {
    use encoding_rs::*;
    let digits: String = name.chars().filter(char::is_ascii_digit).collect();
    match digits.parse::<u32>().unwrap_or(1252) {
        874 => WINDOWS_874,
        932 => SHIFT_JIS,
        936 => GBK,
        949 => EUC_KR,
        950 => BIG5,
        1250 => WINDOWS_1250,
        1251 => WINDOWS_1251,
        1253 => WINDOWS_1253,
        1254 => WINDOWS_1254,
        1255 => WINDOWS_1255,
        1256 => WINDOWS_1256,
        1257 => WINDOWS_1257,
        1258 => WINDOWS_1258,
        _ => WINDOWS_1252,
    }
}

/// The encodings \M+n escapes name: a character from an East Asian code page, as two bytes in hex.
fn multibyte_encoding(n: char) -> Option<&'static encoding_rs::Encoding> {
    match n {
        '1' => Some(encoding_rs::SHIFT_JIS),
        '2' => Some(encoding_rs::BIG5),
        '3' => Some(encoding_rs::EUC_KR),
        '5' => Some(encoding_rs::GBK),
        _ => None,
    }
}

/// Characters a file escapes: \U+XXXX for any Unicode character, \M+nXXXX for one from an East Asian code page.
fn decode_escapes(text: &str) -> String {
    // Nearly every value has no backslash at all, which is quick to see.
    if !text.contains('\\') || (!text.contains("\\U+") && !text.contains("\\M+")) {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(['\\']) {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        let unicode =
            tail.strip_prefix("\\U+").and_then(|t| t.get(..4)).and_then(|h| u32::from_str_radix(h, 16).ok()).and_then(char::from_u32);
        let multibyte = tail.strip_prefix("\\M+").and_then(|t| {
            let encoding = multibyte_encoding(t.chars().next()?)?;
            let code = u16::from_str_radix(t.get(1..5)?, 16).ok()?;
            let bytes = code.to_be_bytes();
            let (decoded, _, failed) = encoding.decode(&bytes);
            (!failed).then(|| decoded.into_owned())
        });
        if let Some(c) = unicode {
            out.push(c);
            rest = &tail[7..];
        } else if let Some(decoded) = multibyte {
            out.push_str(&decoded);
            rest = &tail[8..];
        } else {
            out.push('\\');
            rest = &tail[1..];
        }
    }
    out.push_str(rest);
    out
}

/// Whether a group code's value must be a number. Codes for points, angles, distances, counts and flags are numbers;
/// names, handles and text are not.
fn is_number_code(code: i32) -> bool {
    matches!(code, 10..=58 | 60..=79 | 90..=99 | 110..=149 | 160..=179 | 210..=239 | 270..=289 | 370..=389 | 420..=429 | 460..=469 | 1010..=1071)
}

/// The line of the next object at or after `from`: a group code 0 followed by a name in capitals, such as LINE or ENDSEC.
fn next_object(lines: &[&str], from: usize) -> Option<usize> {
    (from..lines.len().saturating_sub(1)).find(|&i| {
        let name = lines[i + 1].trim();
        lines[i].trim() == "0"
            && name.starts_with(|c: char| c.is_ascii_uppercase())
            && name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
    })
}

/// A line as it is quoted in a note, cut short when it is long.
fn shorten(text: &str) -> String {
    text.chars().take(40).collect()
}

/// A file shorter than this for each thread is split into groups on the one thread.
const BYTES_PER_THREAD: usize = 2_000_000;

/// How a run of a file's lines ended.
#[derive(Debug, PartialEq)]
enum RunEnd {
    /// At its last line: the file carries on in the next run, if there is one.
    Open,
    /// At the EOF group: nothing after it is read.
    Eof,
    /// At a line (this one, counted from 1 in the whole file) that should hold a group code but is blank, as are
    /// all the lines after it in the run. That ends the file when all its remaining lines are blank too.
    Blank(usize),
}

/// The groups in a run of a file's lines.
#[derive(Debug, PartialEq)]
struct Run {
    groups: Vec<Group>,
    /// The first group does not start here: its pairs carry on the last group of the run before.
    carries_on: bool,
    end: RunEnd,
}

/// Split a run of lines, which starts on a group code's line, into groups. `lines_before` is how many lines the
/// file has before the run, and `first` whether it is the file's first, whose first pair starts a group whatever
/// its code.
fn groups_in_run(text: &str, lines_before: usize, first: bool) -> Result<Run, DxfError> {
    let mut groups: Vec<Group> = vec![];
    let mut carries_on = false;
    let mut lines = text.lines();
    let mut line_number = lines_before;
    while let Some(code_line) = lines.next() {
        line_number += 1;
        let code_text = code_line.trim();
        if code_text.is_empty() && lines.clone().all(|l| l.trim().is_empty()) {
            return Ok(Run { groups, carries_on, end: RunEnd::Blank(line_number) });
        }
        let code: i32 =
            code_text.parse().map_err(|_| DxfError(format!("Line {line_number} should hold a group code but holds \"{code_text}\".")))?;
        let value = lines.next().ok_or_else(|| DxfError("The file ends in the middle of a value.".into()))?;
        line_number += 1;
        let value = decode_escapes(value);
        if code == 0 || groups.is_empty() {
            // Only a run's first group can belong to the run before: every later one starts at its own code 0.
            carries_on |= groups.is_empty() && code != 0 && !first;
            groups.push(Group(Vec::with_capacity(PAIRS_TO_START_WITH)));
        }
        let is_eof = code == 0 && value.trim() == "EOF";
        groups.last_mut().expect("a group").0.push(Pair { code, value });
        if is_eof {
            return Ok(Run { groups, carries_on, end: RunEnd::Eof });
        }
    }
    Ok(Run { groups, carries_on, end: RunEnd::Open })
}

/// Room for this many pairs is made when a group starts: most objects have about so many, and a group that grows
/// a pair at a time moves several times on the way.
const PAIRS_TO_START_WITH: usize = 8;

/// Where to cut a file's text into `count` runs of whole pairs of lines (a group code's line and its value's), of
/// about the same size: where each run starts, and how many lines come before it.
fn run_starts(text: &str, count: usize) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let line_ends = |from: usize, to: usize| bytes[from..to].iter().filter(|b| **b == b'\n').count();
    let after_line = |from: usize| bytes[from..].iter().position(|b| *b == b'\n').map_or(bytes.len(), |at| from + at + 1);
    let mut starts = vec![(0, 0)];
    for run in 1..count {
        let (last, lines_before) = starts[starts.len() - 1];
        let aim = (bytes.len() / count * run).max(last);
        // The start of the next line, and of the one after when that one is a value's line.
        let mut start = after_line(aim);
        let mut lines = lines_before + line_ends(last, start);
        if lines % 2 == 1 {
            let next = after_line(start);
            lines += line_ends(start, next);
            start = next;
        }
        if start >= bytes.len() {
            break;
        }
        if lines % 2 == 0 && start > last {
            starts.push((start, lines));
        }
    }
    starts
}

/// Split a file's text into groups, the runs it is cut into shared among threads. It gives what going through the
/// lines one by one gives, to the same error for a file that is not right.
fn groups_shared(text: &str, runs: usize) -> Result<Vec<Group>, DxfError> {
    if runs < 2 {
        return groups_in_run(text, 0, true).map(|run| run.groups);
    }
    let starts = run_starts(text, runs);
    let split: Vec<Result<Run, DxfError>> = crate::threads::map_runs(&(0..starts.len()).collect::<Vec<_>>(), 1, |_, some| {
        let read = |&index: &usize| {
            let (start, lines_before) = starts[index];
            let end = starts.get(index + 1).map_or(text.len(), |next| next.0);
            groups_in_run(&text[start..end], lines_before, index == 0)
        };
        some.iter().map(read).collect()
    });
    let mut groups: Vec<Group> = vec![];
    // The line a run ended blank at: every run after it must be blank as well, or that line is the error.
    let mut blank_at: Option<usize> = None;
    for run in split {
        if let Some(line) = blank_at {
            match run {
                Ok(run) if run.groups.is_empty() && matches!(run.end, RunEnd::Blank(_)) => continue,
                _ => return Err(DxfError(format!("Line {line} should hold a group code but holds \"\"."))),
            }
        }
        let run = run?;
        let mut added = run.groups.into_iter();
        if run.carries_on
            && let Some(last) = groups.last_mut()
        {
            last.0.extend(added.next().into_iter().flat_map(|group| group.0));
        }
        groups.extend(added);
        match run.end {
            RunEnd::Eof => break,
            RunEnd::Blank(line) => blank_at = blank_at.or(Some(line)),
            RunEnd::Open => {}
        }
    }
    Ok(groups)
}

/// A group code as a DXF file writes it: right-aligned in three columns.
fn push_code(text: &mut String, code: i32) {
    match code {
        0..=9 => text.extend([' ', ' ', digit(code)]),
        10..=99 => text.extend([' ', digit(code / 10), digit(code % 10)]),
        100..=999 => text.extend([digit(code / 100), digit(code / 10 % 10), digit(code % 10)]),
        _ => {
            use std::fmt::Write as _;
            let _ = write!(text, "{code:>3}");
        }
    }
}

fn digit(value: i32) -> char {
    char::from(b'0' + value as u8)
}

impl DxfFile {
    pub fn parse(bytes: &[u8]) -> Result<DxfFile, DxfError> {
        if bytes.starts_with(b"AutoCAD Binary DXF") {
            return Err(DxfError("Binary DXF files are not supported. Save it as an ASCII DXF and try again.".into()));
        }
        let version = find_header(bytes, "$ACADVER").unwrap_or_else(|| "AC1009".into());
        let encoding = file_encoding(&version, find_header(bytes, "$DWGCODEPAGE").as_deref());
        let (decoded, _, _) = encoding.decode(bytes);
        let text = decoded.strip_prefix('\u{feff}').unwrap_or(&decoded);
        let groups = groups_shared(text, crate::threads::threads_for(text.len(), BYTES_PER_THREAD))?;
        if !groups.iter().any(|g| g.kind() == "SECTION") {
            return Err(DxfError("This is not a DXF drawing.".into()));
        }
        Ok(DxfFile { groups })
    }

    /// Reads a damaged DXF file that `parse` refuses, keeping what can be read. A line that should hold a group code
    /// but does not, and a value that is not a number where its code needs one, take out the object they belong to,
    /// and reading picks up at the next object. A file that ends in the middle of a value loses that last line. The
    /// notes say what was skipped.
    pub fn parse_recovering(bytes: &[u8]) -> Result<(DxfFile, Vec<String>), DxfError> {
        if bytes.starts_with(b"AutoCAD Binary DXF") {
            return Err(DxfError("Binary DXF files are not supported. Save it as an ASCII DXF and try again.".into()));
        }
        let version = find_header(bytes, "$ACADVER").unwrap_or_else(|| "AC1009".into());
        let encoding = file_encoding(&version, find_header(bytes, "$DWGCODEPAGE").as_deref());
        let (decoded, _, _) = encoding.decode(bytes);
        let text = decoded.strip_prefix('\u{feff}').unwrap_or(&decoded);
        let lines: Vec<&str> = text.lines().collect();
        let mut groups: Vec<Group> = vec![];
        let mut notes = vec![];
        // Whether the object being read holds a value that cannot be read, so the whole object is left out.
        let mut damaged = false;
        let mut i = 0;
        while i < lines.len() {
            let code_text = lines[i].trim();
            if code_text.is_empty() && lines[i..].iter().all(|l| l.trim().is_empty()) {
                break;
            }
            let Ok(code) = code_text.parse::<i32>() else {
                notes.push(format!("Line {} should hold a group code but holds \"{}\".", i + 1, shorten(code_text)));
                // Header variables are read one by one, so only a damaged one is lost there.
                damaged |= groups.last().is_none_or(|g| g.kind() != "SECTION");
                match next_object(&lines, i + 1) {
                    Some(next) => i = next,
                    None => break,
                }
                continue;
            };
            if i + 1 >= lines.len() {
                notes.push("The file ends in the middle of a value, so its last line is left out.".into());
                break;
            }
            let value = decode_escapes(lines[i + 1]);
            let line = i + 1;
            i += 2;
            if code == 0 {
                if std::mem::take(&mut damaged) {
                    groups.pop();
                }
                if value.trim() == "EOF" {
                    break;
                }
                groups.push(Group(vec![]));
            }
            let Some(group) = groups.last_mut() else {
                notes.push(format!("Line {line} holds a value outside any object, so it is skipped."));
                continue;
            };
            if is_number_code(code) && value.trim().parse::<f64>().is_err() {
                notes.push(format!("Line {line} holds \"{}\" where group code {code} needs a number, so it is skipped.", shorten(&value)));
                damaged |= group.kind() != "SECTION";
                continue;
            }
            group.0.push(Pair { code, value });
        }
        if damaged {
            groups.pop();
        }
        if !groups.iter().any(|g| g.kind() == "SECTION") {
            return Err(DxfError("This is not a DXF drawing.".into()));
        }
        Ok((DxfFile { groups }, notes))
    }

    pub fn template() -> DxfFile {
        DxfFile::parse(TEMPLATE.as_bytes()).expect("the built-in template is a valid DXF file")
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let encoding = file_encoding(&self.version(), self.header("$DWGCODEPAGE"));
        // A code and two line ends on top of each value: sized once, and written without a string made for each code.
        let size: usize = self.groups.iter().flat_map(|group| &group.0).map(|p| p.value.len() + 6).sum();
        let mut text = String::with_capacity(size);
        for group in &self.groups {
            for p in &group.0 {
                push_code(&mut text, p.code);
                text.push('\n');
                text.push_str(&p.value);
                text.push('\n');
            }
        }
        if encoding == encoding_rs::UTF_8 {
            return text.into_bytes();
        }
        // In the file's code page; what it has no character for is written as a \U+XXXX escape.
        let mut bytes = Vec::with_capacity(text.len());
        let mut buffer = [0u8; 4];
        for c in text.chars() {
            if c.is_ascii() {
                bytes.push(c as u8);
                continue;
            }
            let (encoded, _, failed) = encoding.encode(c.encode_utf8(&mut buffer));
            if failed {
                bytes.extend(format!("\\U+{:04X}", c as u32).bytes());
            } else {
                bytes.extend_from_slice(&encoded);
            }
        }
        bytes
    }

    pub(super) fn section(&self, name: &str) -> Option<(usize, usize)> {
        let start =
            self.groups.iter().position(|g| g.kind() == "SECTION" && g.get(2).is_some_and(|n| n.trim().eq_ignore_ascii_case(name)))?;
        let end = self.groups[start..].iter().position(|g| g.kind() == "ENDSEC").map_or(self.groups.len(), |i| start + i);
        Some((start, end))
    }

    /// The groups of a table (its TABLE group first, its ENDTAB last but one past the end).
    pub(super) fn table(&self, name: &str) -> Option<(usize, usize)> {
        let (start, end) = self.section("TABLES")?;
        let table = (start..end)
            .find(|&i| self.groups[i].kind() == "TABLE" && self.groups[i].get(2).is_some_and(|n| n.trim().eq_ignore_ascii_case(name)))?;
        let finish = (table..end).find(|&i| self.groups[i].kind() == "ENDTAB").unwrap_or(end);
        Some((table, finish))
    }

    pub(super) fn table_entries(&self, name: &str) -> Vec<usize> {
        match self.table(name) {
            Some((start, end)) => (start + 1..end).filter(|&i| self.groups[i].kind().eq_ignore_ascii_case(name)).collect(),
            None => vec![],
        }
    }

    pub fn header(&self, variable: &str) -> Option<&str> {
        let (start, _) = self.section("HEADER")?;
        let pairs = &self.groups[start].0;
        let at = pairs.iter().position(|p| p.code == 9 && p.value.trim() == variable)?;
        pairs.get(at + 1).filter(|p| p.code != 9).map(|p| p.value.as_str())
    }

    pub(super) fn set_header(&mut self, variable: &str, code: i32, value: String) {
        let Some((start, _)) = self.section("HEADER") else { return };
        let pairs = &mut self.groups[start].0;
        match pairs.iter().position(|p| p.code == 9 && p.value.trim() == variable) {
            Some(at) if at + 1 < pairs.len() && pairs[at + 1].code != 9 => pairs[at + 1].value = value,
            _ => {
                pairs.push(pair(9, variable));
                pairs.push(pair(code, value));
            }
        }
    }

    pub fn version(&self) -> String {
        self.header("$ACADVER").map_or("AC1009".to_string(), |v| v.trim().to_string())
    }

    pub(super) fn modern(&self) -> bool {
        self.version().as_str() >= "AC1012"
    }

    pub fn insunits(&self) -> i64 {
        self.header("$INSUNITS").and_then(|v| v.trim().parse().ok()).unwrap_or(0)
    }

    pub(super) fn next_free_handle(&self) -> u64 {
        let seed = self.header("$HANDSEED").and_then(|v| u64::from_str_radix(v.trim(), 16).ok()).unwrap_or(1);
        let used = self
            .groups
            .iter()
            .flat_map(|g| g.0.iter())
            .filter(|p| p.code == 5 || p.code == 105)
            .filter_map(|p| u64::from_str_radix(p.value.trim(), 16).ok())
            .max()
            .unwrap_or(0);
        seed.max(used + 1)
    }
}

/// A header variable's value, read from the start of the raw file before its encoding is known.
fn find_header(bytes: &[u8], variable: &str) -> Option<String> {
    let head = &bytes[..bytes.len().min(16384)];
    let text = String::from_utf8_lossy(head);
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        if line.trim() == variable {
            lines.next();
            return lines.next().map(|v| v.trim().to_string());
        }
    }
    None
}

#[cfg(test)]
mod shared_tests {
    use super::*;

    /// A file's text of `objects` groups whose values are of many lengths, so cuts fall in different places.
    fn drawing(objects: usize, line_end: &str) -> String {
        let mut lines = vec!["0".to_string(), "SECTION".into(), "2".into(), "ENTITIES".into()];
        for n in 0..objects {
            lines.extend(["  0".to_string(), "LINE".into(), "  8".into(), format!("layer{}", "x".repeat(n * 7 % 23))]);
            for (code, value) in [(10, n as f64 * 1.25), (20, n as f64 / 3.0), (11, 0.5), (21, -(n as f64))] {
                lines.extend([format!(" {code}"), format!("{value}")]);
            }
        }
        lines.extend(["  0".to_string(), "ENDSEC".into(), "  0".into(), "EOF".into()]);
        lines.join(line_end) + line_end
    }

    fn one_by_one(text: &str) -> Result<Vec<Group>, String> {
        groups_in_run(text, 0, true).map(|run| run.groups).map_err(|e| e.0)
    }

    #[test]
    fn a_file_split_on_several_threads_gives_what_one_thread_gives() {
        let whole = drawing(300, "\n");
        let mut files = vec![
            ("as written", whole.clone()),
            ("Windows line ends", drawing(120, "\r\n")),
            ("no line end at the end", whole.trim_end().to_string()),
            ("blank lines at the end", format!("{whole}\n  \n\n\n")),
            ("more after EOF", format!("{whole}this is\nnot a drawing\n")),
            ("empty", String::new()),
            ("only blank lines", "\n\n \n".to_string()),
        ];
        // Damage at several places: each must give the same error, with the same line number.
        for at in [5, 800, 1701, 2399] {
            let mut lines: Vec<&str> = whole.lines().collect();
            lines[at & !1] = "ten";
            files.push(("a code that is no number", lines.join("\n")));
            let mut lines: Vec<&str> = whole.lines().collect();
            lines.insert(at | 1, "");
            files.push(("a blank line that shifts the rest", lines.join("\n")));
            let mut lines: Vec<&str> = whole.lines().collect();
            lines.truncate(at | 1);
            files.push(("cut off in the middle of a value", lines.join("\n")));
            let blanks = "\n".repeat(600);
            let mut lines: Vec<&str> = whole.lines().collect();
            lines.insert(at & !1, &blanks);
            files.push(("many blank lines in the middle", lines.join("\n")));
        }
        for (what, text) in &files {
            let expected = one_by_one(text);
            for runs in 2..=9 {
                let shared = groups_shared(text, runs).map_err(|e| e.0);
                if let (Ok(shared), Ok(expected)) = (&shared, &expected) {
                    let differs = shared.iter().zip(expected).position(|(a, b)| a != b);
                    assert!(
                        differs.is_none() && shared.len() == expected.len(),
                        "{what}, cut into {runs}: {} groups for {}, first differing {differs:?}: {:?} for {:?}; starts {:?}",
                        shared.len(),
                        expected.len(),
                        differs.map(|at| &shared[at]),
                        differs.map(|at| &expected[at]),
                        run_starts(text, runs)
                    );
                }
                assert_eq!(shared, expected, "{what}, cut into {runs}");
            }
        }
        assert!(one_by_one(&whole).is_ok_and(|groups| groups.len() == 303), "the test's own drawing reads");
    }

    #[test]
    fn runs_start_on_a_group_codes_line() {
        let text = drawing(50, "\n");
        for count in 2..=7 {
            let starts = run_starts(&text, count);
            assert!(starts.len() > 1, "{count}");
            for (start, lines_before) in starts {
                assert_eq!(text[..start].matches('\n').count(), lines_before);
                assert!(lines_before % 2 == 0 && (start == 0 || text.as_bytes()[start - 1] == b'\n'));
            }
        }
    }
}
