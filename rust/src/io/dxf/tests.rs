use super::*;
use crate::model::dimension::DimensionLook;
use crate::model::entities::{arc, circle, dimension, line, polyline, text};

fn round_trip(document: &Document) -> Document {
    let bytes = dxf_from_document(document).to_bytes();
    document_from_dxf(DxfFile::parse(&bytes).expect("written files read back"))
}

/// The shapes, with each dimension's line point moved to where the file keeps it: any point on the dimension line
/// gives the same dimension, and DXF stores the one at the second extension line.
fn shapes(document: &Document) -> Vec<Shape> {
    document
        .entities()
        .iter()
        .map(|e| match e.shape {
            Shape::Dimension { start, end, line_point, angle, ref look } => {
                let line_point = dimension_parts(start, end, line_point, angle, look).dimension_line.1;
                Shape::Dimension { start, end, line_point, angle, look: look.clone() }
            }
            ref other => other.clone(),
        })
        .collect()
}

#[test]
fn every_kind_of_entity_survives_a_round_trip() {
    let mut styled = line(pt(0.0, 0.0), pt(10.0, 5.0));
    styled.color = 1;
    styled.linetype = "Dashed".into();
    let entities = vec![
        styled,
        circle(pt(1.0, 2.0), 3.5),
        arc(pt(0.0, 0.0), 2.0, 30.0, 300.0),
        polyline(
            vec![
                Vertex { x: 0.0, y: 0.0, bulge: 0.5, start_width: 0.0, end_width: 0.0 },
                Vertex { x: 4.0, y: 0.0, bulge: 0.0, start_width: 0.0, end_width: 0.0 },
                Vertex { x: 4.0, y: 3.0, bulge: 0.0, start_width: 0.0, end_width: 0.0 },
            ],
            true,
        ),
        text(pt(5.0, 5.0), "Größe ⌀ 20", 2.5, 45.0),
        dimension(pt(0.0, 0.0), pt(10.0, 0.0), pt(5.0, 4.0), 0.0, 2.0),
    ];
    let document = Document::with_entities(entities);
    let back = round_trip(&document);
    assert_eq!(shapes(&back), shapes(&document));
    assert_eq!(back.entities()[0].color, 1);
    assert_eq!(back.entities()[0].linetype, "Dashed");
    assert!(back.skipped.is_empty(), "{:?}", back.skipped);
}

#[test]
fn extended_data_of_other_programs_survives_on_objects_linewright_writes() {
    let entities = vec![line(pt(0.0, 0.0), pt(10.0, 5.0)), dimension(pt(0.0, 0.0), pt(10.0, 0.0), pt(5.0, 4.0), 0.0, 2.0)];
    let mut file = dxf_from_document(&Document::with_entities(entities));
    let theirs = [pair(1001, "SURVEY"), pair(1000, "peg 14"), pair(1040, "2.5")];
    for kind in ["LINE", "DIMENSION"] {
        let (start, end) = file.section("ENTITIES").unwrap();
        let group = file.groups[start..end].iter_mut().find(|g| g.kind() == kind).expect("written");
        if kind == "LINE" {
            // AutoCAD's own data goes with how it draws the object, and is not kept.
            group.0.extend([pair(1001, "ACAD"), pair(1000, "not kept")]);
        }
        group.0.extend(theirs.iter().cloned());
    }
    let mut document = document_from_dxf(DxfFile::parse(&file.to_bytes()).unwrap());
    // Editing an object keeps what was attached to it.
    let mut moved = document.entities()[0].clone();
    moved.shape = Shape::Line { start: pt(1.0, 1.0), end: pt(11.0, 6.0) };
    document.modify(&moved);
    let saved = dxf_from_document(&round_trip(&document));
    for kind in ["LINE", "DIMENSION"] {
        let (start, end) = saved.section("ENTITIES").unwrap();
        let group = saved.groups[start..end].iter().find(|g| g.kind() == kind).expect("written");
        let theirs_at = group.0.iter().position(|p| p.code == 1001 && p.value == "SURVEY").unwrap_or_else(|| panic!("{kind} lost it"));
        assert_eq!(group.0[theirs_at..theirs_at + 3], theirs, "{kind}");
        assert_eq!(group.0.iter().filter(|p| p.value == "SURVEY").count(), 1, "{kind} has it once");
        assert!(!group.0.iter().any(|p| p.value == "not kept"), "{kind}");
    }
}

#[test]
fn named_plot_styles_are_read_and_kept_on_objects_linewright_writes() {
    let mut file = dxf_from_document(&Document::with_entities(vec![line(pt(0.0, 0.0), pt(10.0, 5.0)), circle(pt(1.0, 1.0), 2.0)]));
    // A drawing in named plot style mode, with a style "Thin" whose entry has handle ABC, used by the line and by layer 0.
    let header_at = file.section("HEADER").unwrap().0;
    let header = &mut file.groups[header_at].0;
    let at = header.iter().position(|p| p.code == 9 && p.value == "$PSTYLEMODE").expect("the template has it");
    header[at + 1].value = "0".into();
    let (objects, _) = file.section("OBJECTS").unwrap();
    let root = &file.groups[objects + 1].0;
    let at = root.iter().position(|p| p.code == 3 && p.value == "ACAD_PLOTSTYLENAME").expect("the template has the dictionary");
    let dictionary = object_index(&file, root[at + 1].value.trim()).unwrap();
    file.groups[dictionary].0.extend([pair(3, "Thin"), pair(350, "ABC")]);
    let (start, end) = file.section("ENTITIES").unwrap();
    let line_at = (start..end).find(|&i| file.groups[i].kind() == "LINE").unwrap();
    let at = file.groups[line_at].0.iter().position(|p| p.code == 100 && p.value == "AcDbLine").unwrap();
    file.groups[line_at].0.insert(at, pair(390, "ABC"));
    let layer = file.table_entries("LAYER").into_iter().find(|&i| file.groups[i].get(2) == Some("0")).unwrap();
    file.groups[layer].set(390, "ABC".into());
    let document = document_from_dxf(DxfFile::parse(&file.to_bytes()).unwrap());
    assert!(document.named_plot_styles);
    let [line, circle] = document.entities() else { panic!("two objects") };
    assert_eq!(document.plot_style_of(line.id, &line.layer), "Thin", "its own");
    assert_eq!(document.plot_style_of(circle.id, &circle.layer), "Thin", "ByLayer: layer 0's");
    // Saved again, the line still names its style, inside its AcDbEntity part.
    let saved = dxf_from_document(&document);
    let (start, end) = saved.section("ENTITIES").unwrap();
    let line = saved.groups[start..end].iter().find(|g| g.kind() == "LINE").unwrap();
    let style = line.0.iter().position(|p| p.code == 390).expect("kept");
    let kind = line.0.iter().position(|p| p.code == 100 && p.value == "AcDbLine").unwrap();
    assert!(style < kind && line.0[style].value == "ABC");
}

