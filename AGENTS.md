# AGENTS.md

Guidance for AI coding agents (and people) working in this repository.

## What the project is

**Linewright** is a 2D CAD program in the style of AutoCAD: draw, edit, dimension and plot DXF and DWG drawings.
It aims to feel like AutoCAD to someone who already uses it, so the command names, aliases, prompts, options and
keyboard habits match AutoCAD's (Space and right-click act as Enter, Enter repeats the last command,
`@dx,dy` and `@dist<angle` point entry, and the F3/F7–F12 drafting aids). When in doubt about how something should
behave, do what AutoCAD does.

- The app is the **Rust** crate in `rust/`. It's a native egui/eframe program for Linux (X11 and Wayland) and Windows.
- It was first written in Python with Qt (PySide6 + ezdxf). That version was removed after 1.0 Alpha 3 and lives
  only in the git history (tag `v1.0.0-alpha.3` and earlier).
- License: MIT. Releases are published on GitHub (`idea90/linewright`) as pre-releases with Linux and Windows
  binaries; the current version is `1.0.0-alpha.6.3`.

`rust/README.md` lists every command and feature. Keep it up to date when adding user-facing behaviour.

## Build, run and test

All Rust commands run from `rust/`:

```sh
cargo run --release                 # an empty drawing
cargo run --release -- plan.dxf     # open a drawing
cargo test                          # the app's tests
cargo fmt --all                     # format (max_width 140, see rustfmt.toml)
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace              # the setup's tests too

# the setup, with the program built into it
cargo build --release && LINEWRIGHT_PAYLOAD=target/release/linewright cargo build --release -p linewright-setup
```

`rust/` is a Cargo workspace of the app and `setup/`; the version is `[workspace.package]` in `rust/Cargo.toml`.

CI (`.github/workflows/rust.yml`) runs `cargo fmt --check`, clippy with warnings as errors, the workspace's tests, a
release build and the setup on Ubuntu and Windows. **Run fmt, clippy and the tests before calling a change done.**

`./compile` (at the root) builds release binaries into `dist/` for Linux and Windows. From Linux it cross-builds
Windows with MinGW if installed, otherwise with Zig and cargo-zigbuild installed into `~/.cache/linewright-build`.
`dist/` is git-ignored; its binaries go on GitHub releases.

Notes:
- A debug build keeps dependencies at `opt-level = 2` without debug info so large drawings still draw smoothly
  and `target/` stays small. `target/` can still grow to many GB; `cargo clean` is safe.
- `LINEWRIGHT_CAPTURE=out.png` (debug builds) saves a screenshot of the window, useful for checking the UI.
- `.mcp.json` registers `linewright --mcp` with Claude Code: its `command` and `render` tools let you draw in the real
  app and look at the result, a quick way to check a change to a command or to drawing. It runs no window. It starts
  with `cargo run --release`, so the first start compiles for a few minutes; `.claude/settings.json` raises
  `MCP_TIMEOUT` to 10 minutes so the connection waits for it. After changing the code, reconnect it with `/mcp`.
- DWG support needs the free ODA File Converter (on `PATH`, its default install folder, or `LINEWRIGHT_ODA`).
- `LINEWRIGHT_NO_GPU=1` keeps the canvas's pictures off the graphics card (everything is then drawn through egui), for
  a driver that draws them wrong, and for comparing the two ways of drawing.
- `tests/speed.rs` times what a person waits on in a large drawing (opening, a frame, a mouse move, selecting,
  editing, undo, saving). Run it before and after a change that may cost or save time:
  `LINEWRIGHT_SPEED=big.dxf cargo test --release --test speed -- --ignored --nocapture`.

## Releasing

Push a tag named `v*` (for example `v1.0.0-alpha.4`) and `.github/workflows/release.yml` builds the Linux and
Windows binaries and the setup for each (the `setup/` crate with the program built into it), writes `SHA256SUMS` and
uploads them to the GitHub release for that tag, creating the release if it does not exist. CI builds the setups and
installs and removes Linewright with them (`--quiet`) on both systems on every push, so a broken setup shows before a
release. A tag containing `alpha`, `beta` or `rc` makes a pre-release. `./compile` stays for local builds.

## Repository layout

```
AGENTS.md            this file
README.md            short intro, links to rust/README.md
LICENSE              MIT
compile              build script for Linux/Windows binaries into dist/
.github/workflows/   CI for the Rust crate, and the release build on a `v*` tag
rust/                the app (Cargo crate "linewright")
  README.md          full feature and command list
  src/               see below
  setup/             the setup (crate "linewright-setup", a workspace member): its own egui window that installs,
                     updates and removes Linewright; the program is built into it (setup/build.rs)
  build/             windows_icon.rs, the Windows icon resource both build scripts include
```

### `rust/src/`

