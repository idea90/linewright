//! How long the things a person waits on take in a large drawing: opening, each frame, each mouse move, selecting,
//! editing, undo and saving. Not a pass or fail test: it prints times, to compare before and after a change.
//!
//! ```sh
//! LINEWRIGHT_SPEED=big.dxf cargo test --release --test speed -- --ignored --nocapture
//! ```

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

use eframe::egui::{self, Rect, pos2, vec2};
use linewright::canvas::render::{self, Overlay, Screen};
use linewright::canvas::view::Camera;
use linewright::io::dxf::{DxfFile, document_from_dxf, dxf_from_document};
use linewright::io::files::open_drawing;
use linewright::model::geometry::pt;
use linewright::ui::controller::Controller;

const WIDTH: f32 = 1600.0;
const HEIGHT: f32 = 1000.0;

/// Run it `times` times and print the mean, in milliseconds.
fn timed<T>(what: &str, times: usize, mut run: impl FnMut() -> T) -> T {
    let start = Instant::now();
    let mut last = run();
    for _ in 1..times {
        last = run();
    }
    println!("{:>9.2} ms  {what}", start.elapsed().as_secs_f64() * 1000.0 / times as f64);
    last
}

/// One frame of the drawing area: the shapes the canvas makes, and egui turning them into triangles.
fn frame(ctx: &egui::Context, controller: &Controller, camera: Camera, pointer: Option<egui::Pos2>) -> usize {
    let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(WIDTH, HEIGHT));
    let fonts = HashSet::new();
    let input = egui::RawInput { screen_rect: Some(rect), ..Default::default() };
    let started = Instant::now();
    let output = ctx.run(input, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let screen = Screen::new(rect, camera, &fonts, controller.document());
            let overlay = Overlay { show_grid: true, pointer, panning: false, typed: String::new(), first_field: None };
            render::paint(ui, &screen, controller, &overlay);
        });
    });
    let made = started.elapsed();
    let shapes = output.shapes.len();
    let triangles = ctx.tessellate(output.shapes, output.pixels_per_point);
    if std::env::var_os("LINEWRIGHT_SPEED_PARTS").is_some() {
        let vertices: usize =
            triangles.iter().map(|p| if let egui::epaint::Primitive::Mesh(m) = &p.primitive { m.vertices.len() } else { 0 }).sum();
        println!(
            "             {shapes} shapes in {:.1} ms, {vertices} vertices in {:.1} ms more",
            made.as_secs_f64() * 1000.0,
            (started.elapsed() - made).as_secs_f64() * 1000.0
        );
    }
    triangles.len()
}

#[test]
#[ignore = "a measurement: give a large drawing in LINEWRIGHT_SPEED"]
fn speed_of_a_large_drawing() {
    let Some(path) = std::env::var_os("LINEWRIGHT_SPEED").map(PathBuf::from) else {
        println!("Set LINEWRIGHT_SPEED to a large DXF file.");
        return;
    };
    if path.extension().is_some_and(|suffix| suffix.eq_ignore_ascii_case("dxf")) {
        let bytes = timed("  read the file's bytes", 1, || std::fs::read(&path).expect("the file reads"));
        let mut file = Some(timed("  split them into groups", 1, || DxfFile::parse(&bytes).expect("the file is a DXF")));
        timed("  make the drawing from the groups", 1, || document_from_dxf(file.take().expect("parsed once")));
    }
    let mut opened = Some(timed("open the file", 1, || open_drawing(&path).expect("the drawing opens")));
    println!("{:>12}  objects", opened.as_ref().map_or(0, |document| document.len()));
    let mut controller = Controller::new();
    timed("make it the open drawing", 1, || controller.set_document(opened.take().expect("opened once")));
    let extent = controller.document().extent().expect("something is drawn");
    let mut all = Camera::default();
    all.zoom_extents(extent, WIDTH as f64, HEIGHT as f64);
    let near = Camera { center: extent.center(), scale: all.scale * 40.0 };
    let ctx = egui::Context::default();
    frame(&ctx, &controller, all, None); // fonts and atlases are made on the first frame

    for (view, camera) in [("everything", all), ("zoomed in 40x", near)] {
        controller.scale = camera.scale;
        timed(&format!("a frame showing {view}"), 5, || frame(&ctx, &controller, camera, Some(pos2(800.0, 500.0))));
        let mut step = 0;
        timed(&format!("a mouse move showing {view}"), 200, || {
            step += 1;
            let at = camera.to_world(f64::from(step % 40) * 17.0 - 300.0, f64::from(step % 23) * 13.0 - 150.0);
            controller.move_cursor(Some(at));
        });
        controller.handle_input("LINE");
        controller.handle_input(&format!("{},{}", extent.center().x, extent.center().y));
        let mut step = 0;
        timed(&format!("a mouse move drawing a line, showing {view}"), 200, || {
            step += 1;
            let at = camera.to_world(f64::from(step % 40) * 17.0 - 300.0, f64::from(step % 23) * 13.0 - 150.0);
            controller.move_cursor(Some(at));
        });
        controller.cancel();
    }

    controller.scale = all.scale;
    let (low, high) = (pt(extent.x0, extent.y0), pt(extent.x1, extent.y1));
    timed("click on an object", 20, || {
        controller.click(extent.center());
        controller.release(extent.center(), false);
    });
    timed("select everything with a box", 1, || {
        controller.click(low + pt(-1.0, -1.0));
        controller.move_cursor(Some(high + pt(1.0, 1.0)));
        controller.release(high + pt(1.0, 1.0), false);
    });
    if controller.selection.is_empty() {
        timed("select everything with Ctrl+A", 1, || controller.select_all());
    }
    println!("{:>12}  selected", controller.selection.len());
    timed("a frame with everything selected", 3, || frame(&ctx, &controller, all, None));
    timed("a mouse move with everything selected", 20, || controller.move_cursor(Some(extent.center())));
    timed("start MOVE with everything selected", 1, || controller.handle_input("MOVE"));
    timed("give MOVE its base point", 1, || controller.handle_input("0,0"));
    timed("a mouse move while moving everything", 20, || controller.move_cursor(Some(extent.center())));
    timed("a frame while moving everything", 3, || frame(&ctx, &controller, all, None));
    timed("finish the MOVE", 1, || controller.handle_input("10,10"));
    timed("undo it", 1, || controller.undo());
    timed("redo it", 1, || controller.redo());
    timed("draw one line", 20, || {
        for entry in ["LINE", "0,0", "100,100", ""] {
            controller.handle_input(entry);
        }
    });
    timed("erase one object", 1, || {
        controller.click(extent.center());
        controller.release(extent.center(), false);
        controller.handle_input("ERASE");
    });
    timed("ZOOM Extents", 5, || controller.document().extent());
    let file = timed("write the drawing as DXF", 1, || dxf_from_document(controller.document()));
    timed("turn it into the file's bytes", 1, || file.to_bytes().len());
}