#[test]
fn a_new_drawing_is_in_millimeters() {
    let back = round_trip(&Document::new());
    assert_eq!(unit_names(&back), ("millimeters", "mm"));
    assert_eq!(unit_names(&Document::new()), ("millimeters", "mm"));
}

#[test]
fn saving_twice_does_not_duplicate_anything() {
    let document =
        Document::with_entities(vec![line(pt(0.0, 0.0), pt(1.0, 1.0)), dimension(pt(0.0, 0.0), pt(3.0, 0.0), pt(1.0, 1.0), 0.0, 1.0)]);
    let once = round_trip(&document);
    let twice = round_trip(&once);
    assert_eq!(twice.len(), 2);
    let file = dxf_from_document(&twice);
    let dimension_blocks = file.groups.iter().filter(|g| g.kind() == "BLOCK" && g.get(2).is_some_and(|n| n.starts_with("*D"))).count();
    assert_eq!(dimension_blocks, 1, "the old dimension block is replaced, not kept");
}

#[test]
fn layer_switches_are_kept() {
    let mut document = Document::new();
    document.layers.push(Layer { color: 3, linetype: "Hidden".into(), visible: false, locked: true, ..Layer::new("Walls") });
    document.layers.push(Layer { color: 5, frozen: true, plot: false, ..Layer::new("Doors") });
    let mut on_walls = line(pt(0.0, 0.0), pt(1.0, 0.0));
    on_walls.layer = "Walls".into();
    document.add(on_walls);
    let back = round_trip(&document);
    let walls = back.layer("Walls").unwrap();
    assert_eq!((walls.color, walls.linetype.as_str(), walls.visible, walls.locked), (3, "Hidden", false, true));
    let doors = back.layer("Doors").unwrap();
    assert!(doors.visible && doors.frozen && !doors.plot && !doors.shown());
}

#[test]
fn deleted_layers_leave_the_file() {
    let mut document = Document::new();
    document.layers.push(Layer::new("Temporary"));
    let mut back = round_trip(&document);
    assert!(back.layer("Temporary").is_some());
    back.layers.retain(|l| l.name != "Temporary");
    let again = round_trip(&back);
    assert!(again.layer("Temporary").is_none());
    assert!(again.layer("Defpoints").is_some(), "Defpoints always stays");
}

const R12: &str = "  0\nSECTION\n  2\nHEADER\n  9\n$ACADVER\n  1\nAC1009\n  9\n$INSUNITS\n 70\n     1\n  0\nENDSEC\n  0\nSECTION\n  2\nTABLES\n  0\nTABLE\n  2\nLAYER\n 70\n     1\n  0\nLAYER\n  2\nWALLS\n 70\n     1\n 62\n     2\n  6\nCONTINUOUS\n  0\nENDTAB\n  0\nENDSEC\n  0\nSECTION\n  2\nENTITIES\n  0\nLINE\n  8\nwalls\n 10\n0.0\n 20\n0.0\n 30\n0.0\n 11\n5.0\n 21\n0.0\n 31\n0.0\n  0\nPOLYLINE\n  8\n0\n 66\n     1\n 70\n     1\n  0\nVERTEX\n  8\n0\n 10\n0.0\n 20\n0.0\n 42\n1.0\n  0\nVERTEX\n  8\n0\n 10\n2.0\n 20\n0.0\n  0\nSEQEND\n  8\n0\n  0\n3DFACE\n  8\n0\n 10\n1.0\n 20\n1.0\n 30\n0.0\n 11\n2.0\n 21\n1.0\n 31\n0.0\n 12\n2.0\n 22\n2.0\n 32\n0.0\n 13\n2.0\n 23\n2.0\n 33\n0.0\n  0\nCIRCLE\n  8\n0\n 10\n0.0\n 20\n0.0\n 40\n1.0\n210\n0.0\n220\n0.0\n230\n-1.0\n  0\nENDSEC\n  0\nEOF\n";

#[test]
fn reads_an_old_file() {
    let document = document_from_dxf(DxfFile::parse(R12.as_bytes()).unwrap());
    assert_eq!(unit_names(&document), ("inches", "in"));
    assert_eq!(document.len(), 2);
    assert_eq!(document.entities()[0].layer, "WALLS", "the layer name is spelled as the table defines it");
    let walls = document.layer("WALLS").unwrap();
    assert!(walls.frozen && walls.visible && !walls.shown(), "frozen, which hides it, though not turned off");
    let Shape::Polyline { vertices, closed } = &document.entities()[1].shape else { panic!() };
    assert!(*closed && vertices.len() == 2 && vertices[0].bulge == 1.0);
    assert_eq!(document.skipped.get("3DFACE"), Some(&1));
    assert_eq!(document.skipped.get("CIRCLE"), Some(&1), "circles off the drawing plane are not shown");
}

#[test]
fn what_cannot_be_shown_is_kept_when_saving() {
    let document = document_from_dxf(DxfFile::parse(R12.as_bytes()).unwrap());
    let file = dxf_from_document(&document);
    let kinds: Vec<&str> = file.groups.iter().map(|g| g.kind()).collect();
    assert!(kinds.contains(&"3DFACE"));
    assert_eq!(kinds.iter().filter(|k| **k == "CIRCLE").count(), 1);
    assert!(kinds.contains(&"POLYLINE") && !kinds.contains(&"LWPOLYLINE"), "R12 has no light-weight polylines");
    let back = document_from_dxf(DxfFile::parse(&file.to_bytes()).unwrap());
    assert_eq!(shapes(&back), shapes(&document));
}

#[test]
fn mtext_formatting_is_dropped() {
    assert_eq!(plain_mtext("{\\fArial|b1;Bold} line\\Pnext \\S1^2; \\\\path"), "Bold line\nnext 1/2 \\path");
}

#[test]
fn dimension_scale_comes_from_its_overrides_or_its_style() {
    let file = dxf_from_document(&Document::with_entities(vec![dimension(pt(0.0, 0.0), pt(1.0, 0.0), pt(0.0, 1.0), 0.0, 3.0)]));
    let back = document_from_dxf(DxfFile::parse(&file.to_bytes()).unwrap());
    let Shape::Dimension { look, .. } = &back.entities()[0].shape else { panic!() };
    assert_eq!(look.scale, 3.0);
}

#[test]
fn text_placed_away_from_the_middle_stays_there() {
    let look = DimensionLook { text_position: Some(pt(20.0, 3.0)), ..DimensionLook::default() };
    let shape = Shape::Dimension { start: pt(0.0, 0.0), end: pt(2.5, 0.0), line_point: pt(2.5, 1.0), angle: 0.0, look };
    let back = round_trip(&Document::with_entities(vec![Entity::new(shape)]));
    let Shape::Dimension { look, .. } = &back.entities()[0].shape else { panic!() };
    assert_eq!(look.text_position, Some(pt(20.0, 3.0)));
}

#[test]
fn rubbish_is_refused() {
    assert!(DxfFile::parse(b"hello\nworld\n").is_err());
    assert!(DxfFile::parse(b"AutoCAD Binary DXF\r\n\x1a\x00").is_err());
}