| Path | What it holds |
|---|---|
| `main.rs`, `lib.rs` | Entry point (opens a file given on the command line, or `--mcp` serves MCP) and the module list |
| `mcp.rs` | The MCP server for AI agents: tools that type at the command line through `ui::controller::Controller`, render PNGs and open and save |
| `fonts.rs` | Fonts for text in any script: the drawing's fonts, system fallbacks, legacy code pages |
| `threads.rs` | Sharing work over many items among the processors (`map_runs`): the canvas's picture, reading a file |
| `shx.rs` | AutoCAD SHX stroke fonts (romans, simplex, txt…), read from AutoCAD's font folders |
| **`model/`** | **The drawing itself: geometry, entities and the document. No UI code.** |
| `model/entities.rs` | `Entity` (id, layer, color, linetype, lineweight, shape) and the `Shape` enum: every kind of object |
| `model/document.rs` | `Document`: layers, layouts, blocks, styles, groups, units, and the entities of each space |
| `model/geometry.rs` | `Point`, `Bounds` and basic geometry helpers (angles, bulges, arcs through points, `arc_points`) |
| `model/parts.rs` | What an entity is made of (paths, filled areas, masks, points, texts) for drawing and plotting; `flatten` handles blocks |
| `model/drawn.rs` | The types `parts` produces and painters read: `Parts`, `TextSpec`, `TextLook` (no other model logic, so any module can use them) |
| `model/index.rs` | Spatial index for finding entities near a spot; `entity_extent` |
| `model/editing.rs` | Geometry behind editing: explode, stretch, grips, join, closed loops |
| `model/transform.rs` | Move/rotate/scale/mirror of shapes (`Similarity`) |
| `model/trimming.rs`, `fillet.rs`, `offset.rs` | TRIM/EXTEND/BREAK, FILLET/CHAMFER, OFFSET geometry |
| `model/primitives.rs`, `intersect.rs` | Each entity as lines, arcs and circles (`Primitive`), and where they cross: used by snaps, TRIM, OFFSET and FILLET |
| `model/boundary.rs` | Finding the closed area around a clicked point (HATCH and BOUNDARY) |
| `model/overkill.rs` | OVERKILL's duplicate and overlap clean-up |
| `model/mline.rs` | Multiline styles and the geometry of multilines |
| `model/dimension.rs`, `annotation.rs`, `multileader.rs` | Dimensions, leaders and multileaders: their styles and what they draw |
| `model/mtext.rs`, `table.rs` | MTEXT formatting codes and layout; tables |
| `model/properties.rs` | Rows of the Properties palette, and setting a property by typing a value |
| `model/spline.rs`, `xline.rs`, `viewport.rs`, `lineweight.rs`, `styles.rs`, `find.rs`, `measure.rs`, `calc.rs` | Smaller topics, named for what they hold |
| **`commands/`** | **Every command, each an `async fn`** |
| `commands/prompts.rs` | How a command talks to the user: `Io::ask(Prompt)` returns an `Answer`; `Io::step(Step)` changes the drawing. `Session` drives a command; `Host` is the drawing side |
| `commands/registry.rs` | `COMMANDS` (name, label), `ALIASES`, `create()` that builds a command by name, `WINDOW_COMMANDS` handled by the window, AutoComplete |
| `commands/defaults.rs` | `Defaults`: what commands remember between uses, plus drawing data the window keeps up to date |
| `commands/draw.rs`, `curves.rs`, `shapes.rs`, `drafting.rs` | Drawing commands (LINE, PLINE, CIRCLE, ARC, HATCH, BOUNDARY, POLYGON, MTEXT, WIPEOUT, MLINE…) |
| `commands/edit.rs`, `modify.rs`, `reshape.rs` | Editing commands (MOVE, COPY, TRIM, FILLET, ARRAY, JOIN, OVERKILL, LENGTHEN, PEDIT…) |
| `commands/dimension.rs`, `qdim.rs` | Dimension and leader commands |
| `commands/blocks.rs`, `groups.rs`, `layer_tools.rs`, `layout.rs` | Blocks and clipboard, groups, LAY* tools, layouts and viewports |
| `commands/settings.rs` | Commands that change the program's own settings (SAVETIME); `ui/settings.rs` keeps them in the settings file |
| `commands/inquiry.rs`, `measuring.rs` | LIST, ID, ZOOM, text edit, grip dragging; DISTANCE and AREA |
| `commands/parse.rs` | Parsing typed numbers, lengths and points |
| `commands/tests.rs` | Commands run start to finish against a `FakeHost` |
| **`io/`** | **Files** |
| `io/dxf.rs` | DXF read and write: turning groups into a drawing and back (R12 to R2018). Writing starts from the opened file (or `template.dxf`), so whatever Linewright cannot show is kept |
| `io/dxf/file.rs` | `DxfFile`: the file as code/value pairs and groups, its text encoding and header. Knows nothing of the drawing, so `Document` can keep the opened file |
| `io/dxf/tests.rs` | DXF round-trip tests |
| `io/files.rs` | Opening and saving drawings, DWG through the ODA File Converter |
| `io/ctb.rs`, `pat.rs` | Plot style tables (.ctb) and hatch pattern files (.pat) |
| `io/template.dxf` | The drawing a new document is saved from |
| **`canvas/`** | **Showing and printing** |
| `canvas/render.rs` | Drawing the canvas with egui: entities, selection, rubber band, cursor. The drawing's picture is kept between frames (`picture`) and made anew only when the drawing, its layers or the view change |
| `canvas/gpu.rs` | A large picture's triangles kept on the graphics card and drawn from there by an egui paint callback, moved and scaled while the view is panned or zoomed |
| `canvas/plot.rs`, `pdf.rs` | Plotting at a scale onto a `Page`: a PDF page, or `raster.rs`'s PNG picture |
| `canvas/view.rs`, `aci.rs`, `shapes.rs` | Camera, AutoCAD color index palette, colors parts draw in |
| **`snap/`** | Object snaps (`osnap.rs`), tracking and drafting aids, built on `model/primitives.rs` and `model/intersect.rs` |
| **`selection/`** | The selection and what an entity looks like to the mouse |
| **`ui/`** | **The window** |
| `ui/app.rs` | The main window, its state, each frame, and the actions buttons and keys trigger |
| `ui/controller.rs` | Connects typed input and clicks to the running command, and its steps to the drawing |
| `ui/panels.rs` | The header, palettes, dialogs, command line and status bar |
| `ui/ribbon.rs` | The ribbon: its tabs, panels, flyouts, the layer drop-down and the Properties panel's drop-downs |
| `ui/undo.rs` | Undo and redo: every change is a `Change` pushed here |
| `ui/layers.rs`, `icons.rs`, `theme.rs`, `frame.rs`, `recent.rs`, `debug_capture.rs` | Layer edits, icons, colors, the custom window frame, recent files, screenshot aid |