#[test]
fn a_dimension_keeps_its_own_look() {
    let look = DimensionLook {
        text_height: 0.1,
        arrow_size: 0.05,
        extension_gap: 0.02,
        extension_overshoot: 0.05,
        text_gap: 0.03,
        decimals: 3,
        length_factor: 1000.0,
        arrowhead: crate::model::dimension::Arrowhead::Tick,
        trim_zeros: true,
        text: "<> mm".into(),
        ..DimensionLook::default()
    };
    let shape = Shape::Dimension { start: pt(0.0, 0.0), end: pt(2.5, 0.0), line_point: pt(2.5, 1.0), angle: 0.0, look: look.clone() };
    let back = round_trip(&Document::with_entities(vec![Entity::new(shape)]));
    let Shape::Dimension { look: read, .. } = &back.entities()[0].shape else { panic!() };
    assert_eq!(*read, look);
}

#[test]
fn a_dimension_without_overrides_takes_its_style() {
    let mut file = dxf_from_document(&Document::with_entities(vec![dimension(pt(0.0, 0.0), pt(2.5, 0.0), pt(0.0, 1.0), 0.0, 1.0)]));
    // A drawing in meters: small text, architectural ticks, three decimals, set on the style alone.
    for group in file.groups.iter_mut() {
        if group.kind() == "DIMENSION" {
            group.0.truncate(group.0.iter().position(|p| p.code == 1001).unwrap());
        }
        if group.kind() == "DIMSTYLE" && group.get(2) == Some("Standard") {
            for (code, value) in [(140, "0.1"), (142, "0.04"), (271, "3"), (40, "1.0")] {
                group.set(code, value.to_string());
            }
        }
    }
    let back = document_from_dxf(DxfFile::parse(&file.to_bytes()).unwrap());
    let Shape::Dimension { look, .. } = &back.entities()[0].shape else { panic!() };
    assert_eq!(
        (look.text_height, look.arrowhead, look.arrow_size, look.decimals),
        (0.1, crate::model::dimension::Arrowhead::Tick, 0.04, 3)
    );
}

/// An old file in a code page, with one TEXT whose value is the given bytes.
fn old_file_with_text(code_page: &str, text: &[u8]) -> Vec<u8> {
    let mut bytes = format!(
        "  0\nSECTION\n  2\nHEADER\n  9\n$ACADVER\n  1\nAC1015\n  9\n$DWGCODEPAGE\n  3\n{code_page}\n  0\nENDSEC\n  0\nSECTION\n  2\nENTITIES\n  0\nTEXT\n  8\n0\n 10\n0.0\n 20\n0.0\n 40\n2.5\n  1\n"
    )
    .into_bytes();
    bytes.extend_from_slice(text);
    bytes.extend_from_slice(b"\n  0\nENDSEC\n  0\nEOF\n");
    bytes
}

fn first_text(document: &Document) -> String {
    match &document.entities()[0].shape {
        Shape::Text { text, .. } => text.clone(),
        other => panic!("not text: {other:?}"),
    }
}

#[test]
fn old_files_are_read_in_their_code_page() {
    let (thai, _, _) = encoding_rs::WINDOWS_874.encode("สวัสดี");
    let document = document_from_dxf(DxfFile::parse(&old_file_with_text("ANSI_874", &thai)).unwrap());
    assert_eq!(first_text(&document), "สวัสดี");
    let (russian, _, _) = encoding_rs::WINDOWS_1251.encode("Чертёж");
    let document = document_from_dxf(DxfFile::parse(&old_file_with_text("ANSI_1251", &russian)).unwrap());
    assert_eq!(first_text(&document), "Чертёж");
}

#[test]
fn escapes_and_special_codes_are_decoded() {
    let document = document_from_dxf(
        DxfFile::parse(&old_file_with_text("ANSI_1252", b"\\U+0E81\\U+0EB2 \\M+5D6D0 %%c20 45%%d %%p0.1 %%u100%%%")).unwrap(),
    );
    assert_eq!(first_text(&document), "ກາ 中 ⌀20 45° ±0.1 100%");
}

#[test]
fn text_keeps_its_font_and_lines() {
    let mut legacy = text(pt(1.0, 2.0), "âØèð {10}", 2.5, 30.0);
    if let Shape::Text { font, .. } = &mut legacy.shape {
        *font = "Alice4 2000".into();
    }
    let lines = text(pt(0.0, 10.0), "first line\nsecond ⌀ line", 3.0, 0.0);
    let back = round_trip(&Document::with_entities(vec![legacy.clone(), lines.clone()]));
    for (written, read) in [&legacy, &lines].into_iter().zip(back.entities()) {
        let (
            Shape::Text { position: a, text: t, font: f, rotation: r, .. },
            Shape::Text { position: b, text: u, font: g, rotation: s, .. },
        ) = (&written.shape, &read.shape)
        else {
            panic!()
        };
        assert_eq!((t, f), (u, g));
        assert!(a.dist(*b) < 1e-9 && (r - s).abs() < 1e-9, "{a:?} {b:?} {r} {s}");
    }
}

#[test]
fn text_in_an_old_file_is_written_in_its_code_page() {
    let mut document = document_from_dxf(DxfFile::parse(&old_file_with_text("ANSI_1251", b"x")).unwrap());
    document.set_entities(vec![text(pt(0.0, 0.0), "Чертёж ⌀ 中", 2.5, 0.0)]);
    let bytes = dxf_from_document(&document).to_bytes();
    let (russian, _, _) = encoding_rs::WINDOWS_1251.encode("Чертёж");
    assert!(bytes.windows(russian.len()).any(|w| w == &russian[..]));
    assert!(String::from_utf8_lossy(&bytes).contains("%%c \\U+4E2D"), "characters the code page lacks are escaped");
    assert_eq!(first_text(&document_from_dxf(DxfFile::parse(&bytes).unwrap())), "Чертёж ⌀ 中");
}

#[test]
fn radial_angular_dimensions_and_leaders_survive_a_round_trip() {
    use crate::model::annotation::AnnotationKind;
    let look = DimensionLook { angle_decimals: 1, ..DimensionLook::default() };
    let shapes = vec![
        Shape::Annotation {
            kind: AnnotationKind::Radius,
            points: vec![pt(0.0, 0.0), pt(3.0, 4.0)],
            look: DimensionLook { text_position: Some(pt(9.0, 9.0)), ..look.clone() },
        },
        Shape::Annotation { kind: AnnotationKind::Diameter, points: vec![pt(1.0, 1.0), pt(4.0, 1.0)], look: look.clone() },
        Shape::Annotation {
            kind: AnnotationKind::Angle,
            points: vec![pt(0.0, 0.0), pt(10.0, 0.0), pt(0.0, 10.0), pt(4.0, 4.0)],
            look: look.clone(),
        },
        Shape::Annotation {
            kind: AnnotationKind::Leader { arrow: true },
            points: vec![pt(0.0, 0.0), pt(5.0, 5.0), pt(8.0, 5.0)],
            look: look.clone(),
        },
    ];
    let document = Document::with_entities(shapes.iter().cloned().map(Entity::new).collect());
    let back = round_trip(&document);
    let read: Vec<Shape> = back.entities().iter().map(|e| e.shape.clone()).collect();
    assert_eq!(read.len(), 4, "{:?}", back.skipped);
    for (read, written) in read.iter().zip(&shapes) {
        let (Shape::Annotation { kind: k1, points: p1, look: l1 }, Shape::Annotation { kind: k2, points: p2, look: l2 }) = (read, written)
        else {
            panic!()
        };
        assert_eq!(k1, k2);
        assert!(p1.iter().zip(p2).all(|(a, b)| a.dist(*b) < 1e-9), "{p1:?} {p2:?}");
        assert_eq!(l1.text_position, l2.text_position);
        assert_eq!(l1.angle_decimals, 1);
    }
}

#[test]
fn layouts_keep_their_sheets_viewports_and_what_is_drawn_on_them() {
    use crate::model::document::Layout;
    let mut document = Document::with_entities(vec![line(pt(0.0, 0.0), pt(1000.0, 500.0))]);
    let frame = Shape::Viewport {
        center: pt(200.0, 150.0),
        width: 300.0,
        height: 200.0,
        view_center: pt(500.0, 250.0),
        view_height: 2000.0,
        frozen: vec!["0".into()],
        locked: true,
    };
    document.set_space_entities(Some(0), vec![Entity::new(frame.clone()), line(pt(10.0, 10.0), pt(410.0, 10.0))]);
    document.layouts.push(Layout::new("Sheet 2"));
    document.set_space_entities(Some(1), vec![text(pt(20.0, 20.0), "Title", 5.0, 0.0)]);
    let back = round_trip(&document);
    let names: Vec<&str> = back.layouts.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, vec!["Layout1", "Sheet 2"]);
    assert_eq!(back.space_entities(None).len(), 1);
    let first: Vec<Shape> = back.space_entities(Some(0)).iter().map(|e| e.shape.clone()).collect();
    assert_eq!(first.len(), 2, "{first:?}");
    assert!(first.contains(&frame));
    assert!(matches!(back.space_entities(Some(1)), [Entity { shape: Shape::Text { .. }, .. }]));
    // Saved again, nothing doubles up.
    let again = round_trip(&back);
    assert_eq!(again.space_entities(Some(0)).len(), 2);
    assert_eq!(again.space_entities(Some(1)).len(), 1);
    assert_eq!(again.layouts.len(), 2);
}

#[test]
fn true_colors_survive_with_an_index_color_beside_them() {
    use crate::canvas::aci::true_color;
    let mut orange = line(pt(0.0, 0.0), pt(1.0, 0.0));
    orange.color = true_color(255, 128, 0);
    let mut document = Document::with_entities(vec![orange]);
    document.layers[0].color = true_color(10, 20, 30);
    let file = dxf_from_document(&document);
    let text = String::from_utf8_lossy(&file.to_bytes()).into_owned();
    assert!(text.contains("\n420\n16744448\n"), "0xFF8000 as AutoCAD writes it");
    let back = document_from_dxf(DxfFile::parse(&file.to_bytes()).unwrap());
    assert_eq!(back.entities()[0].color, true_color(255, 128, 0));
    assert_eq!(back.layers[0].color, true_color(10, 20, 30));
}

#[test]
fn groups_survive_with_their_members_pointing_back() {
    use crate::model::document::ObjectGroup;
    let (a, b, c) = (line(pt(0.0, 0.0), pt(1.0, 0.0)), line(pt(0.0, 1.0), pt(1.0, 1.0)), circle(pt(5.0, 5.0), 1.0));
    let mut document = Document::with_entities(vec![a.clone(), b.clone(), c.clone()]);
    document.groups.push(ObjectGroup {
        name: "Edges".into(),
        description: "Two edges".into(),
        unnamed: false,
        selectable: true,
        members: vec![a.id, b.id],
    });
    document.groups.push(ObjectGroup {
        name: "*A1".into(),
        description: String::new(),
        unnamed: true,
        selectable: false,
        members: vec![c.id],
    });
    let file = dxf_from_document(&document);
    let text = String::from_utf8_lossy(&file.to_bytes()).into_owned();
    let plain = dxf_from_document(&Document::with_entities(vec![line(pt(0.0, 0.0), pt(1.0, 0.0))]));
    let before = String::from_utf8_lossy(&plain.to_bytes()).matches("{ACAD_REACTORS").count();
    assert_eq!(text.matches("{ACAD_REACTORS").count(), before + 2 + 3, "each group, and each member pointing at it");
    let back = document_from_dxf(DxfFile::parse(&file.to_bytes()).unwrap());
    assert_eq!(back.groups.len(), 2);
    let edges = &back.groups[0];
    assert_eq!((edges.name.as_str(), edges.description.as_str(), edges.members.len()), ("Edges", "Two edges", 2));
    assert!(back.groups[1].unnamed && !back.groups[1].selectable);
    let members: Vec<&Shape> = edges.members.iter().map(|id| &back.get(*id).unwrap().shape).collect();
    assert!(members.contains(&&a.shape) && members.contains(&&b.shape));
    // Saved again, the groups are written once.
    let again = dxf_from_document(&back);
    assert_eq!(String::from_utf8_lossy(&again.to_bytes()).matches("AcDbGroup").count(), 2);
}

#[test]
fn construction_lines_survive_and_stay_out_of_the_extent() {
    let xline = Entity::new(Shape::XLine { base: pt(1.0, 2.0), direction: pt(0.6, 0.8), ray: false });
    let ray = Entity::new(Shape::XLine { base: pt(0.0, 0.0), direction: pt(1.0, 0.0), ray: true });
    let document = Document::with_entities(vec![xline.clone(), ray.clone(), line(pt(0.0, 0.0), pt(5.0, 5.0))]);
    assert_eq!(document.extent().unwrap().x1, 5.0);
    let back = round_trip(&document);
    assert_eq!(shapes(&back)[..2], [xline.shape, ray.shape]);
}

#[test]
fn units_precision_and_purged_styles_reach_the_file() {
    let mut document = Document::with_entities(vec![line(pt(0.0, 0.0), pt(1.0, 0.0))]);
    document.dimension_styles.push(crate::model::dimension::DimensionStyle { name: "Old".into(), look: DimensionLook::default() });
    let with_old = round_trip(&document);
    assert!(with_old.dimension_style("Old").is_some());
    let mut purged = with_old;
    purged.dimension_styles.retain(|s| s.name != "Old");
    purged.units = Some(6);
    purged.precision = 2;
    let back = round_trip(&purged);
    assert!(back.dimension_style("Old").is_none(), "a purged style leaves the file");
    assert_eq!(unit_names(&back), ("meters", "m"));
    assert_eq!(back.precision, 2);
}