## How to make common changes

### Add a command

1. Write it as `pub async fn name_command(io: Io, ...)` in the fitting `commands/` file. Ask with
   `io.ask(Prompt::point("Specify first point:"))` and friends; keyword options are passed as their typed
   abbreviations (`.options(&["C", "U"])`) and shown in the message as AutoCAD does: `[Close/Undo]`.
2. Change the drawing only through `io.step(...)`: `Step::Drawn` for brand-new objects (they take the current
   layer and color), `Step::Commit` for objects made from existing ones, `Step::Remove`, `Step::Modify`,
   `Step::Report` for command-line messages, and `Step::Compound` for several changes that undo as one.
3. Register it in `commands/registry.rs`: add `(NAME, label)` to `COMMANDS`, its AutoCAD aliases to `ALIASES`,
   and a `create()` arm. Settings it should remember go in `commands/defaults.rs`.
4. Add a test in `commands/tests.rs` that types the answers (`typed(&mut session, &mut host, &[...])`) and checks
   the resulting shapes and reports.
5. List it in `rust/README.md`.

### Add a kind of object

Add a variant to `Shape` in `model/entities.rs`, then let the compiler's non-exhaustive match errors guide you.
Places that usually need it, beyond those errors: `Shape::kind`, `index::entity_extent`, `parts::entity_parts`
(and `flatten_into` if parts have their own colors), `transform::transform_shape`, `editing.rs` (stretch, grip
points, grip moves, explode, `closed_loop`), `model/primitives.rs`, `properties.rs` (rows and setters),
`commands/inquiry.rs` (LIST), and `io/dxf.rs` (reading in the entity match, writing in `Writer::entity`, with an
R12 fallback, plus any CLASS or OBJECTS entries AutoCAD needs). Add a DXF round-trip test in `io/dxf/tests.rs`.

## Conventions

- Match the surrounding code: doc comments on most items, written as plain sentences saying what something is
  or does in the user's terms ("The corners after the first, until Enter or Close"). Keep comment density similar.
- Names follow AutoCAD's vocabulary (entity, block, insert, layer, linetype, ByLayer, model space, layout).
- User-facing messages copy AutoCAD's wording where AutoCAD has a message for the same thing.
- Keep geometry pure and testable in `model/`; commands in `commands/` only ask and step.
- Large drawings must stay quick. Nothing that runs on every frame, mouse move or small edit may go through every
  object of the drawing: look objects up by id or by area (`Document::get`, `ids_in`), and keep what is costly to
  work out until `Document::revision` changes. Maps and sets keyed by object id use `IdMap` and `IdSet`.
- DXF: Never drop data the program doesn't understand. Unknown entities and objects must survive a save.
- Lines are at most 140 characters (`rustfmt.toml`); run `cargo fmt`.
- Don't commit `dist/` or `target/`. Commit messages are short and descriptive.