#[test]
fn a_block_changed_in_the_block_editor_is_saved_once_with_its_new_contents() {
    use crate::model::entities::{BlockDef, BlockRef};
    let block = BlockRef(std::sync::Arc::new(BlockDef {
        name: "Chair".into(),
        base: pt(0.0, 0.0),
        entities: vec![line(pt(0.0, 0.0), pt(1.0, 0.0))],
        attribute_defs: vec![],
        xref: None,
    }));
    let insert =
        Entity::new(Shape::Insert { block, position: pt(5.0, 5.0), scale_x: 1.0, scale_y: 1.0, rotation: 0.0, attributes: vec![] });
    let mut document = round_trip(&Document::with_entities(vec![insert]));
    let chair = document.blocks.get("CHAIR").cloned().unwrap();
    document.begin_block_edit(chair.clone());
    assert_eq!(document.len(), 1, "the block's own objects are open");
    document.add(circle(pt(0.0, 0.0), 2.0));
    document.end_block_edit(false);
    assert_eq!(document.blocks["CHAIR"].0.entities.len(), 1, "discarded");
    document.begin_block_edit(chair);
    document.add(circle(pt(0.0, 0.0), 2.0));
    document.end_block_edit(true);
    let Shape::Insert { block, .. } = &document.entities()[0].shape else { panic!() };
    assert_eq!(block.0.entities.len(), 2, "every insert shows the new block");
    let back = round_trip(&document);
    assert_eq!(back.blocks["CHAIR"].0.entities.len(), 2);
    let blocks_named = dxf_from_document(&back).groups.iter().filter(|g| g.kind() == "BLOCK" && g.get(2) == Some("Chair")).count();
    assert_eq!(blocks_named, 1, "defined once");
}

#[test]
fn attribute_definitions_survive_on_their_own_and_in_the_block_editor() {
    use crate::model::entities::{AttributeInfo, BlockDef, BlockRef};
    let mut definition = text(pt(1.0, 2.0), "ROOM", 2.5, 0.0);
    definition.attribute = Some(AttributeInfo { prompt: "Room number?".into(), default: "101".into(), invisible: true });
    let back = round_trip(&Document::with_entities(vec![definition.clone()]));
    let read = &back.entities()[0];
    assert_eq!((read.attribute.clone(), &read.shape), (definition.attribute.clone(), &definition.shape));
    // In the block editor a block's attributes are definitions, and saving makes them attributes again.
    let block = BlockRef(std::sync::Arc::new(BlockDef {
        name: "Door".into(),
        base: pt(0.0, 0.0),
        entities: vec![],
        attribute_defs: vec![],
        xref: None,
    }));
    let mut document = Document::new();
    document.blocks.insert("DOOR".into(), block.clone());
    document.begin_block_edit(block);
    document.add(definition);
    let saved = document.end_block_edit(true).unwrap();
    assert_eq!(saved.0.attribute_defs.len(), 1);
    assert!(saved.0.entities.is_empty());
    document.begin_block_edit(saved);
    assert!(document.entities()[0].attribute.is_some(), "it opens again as a definition");
}

#[test]
fn tables_survive_as_acad_tables_with_their_style() {
    let table = Shape::Table {
        position: pt(10.0, 50.0),
        columns: vec![30.0, 20.0, 25.0],
        rows: vec![10.0, 8.0, 8.0],
        cells: vec![
            vec!["Door schedule".into(), String::new(), String::new()],
            vec!["D1".into(), "900".into(), "Oak".into()],
            vec!["D2".into(), "800".into(), "Pine, painted\nwhite".into()],
        ],
        text_height: 2.5,
        title: true,
    };
    let document = Document::with_entities(vec![Entity::new(table.clone())]);
    let file = dxf_from_document(&document);
    let text = String::from_utf8_lossy(&file.to_bytes()).into_owned();
    assert!(text.contains("ACAD_TABLE") && text.contains("TABLESTYLE") && text.contains("*T1"));
    let back = document_from_dxf(DxfFile::parse(&file.to_bytes()).unwrap());
    let Shape::Table { position, columns, rows, cells, title, .. } = &back.entities()[0].shape else { panic!("{:?}", back.skipped) };
    let Shape::Table { cells: written, .. } = &table else { panic!() };
    assert_eq!((position, columns, rows, cells, title), (&pt(10.0, 50.0), &vec![30.0, 20.0, 25.0], &vec![10.0, 8.0, 8.0], written, &true));
    // Saved again: one table, one block for it, one Standard style.
    let again = String::from_utf8_lossy(&dxf_from_document(&back).to_bytes()).into_owned();
    assert_eq!(again.matches("\nACAD_TABLE\n").count(), 1);
    assert_eq!(again.matches("\nAcDbTableStyle\n").count(), text.matches("\nAcDbTableStyle\n").count(), "the style is not made twice");
}

#[test]
fn lineweights_and_plot_styles_survive_a_round_trip() {
    use crate::model::entities::{BlockDef, BlockRef};
    use crate::model::lineweight::{BYBLOCK, BYLAYER, DEFAULT};
    let mut document = Document::new();
    document.layers.push(Layer { lineweight: 50, ..Layer::new("Thick") });
    let mut own = line(pt(0.0, 0.0), pt(1.0, 0.0));
    own.lineweight = 30;
    let mut by_layer = circle(pt(0.0, 0.0), 1.0);
    by_layer.layer = "Thick".into();
    let mut by_block = line(pt(0.0, 0.0), pt(0.0, 1.0));
    by_block.lineweight = BYBLOCK;
    let block = BlockRef(std::sync::Arc::new(BlockDef {
        name: "Mark".into(),
        base: pt(0.0, 0.0),
        entities: vec![by_block],
        attribute_defs: vec![],
        xref: None,
    }));
    let mut insert =
        Entity::new(Shape::Insert { block, position: pt(5.0, 5.0), scale_x: 1.0, scale_y: 1.0, rotation: 0.0, attributes: vec![] });
    insert.lineweight = 100;
    for entity in [own, by_layer, insert] {
        document.add(entity);
    }
    document.current_lineweight = 18;
    document.default_lineweight = 35;
    document.show_lineweights = true;
    document.model_plot = PlotSettings { style_table: "monochrome.ctb".into(), lineweights: true };
    document.layouts[0].plot = PlotSettings { style_table: String::new(), lineweights: false };

    let back = round_trip(&document);
    let weights: Vec<i16> = back.entities().iter().map(|e| e.lineweight).collect();
    assert_eq!(weights, vec![30, BYLAYER, 100]);
    assert_eq!(back.layer("Thick").unwrap().lineweight, 50);
    assert_eq!(back.layer("0").unwrap().lineweight, DEFAULT);
    let Shape::Insert { block, .. } = &back.entities()[2].shape else { panic!() };
    assert_eq!(block.0.entities[0].lineweight, BYBLOCK);
    let pieces = crate::model::parts::flatten(&back.entities()[2], 0.01);
    assert_eq!(pieces[0].0.lineweight, 100, "a ByBlock part takes its insert's lineweight");
    assert_eq!((back.current_lineweight, back.default_lineweight, back.show_lineweights), (18, 35, true));
    assert_eq!(back.model_plot, document.model_plot);
    assert_eq!(back.layouts[0].plot, document.layouts[0].plot);
}

#[test]
fn new_dimension_kinds_and_tolerances_survive_a_round_trip() {
    use crate::model::dimension::Tolerance;
    let look = DimensionLook {
        tolerance: Tolerance::Deviation,
        tolerance_plus: 0.1,
        tolerance_minus: 0.05,
        tolerance_height: 0.7,
        text_rotation: Some(30.0),
        ..DimensionLook::default()
    };
    let annotation = |kind, points: Vec<Point>| Entity::new(Shape::Annotation { kind, points, look: look.clone() });
    let entities = vec![
        annotation(AnnotationKind::Ordinate { x_datum: true }, vec![pt(0.0, 0.0), pt(5.0, 5.0), pt(5.0, 20.0)]),
        annotation(AnnotationKind::Ordinate { x_datum: false }, vec![pt(0.0, 0.0), pt(5.0, 5.0), pt(20.0, 5.0)]),
        annotation(AnnotationKind::ArcLength, vec![pt(0.0, 0.0), pt(10.0, 0.0), pt(0.0, 10.0), pt(14.0, 14.0)]),
        annotation(AnnotationKind::JoggedRadius, vec![pt(0.0, 0.0), pt(100.0, 0.0), pt(70.0, 10.0), pt(85.0, 0.0)]),
        Entity::new(Shape::Dimension { start: pt(0.0, 0.0), end: pt(10.0, 0.0), line_point: pt(5.0, 8.0), angle: 0.0, look: look.clone() }),
    ];
    let document = Document::with_entities(entities.clone());
    let back = round_trip(&document);
    assert_eq!(back.entities().len(), entities.len(), "skipped: {:?}", back.skipped);
    for (read, written) in back.entities().iter().zip(&entities) {
        let (Shape::Annotation { kind: k1, points: p1, look: l1 }, Shape::Annotation { kind: k2, points: p2, .. }) =
            (&read.shape, &written.shape)
        else {
            let Shape::Dimension { look: l1, .. } = &read.shape else { panic!() };
            assert_eq!(l1.tolerance, Tolerance::Deviation);
            continue;
        };
        assert_eq!((k1, p1), (k2, p2));
        assert_eq!((l1.tolerance, l1.tolerance_plus, l1.tolerance_minus, l1.tolerance_height), (Tolerance::Deviation, 0.1, 0.05, 0.7));
        assert!((l1.text_rotation.unwrap() - 30.0).abs() < 1e-9);
    }
}

#[test]
fn multileaders_and_their_styles_survive_a_round_trip() {
    use crate::model::multileader::MleaderStyle;
    let mut document = Document::new();
    document.mleader_styles.push(MleaderStyle { name: "Notes".into(), text_height: 3.5, arrow_size: 2.0, dogleg: 6.0, landing_gap: 1.0 });
    document.current_mleader_style = "Notes".into();
    let shape = Shape::Multileader {
        leaders: vec![vec![pt(0.0, 0.0), pt(5.0, 10.0)], vec![pt(0.0, 20.0)]],
        landing: pt(10.0, 14.0),
        dogleg: -6.0,
        text: "SEE NOTE\n3".into(),
        text_position: pt(3.0, 15.75),
        text_height: 3.5,
        arrow_size: 2.0,
        style: "Notes".into(),
    };
    document.add(Entity::new(shape.clone()));
    let back = round_trip(&document);
    assert_eq!(back.entities()[0].shape, shape, "skipped: {:?}", back.skipped);
    assert_eq!(back.current_mleader_style, "Notes");
    assert!(back.mleader_styles.iter().any(|s| s.name == "Notes" && s.text_height == 3.5 && s.dogleg == 6.0));
}

#[test]
fn hatch_settings_gradients_and_links_survive_a_round_trip() {
    use crate::model::entities::{Association, Gradient};
    let boundary = circle(pt(0.0, 0.0), 5.0);
    let loops = vec![vec![pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)]];
    let (solid, pattern) = crate::model::editing::hatch_pattern("LINE", 2.0, 30.0).unwrap();
    let hatch = Entity::new(Shape::Hatch {
        loops: loops.clone(),
        solid,
        pattern,
        name: "LINE".into(),
        scale: 2.0,
        angle: 30.0,
        gradient: None,
        association: None,
    });
    let gradient = Gradient { name: "CYLINDER".into(), first: (10, 20, 30), second: (200, 100, 50), angle: 45.0 };
    let filled = Entity::new(Shape::Hatch {
        loops: loops.clone(),
        solid: true,
        pattern: vec![],
        name: "CYLINDER".into(),
        scale: 1.0,
        angle: 0.0,
        gradient: Some(gradient.clone()),
        association: None,
    });
    let linked = Entity::new(Shape::Hatch {
        loops,
        solid: true,
        pattern: vec![],
        name: "SOLID".into(),
        scale: 1.0,
        angle: 0.0,
        gradient: None,
        association: Some(Association::Objects(vec![boundary.id])),
    });
    let back = round_trip(&Document::with_entities(vec![boundary, hatch, filled, linked]));
    let Shape::Hatch { scale, angle, .. } = &back.entities()[1].shape else { panic!() };
    assert_eq!((*scale, *angle), (2.0, 30.0));
    let Shape::Hatch { gradient: Some(read), .. } = &back.entities()[2].shape else { panic!() };
    assert_eq!(read.name, gradient.name);
    assert_eq!((read.first, read.second), (gradient.first, gradient.second));
    assert!((read.angle - 45.0).abs() < 1e-9);
    let Shape::Hatch { association, .. } = &back.entities()[3].shape else { panic!() };
    assert_eq!(*association, Some(Association::Objects(vec![back.entities()[0].id])));
}

#[test]
fn polyline_widths_survive_a_round_trip() {
    let vertices = vec![
        Vertex { x: 0.0, y: 0.0, bulge: 0.0, start_width: 2.0, end_width: 2.0 },
        Vertex { x: 10.0, y: 0.0, bulge: 0.5, start_width: 2.0, end_width: 0.5 },
        Vertex { x: 20.0, y: 5.0, bulge: 0.0, start_width: 0.0, end_width: 0.0 },
    ];
    let shape = Shape::Polyline { vertices, closed: false };
    assert_eq!(round_trip(&Document::with_entities(vec![Entity::new(shape.clone())])).entities()[0].shape, shape);
}

#[test]
fn text_styles_and_formatted_mtext_survive_a_round_trip() {
    use crate::model::document::TextStyleDef;
    let mut document = Document::new();
    document.text_styles.push(TextStyleDef {
        name: "Notes".into(),
        font: "romans.shx".into(),
        height: 3.5,
        width_factor: 0.8,
        oblique: 15.0,
    });
    document.current_text_style = "Notes".into();
    let raw = "Plain {\\fArial|b1|i0|c0|p0;bold}\\P{\\H2x;big} \\S1/2;";
    let lines = crate::model::mtext::parse(raw, 2.5);
    let mut rich = text(pt(0.0, 0.0), &crate::model::mtext::plain(&lines), 2.5, 0.0);
    if let Shape::Text { formatted, align, .. } = &mut rich.shape {
        *formatted = raw.to_string();
        align.v = VAlign::Top;
    }
    let mut leaning = text(pt(0.0, 9.0), "lean", 2.5, 0.0);
    if let Shape::Text { width_factor, oblique, style, font, .. } = &mut leaning.shape {
        (*width_factor, *oblique, *style, *font) = (0.8, 15.0, "Notes".into(), "romans.shx".into());
    }
    document.add(rich.clone());
    document.add(leaning);
    let back = round_trip(&document);
    let style = back.text_styles.iter().find(|s| s.name == "Notes").unwrap();
    assert_eq!((style.font.as_str(), style.height, style.width_factor, style.oblique), ("romans.shx", 3.5, 0.8, 15.0));
    assert_eq!(back.current_text_style, "Notes");
    let Shape::Text { formatted, text: shown, .. } = &back.entities()[0].shape else { panic!() };
    let Shape::Text { text: expected, .. } = &rich.shape else { panic!() };
    assert_eq!((formatted.as_str(), shown), (raw, expected));
    let Shape::Text { width_factor, oblique, .. } = &back.entities()[1].shape else { panic!() };
    assert_eq!((*width_factor, *oblique), (0.8, 15.0));
}

#[test]
fn a_wipeout_survives_a_round_trip_with_its_class_declared() {
    let corners = vec![pt(2.0, 1.0), pt(12.0, 1.0), pt(9.0, 6.0), pt(2.0, 4.0)];
    let document = Document::with_entities(vec![Entity::new(Shape::Wipeout { corners: corners.clone() })]);
    let file = dxf_from_document(&document);
    let class = file.groups.iter().filter(|g| g.kind() == "CLASS" && g.get(1).is_some_and(|n| n.trim() == "WIPEOUT")).count();
    assert_eq!(class, 1);
    let back = round_trip(&document);
    let Shape::Wipeout { corners: read } = &back.entities()[0].shape else { panic!("{:?}", back.entities()[0].shape) };
    assert_eq!(read.len(), 4);
    for (a, b) in read.iter().zip(&corners) {
        assert!(a.dist(*b) < 1e-9, "{a:?} {b:?}");
    }
    assert!(back.skipped.is_empty(), "{:?}", back.skipped);
}

#[test]
fn a_rectangular_wipeout_clip_gives_four_corners() {
    let pairs = [(0, "WIPEOUT"), (8, "0"), (10, "10.0"), (20, "20.0"), (11, "4.0"), (21, "0.0"), (12, "0.0"), (22, "2.0")]
        .into_iter()
        .chain([(13, "1.0"), (23, "1.0"), (71, "1"), (91, "2"), (14, "-0.5"), (24, "-0.5"), (14, "0.5"), (24, "0.5")])
        .map(|(code, value)| pair(code, value))
        .collect();
    let Some(Shape::Wipeout { corners }) = read_wipeout(&Group(pairs)) else { panic!() };
    let mut seen: Vec<(f64, f64)> = corners.iter().map(|c| (c.x, c.y)).collect();
    seen.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(seen, vec![(10.0, 20.0), (10.0, 22.0), (14.0, 20.0), (14.0, 22.0)]);
}

#[test]
fn the_wipeout_frame_mode_survives_a_round_trip_and_is_written_only_when_needed() {
    let plain = Document::with_entities(vec![line(pt(0.0, 0.0), pt(1.0, 1.0))]);
    let file = dxf_from_document(&plain);
    assert!(!file.groups.iter().any(|g| g.kind() == "WIPEOUTVARIABLES"), "the default mode needs no object");

    let mut document =
        Document::with_entities(vec![Entity::new(Shape::Wipeout { corners: vec![pt(0.0, 0.0), pt(4.0, 0.0), pt(4.0, 3.0)] })]);
    assert_eq!(round_trip(&document).wipeout_frame, 1, "a drawing starts with its frames on and printed");
    for mode in [0u8, 2] {
        document.wipeout_frame = mode;
        let file = dxf_from_document(&document);
        let object = file.groups.iter().find(|g| g.kind() == "WIPEOUTVARIABLES").expect("the mode is written");
        assert_eq!(object.subclass("AcDbWipeoutVariables").int(70), Some(i64::from(mode)));
        let back = round_trip(&document);
        assert_eq!(back.wipeout_frame, mode);
        assert!(back.skipped.is_empty(), "{:?}", back.skipped);
    }

    // A file that already has the object keeps it and takes the new mode.
    let mut back = round_trip(&document);
    back.wipeout_frame = 1;
    assert_eq!(round_trip(&back).wipeout_frame, 1);

    // A mode other than the default is kept even with no wipeouts to frame.
    let mut quiet = Document::with_entities(vec![line(pt(0.0, 0.0), pt(1.0, 1.0))]);
    quiet.wipeout_frame = 0;
    assert_eq!(round_trip(&quiet).wipeout_frame, 0);
}

#[test]
fn the_linetype_scales_survive_a_round_trip_and_are_written_only_when_needed() {
    let mut scaled = line(pt(0.0, 0.0), pt(1.0, 1.0));
    scaled.linetype_scale = 0.5;
    let mut document = Document::with_entities(vec![line(pt(0.0, 0.0), pt(2.0, 0.0)), scaled]);
    document.ltscale = 2.0;
    document.psltscale = false;
    document.celtscale = 3.0;
    let file = dxf_from_document(&document);
    let scales: Vec<Option<f64>> = file.groups.iter().filter(|g| g.kind() == "LINE").map(|g| g.float(48)).collect();
    assert_eq!(scales, vec![None, Some(0.5)], "code 48 is written only for a scale other than 1");
    assert_eq!(file.header("$LTSCALE").map(str::trim), Some("2.0"));
    assert_eq!(file.header("$PSLTSCALE").map(str::trim), Some("0"));
    assert_eq!(file.header("$CELTSCALE").map(str::trim), Some("3.0"));

    let back = round_trip(&document);
    assert_eq!((back.ltscale, back.psltscale, back.celtscale), (2.0, false, 3.0));
    let scales: Vec<f64> = back.entities().iter().map(|e| e.linetype_scale).collect();
    assert_eq!(scales, vec![1.0, 0.5]);
    assert!(back.skipped.is_empty(), "{:?}", back.skipped);

    // A new drawing starts at AutoCAD's values: LTSCALE 1, PSLTSCALE on and CELTSCALE 1.
    let fresh = round_trip(&Document::with_entities(vec![line(pt(0.0, 0.0), pt(1.0, 1.0))]));
    assert_eq!((fresh.ltscale, fresh.psltscale, fresh.celtscale), (1.0, true, 1.0));
}

#[test]
fn a_multiline_survives_a_round_trip_in_its_style() {
    use crate::model::mline::{Justification, MlineGap, MlineStyle};
    // With a doorway cut in the outer line of its second segment.
    let wall = Entity::new(Shape::Mline {
        vertices: vec![pt(0.0, 0.0), pt(4000.0, 0.0), pt(4000.0, 3000.0)],
        closed: false,
        justification: Justification::Bottom,
        scale: 200.0,
        style: MlineStyle::standard(),
        gaps: vec![MlineGap { segment: 1, element: 1, from: 500.0, to: 1400.0 }],
    });
    let document = Document::with_entities(vec![wall.clone()]);
    let file = dxf_from_document(&document);
    let written = file.groups.iter().find(|g| g.kind() == "MLINE").expect("an MLINE");
    let style = file.groups.iter().find(|g| g.kind() == "MLINESTYLE").unwrap();
    assert_eq!(written.get(340).map(str::trim), style.handle().map(str::trim), "it points at the file's Standard style");
    let back = round_trip(&document);
    assert_eq!(back.entities()[0].shape, wall.shape);
    assert_eq!(back.mline_styles[0].elements.len(), 2);
    assert!(back.skipped.is_empty(), "{:?}", back.skipped);
}

#[test]
fn a_multiline_style_reads_its_fill_and_round_ends() {
    use crate::model::mline::MlineStyle;
    // Flags 1 (fill) | 64 (round start) | 1024 (round end), the fill color 3 before the first element.
    let pairs = [
        (0, "MLINESTYLE"),
        (2, "Rounded"),
        (70, "1089"),
        (62, "3"),
        (49, "0.5"),
        (62, "256"),
        (6, "BYLAYER"),
        (49, "-0.5"),
        (62, "256"),
        (6, "BYLAYER"),
    ]
    .into_iter()
    .map(|(code, value)| pair(code, value))
    .collect();
    let style: MlineStyle = read_mline_style(&Group(pairs)).expect("a style");
    assert_eq!(style.name, "Rounded");
    assert_eq!(style.fill, Some(3));
    assert!(style.start_round && style.end_round);
    assert!(!style.start_cap && !style.end_cap && !style.start_inner_arcs && !style.end_inner_arcs && !style.joints);
    assert_eq!(style.elements.len(), 2);
}

#[test]
fn new_and_changed_multiline_styles_are_saved_and_removed_ones_go() {
    use crate::model::mline::{Justification, MlineElement, MlineStyle};
    let element = |offset: f64, color: i32| MlineElement { offset, color, linetype: "ByLayer".into() };
    let wall = MlineStyle {
        name: "WALL".into(),
        description: "Cavity wall".into(),
        elements: vec![element(1.5, 1), element(0.5, 256), element(-1.5, 5)],
        fill: Some(8),
        start_cap: true,
        end_round: true,
        ..MlineStyle::standard()
    };
    let mut document = Document::with_entities(vec![Entity::new(Shape::Mline {
        vertices: vec![pt(0.0, 0.0), pt(10.0, 0.0)],
        closed: false,
        justification: Justification::Top,
        scale: 1.0,
        style: wall.clone(),
        gaps: vec![],
    })]);
    document.mline_styles = vec![MlineStyle::standard(), wall.clone()];
    let back = round_trip(&document);
    assert_eq!(back.mline_styles.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), vec!["Standard", "WALL"]);
    assert_eq!(back.mline_styles[1], wall);
    let Shape::Mline { style, .. } = &back.entities()[0].shape else { panic!() };
    assert_eq!(style, &wall, "the multiline points at its own style");

    // Saving the reopened drawing without the style and its multiline takes the style out of the file.
    let mut changed = back;
    changed.mline_styles.retain(|s| s.name == "Standard");
    changed.set_entities(vec![]);
    let again = round_trip(&changed);
    assert_eq!(again.mline_styles.len(), 1);
}

/// The lines of a drawing of two lines and a circle, as its DXF file.
fn saved_lines() -> Vec<String> {
    let document =
        Document::with_entities(vec![line(pt(0.0, 0.0), pt(10.0, 0.0)), line(pt(0.0, 5.0), pt(10.0, 5.0)), circle(pt(3.0, 3.0), 1.0)]);
    String::from_utf8(dxf_from_document(&document).to_bytes()).unwrap().lines().map(str::to_string).collect()
}

/// The index of the value line of a group code, in the first object of this kind.
fn value_of(lines: &[String], kind: &str, code: &str) -> usize {
    let start = lines.iter().position(|l| l.trim() == kind).unwrap();
    (start..lines.len()).find(|&i| lines[i].trim() == code).unwrap() + 1
}

fn recovered(lines: &[String]) -> (Document, Vec<String>) {
    let (file, notes) = DxfFile::parse_recovering(lines.join("\n").as_bytes()).unwrap();
    (document_from_dxf(file), notes)
}

#[test]
fn a_line_missing_a_value_takes_out_only_its_own_object() {
    let mut lines = saved_lines();
    let x = value_of(&lines, "LINE", "10");
    lines.remove(x);
    assert!(DxfFile::parse(lines.join("\n").as_bytes()).is_err(), "the normal reader refuses it");
    let (document, notes) = recovered(&lines);
    assert_eq!(document.len(), 2, "the other line and the circle are kept");
    assert!(notes.iter().any(|note| note.contains("should hold a group code")), "{notes:?}");
}

#[test]
fn a_group_code_that_is_not_a_code_takes_out_only_its_own_object() {
    let mut lines = saved_lines();
    let code = value_of(&lines, "LINE", "10") - 1;
    lines[code] = "1O".into();
    let (document, notes) = recovered(&lines);
    assert_eq!(document.len(), 2);
    assert!(notes.iter().any(|note| note.contains("1O")), "{notes:?}");
}

#[test]
fn a_number_that_is_not_one_takes_out_its_object() {
    let mut lines = saved_lines();
    let radius = value_of(&lines, "CIRCLE", "40");
    lines[radius] = "big".into();
    let (document, notes) = recovered(&lines);
    assert_eq!(document.len(), 2, "the circle goes, the lines stay");
    assert!(notes.iter().any(|note| note.contains("needs a number")), "{notes:?}");
}

#[test]
fn a_file_without_its_end_reads_as_it_did() {
    let mut lines = saved_lines();
    lines.truncate(lines.len() - 2);
    assert!(DxfFile::parse(lines.join("\n").as_bytes()).is_ok());
    let (document, notes) = recovered(&lines);
    assert_eq!(document.len(), 3);
    assert!(notes.is_empty(), "{notes:?}");
}

#[test]
fn a_file_cut_off_in_the_middle_of_a_value_keeps_what_came_before() {
    let mut lines = saved_lines();
    lines.pop();
    assert!(DxfFile::parse(lines.join("\n").as_bytes()).is_err());
    let (document, notes) = recovered(&lines);
    assert_eq!(document.len(), 3);
    assert!(notes.iter().any(|note| note.contains("ends in the middle")), "{notes:?}");
}

#[test]
fn a_file_cut_off_inside_an_object_keeps_the_objects_before_it() {
    let mut lines = saved_lines();
    let circle = lines.iter().position(|l| l.trim() == "CIRCLE").unwrap();
    lines.truncate(circle + 3);
    let (document, _) = recovered(&lines);
    assert_eq!(document.len(), 2, "the circle, cut short, is not drawn");
}
