# Linewright

A 2D CAD program in the style of AutoCAD, rewritten in Rust from the original Python/Qt version. It runs natively on
Linux (X11 and Wayland) and Windows.

## Build and run

```sh
cargo run --release                  # an empty drawing
cargo run --release -- plan.dxf      # open a drawing
cargo test
```

On Linux the window needs the usual desktop libraries (`libxkbcommon`, `libwayland-client` or `libX11`, and an
OpenGL driver). File dialogs use the XDG desktop portal.

## What it does

- A start page when it opens without a file: new, open, recent drawings and first steps.
- A ribbon laid out as AutoCAD's: Home, Insert, Annotate, View, Manage and Output tabs (double-click the open tab to
  hide the panels). Home has AutoCAD's Modify, Layers, Annotation, Block, Properties, Draw, Groups, Utilities and
  Clipboard panels. Buttons with a ▾ are flyouts: the button runs the tool used last, the ▾ lists the others (Circle
  2-Point and 3-Point, Arc Center, Start, End, the dimension types and so on). The Layers panel's drop-down turns
  layers on and off, freezes and locks them from their icons. Each panel's name strip opens its other tools, and the
  columns that do not fit a narrow window move there too.

- A command line as AutoCAD's: a prompt's options in square brackets are buttons that answer with the option when
  clicked, their capital letters picked out. The status bar's drafting aids are icons, lit while on, with their
  name and key in the tip; a right-click on Object snap opens its settings.
- Palettes: Properties (Ctrl+1) in sections that fold away, and Layer Properties with each layer's on, freeze, lock
  and plot states as icons to click.

- Draw: `LINE` (`L`), `PLINE` (`PL`), `RECTANGLE` (`REC`), `POLYGON` (`POL`), `MTEXT` (`MT`), `XLINE` (`XL`), `RAY`, `DONUT` (`DO`), `REVCLOUD`, `TABLE` (`TB`), `CIRCLE` (`C`), `ARC` (`A`), `ELLIPSE` (`EL`),
  `SPLINE` (`SPL`), `POINT` (`PO`), `HATCH` (`H`; SOLID, ANSI31, ANSI37, NET, LINE, DOTS, DASH), `TEXT` (`T`)
- `MLINE` (`ML`): multilines with Justification (Top/Zero/Bottom), Scale and STyle, in the drawing's own multiline
  styles (element offsets, colors, linetypes, line, outer arc and inner arc caps, joints and fill); they load from
  and save to DXF and explode to lines. `MLSTYLE` manages the styles (new, delete, set current, elements, caps,
  fill), and `MLEDIT` (`-MLEDIT`) makes closed, open and merged crosses and tees, corner joints,
  adds and deletes vertices, and cuts (one line or all) and welds
- `WIPEOUT`: masking areas from clicked corners or a closed polyline, hiding what is drawn under them; framed on
  screen and printed with their frames; its Frames option sets WIPEOUTFRAME for the drawing (ON shows and prints
  the frames, OFF hides them, Display but not plot shows them on screen only); they load from and save to DXF
- `BOUNDARY` (`BO`): a closed polyline around each area clicked inside, and around its islands; curved edges
  become arc segments
- `OOPS`: brings back the objects the last `ERASE` or `BLOCK` took out, as one undo step, and forgets them. Delete
  does the same erase as `ERASE`.
- Modify: `ERASE`, `MOVE`, `COPY`, `ROTATE`, `SCALE`, `MIRROR`, `OFFSET`, `TRIM`, `EXTEND`, `FILLET` (`F`;
  lines, arcs and circles, or two segments of a polyline that meet; parallel lines get a half circle; Polyline
  rounds every corner), `CHAMFER`
  (`CHA`), `LENGTHEN` (`LEN`), `ALIGN` (`AL`), `PEDIT` (`PE`), `DIVIDE` (`DIV`), `MEASURE` (`ME`), `DRAWORDER` (`DR`), `STRETCH` (`S`), `ARRAY` (`AR`; rectangular and polar), `BREAK` (`BR`), `JOIN` (`J`), `EXPLODE` (`X`),
  `MATCHPROP` (`MA`), `OVERKILL` (`OV`; opens Delete Duplicate Objects with Tolerance, Ignore properties, Optimize
  polylines, Partial overlap and End to end options; deletes duplicates, combines overlapping lines and arcs, and tidies
  polylines), `-OVERKILL` (the same with command line options)
- As in AutoCAD: `PLINE` draws arcs with Arc (carrying on from the segment before, or through a Second pt) and goes
  back to lines with Line; `ROTATE` and `SCALE` take Copy and Reference; `OFFSET` makes a spline beside an ellipse
  or a spline; `JOIN` joins lines in one straight line across the gaps between them; `AREA` and `LIST` measure whole
  ellipses.
- Blocks: `INSERT` (`I`, asks for attribute values), `BLOCK` (`B`); attributes edited with `TEXTEDIT` (`ED`)
- Groups: `GROUP` (`G`; named or unnamed `*A1`, with a description), `UNGROUP`, `GROUPEDIT` (add, remove, rename),
  `PICKSTYLE` or Ctrl+Shift+A for group selection; groups load from and save to DXF
- Colors: all 255 index colors and true colors (RGB), from the Select Color dialog (`COLOR`, More colors…)
- Annotate and measure: `DIMLINEAR`, `DIMALIGNED`, `DIMCONTINUE` (`DCO`), `DIMBASELINE` (`DBA`), `DIMRADIUS` (`DRA`), `DIMDIAMETER` (`DDI`), `DIMANGULAR` (`DAN`),
  `QLEADER`/`LEADER` (`LE`), `DIMSTYLE` (`D`; style manager), `-DIMSTYLE` (Save/Restore/STatus/Apply/?), `DIMSCALE`,
  `DIMTXT`, `DIMASZ`, `DISTANCE`, `AREA`, `MEASUREGEOM` (`MEA`; Distance, Radius, Angle between lines, of an arc or
  at a vertex, and ARea, repeated until eXit), `LIST` (`LI`), `ID`, `ZOOM` (`Z`)
- Calculator: `CAL` evaluates an expression (`+ - * / ^`, parentheses, `sqrt`, `abs`, `sin`/`cos`/`tan` in degrees,
  `pi`); `QUICKCALC` (`QC`) runs the same command-line calculator rather than a palette
- `HATCH` picks internal points (islands stay empty) or selected closed objects
- File commands typed as in AutoCAD: `QSAVE`, `SAVEAS`, `OPEN`, `NEW`, `PLOT`
- Autosave: `SAVETIME` sets the minutes between copies of a drawing with unsaved changes (default 10, 0 turns it
  off), kept in the `autosave` folder of the settings folder (`~/.config/linewright`, or `%APPDATA%\Linewright`).
  After a crash or kill, the Drawing Recovery dialog at start-up (or `DRAWINGRECOVERY`) opens each copy as an
  unsaved "(recovered)" drawing, or deletes it. A copy goes once its drawing is saved or discarded.
- AutoCAD habits: Space and right-click are Enter, Enter repeats the last command, grips on selected objects,
  double-click to edit text, Ctrl+C / Ctrl+X / Ctrl+V for objects (`COPYCLIP`, `CUTCLIP`, `PASTECLIP`), Ctrl+A
  (`SELECTALL`, `AI_SELALL`), Ctrl+1 for the Properties palette
  (several objects of one kind change together; differing values show *VARIES*).
- Object snaps (endpoint, midpoint, center, node, quadrant, intersection, insertion, perpendicular, nearest) chosen
  in Drafting Settings (`OSNAP`, `OS`), object snap tracking (F11) and dynamic input by the cursor (F12).
- Selecting and finding: `QSELECT`, `SELECTSIMILAR`, `FIND` (find and replace text); `PURGE` (`PU`), `UNITS` (`UN`).
- Checking damaged drawings: `AUDIT` lists what cannot be drawn (no radius, too few vertices, coordinates that are not
  numbers, text with no height, hatches without a loop, and so on) and what refers to a missing layer, linetype,
  block or group, then asks `Fix any errors detected? [Yes/No] <N>:`. Yes fixes it all as one undo step; blocks are
  only reported, to be fixed in the block editor. `RECOVER` opens a damaged DXF or DWG (DWG through the ODA
  converter), skipping the lines and objects it cannot read, fixes what `AUDIT` would, and reports what it skipped.
- Images (`IMAGEATTACH`, `IM`), external references (`XATTACH`, `XREF`) and the block editor (`BEDIT`, `BCLOSE`);
  tables (`TABLE`, `TABLEDIT` or double-click a cell) are AutoCAD tables, read from and saved to files; attribute
  definitions (`ATTDEF`) for blocks; images print in PDFs. Dynamic input takes a length and angle (Tab between
  them) or a distance along the cursor.
- Points as `x,y`, `@dx,dy` or `@distance<angle`; `U`/`UNDO`, `REDO`, `ALL`.
- Object snaps (endpoint, midpoint, center, intersection, perpendicular, node), grid snap, ortho and polar
  tracking: F3, F7–F10.
- Layers with color, linetype, on/off and lock in the Layers palette (`LAYER`, `LA`), and AutoCAD's layer tools
  `LAYISO`, `LAYUNISO`, `LAYOFF`, `LAYON`, `LAYFRZ`, `LAYTHW`, `LAYMCUR`, `LAYMCH`, `LAYLCK`, `LAYULK`; layers freeze and thaw, and can be kept off plots; per-object color and linetype; undo and redo of everything.
- DXF open and save (R12 to R2018): lines, arcs, circles, polylines, ellipses, splines, points, solids, hatches,
  text and MTEXT (aligned, multi-line), dimensions with their styles, and blocks with attributes (nested,
  mirrored, stretched). Whatever Linewright cannot show yet is kept when saving.
- Text in any script: the fonts a drawing names (legacy Lao fonts included), the system's fallback fonts,
  code pages of older files.
- Right-click menus as AutoCAD's: on the drawing (Repeat, Recent Input, Paste, Undo, Zoom, Quick Select, Find,
  Properties), on a selection (Cut, Copy, Erase, Move, Scale, Rotate, Draw Order, Group, Select Similar, and what
  the object offers), during a command with options (its options, Snap Overrides), and Shift+right-click for a
  one-time object snap. END, MID, CEN and the like typed at a point prompt do the same.
- Dimensions: `QDIM`, `DIMORDINATE`, `DIMARC`, `DIMJOGGED` and `DIMTEDIT`, tolerances (symmetrical, deviation, limits)
  in styles and Properties.
- Multileaders: `MLEADER`, `MLEADERADD`, `MLEADERREMOVE`, `MLEADERSTYLE`. Ones holding a block stay in the file as they are.
- Hatching: every pattern in AutoCAD's acadiso.pat (`?` lists them), `HATCHEDIT`, `GRADIENT` (linear, cylinder, inverse
  cylinder), and hatches that follow their boundary. Those made from closed objects keep following after saving;
  those picked inside areas follow until the drawing closes.
- Polyline widths (`PLINE` Width and Halfwidth, `PEDIT` Width, Properties), `SPLINEDIT`.
- Text: `STYLE` for text styles (font, fixed height, width factor, oblique) and an MTEXT editor with bold, italic,
  underline, font, height, color and stacking. MTEXT's formatting shows, prints and is kept when saving.
- SHX fonts (romans.shx, simplex.shx, txt.shx, isocp.shx, gothice.shx…) drawn from the font's own strokes, on
  screen and in PDFs, sized as AutoCAD sizes them. They are read from the drawing's folder, AutoCAD's `Fonts`
  folders (`C:\Program Files\Autodesk\*\Fonts`, also seen from WSL under `/mnt/c`) and `LINEWRIGHT_FONT_DIRS`.
  A missing SHX font is replaced by simplex.shx, as AutoCAD does. Asian big fonts are not read yet.
- DWG open and save through the free ODA File Converter, found on the `PATH`, in its default install folder,
  or through `LINEWRIGHT_ODA=/path/to/ODAFileConverter`.
- PDF plotting at a scale (`1:50`) or fit to the page; on a layout, `PLOT` prints its sheet at 1:1.
- Lineweights as in AutoCAD: on objects (ByLayer, ByBlock, Default or 0.00–2.11 mm) and layers, set from the
  ribbon, the Properties palette's General section or the Layer Properties manager. `LWEIGHT` (`LW`) sets the
  current and default lineweight; the status bar's Lineweight button (`LWDISPLAY`) shows them on screen, at a
  fixed width in model space and to scale on layouts. They print at their real width.
- Linetype scale as in AutoCAD: dashes are drawn in drawing units times `LTSCALE` (`LTS`) and the object's own
  linetype scale (Properties palette, General section; `CELTSCALE` for new objects). `PSLTSCALE` (0 or 1) keeps
  model space dashes in viewports as long on the sheet as paper space ones. A dash too short to see draws solid.
  Plots follow the same rule in paper units.
- Plot style tables (.ctb): `PLOT` picks one per page setup, and with it whether objects' lineweights print.
  Pens give each color its plotted color, grayscale, screening and lineweight. Tables are read from AutoCAD's
  `Plotters\Plot Styles` folders (or `LINEWRIGHT_PLOT_STYLES`); acad, monochrome and grayscale.ctb work
  without AutoCAD. Named plot styles (.stb) are not read yet.
- Layouts: Model and layout tabs under the drawing (right-click to rename or delete, + to add), `LAYOUT` (`LO`;
  New/Rename/Delete/Set/?), `MVIEW` (`MV`) for viewports onto model space at a scale (set it in Properties), each
  space with its own undo. Deleting or renaming a layout undoes like any other change: `U` brings a deleted layout
  back in its place, with what was on it and its own undo history. Layouts, paper space and viewports load from and save to DXF.
- Working through viewports: `MSPACE` (`MS`), double-clicking inside a viewport, or the status bar's Paper/Model
  button opens model space through it with the layout still on screen; draw and edit there, and zooming and
  panning change that viewport's view. Click another viewport to work through it; `PSPACE` (`PS`) or
  double-clicking the sheet goes back. `VPLAYER` freezes or thaws layers in the current, chosen or all viewports,
  as does Layer Properties' VP Freeze column; `MVIEW` Lock (or Display locked in Properties) keeps a viewport's
  scale from changing.

## Use it from Claude Code (MCP)

`linewright --mcp` runs no window: it is a [Model Context Protocol](https://modelcontextprotocol.io) server on
standard input and output, so an AI agent such as Claude Code can draw, edit, look at and save drawings. The
repository's `.mcp.json` starts it with `cargo run --release -- --mcp`, so Claude Code offers it when opened in this
folder. To use it elsewhere, point an MCP client at the binary:

```sh
claude mcp add linewright -- /path/to/linewright --mcp
```

Its tools:

- `command`: type at the command line, one entry per line: `["LINE", "0,0", "100,0", "@0,50", ""]`. `""` is
  Enter. Entries can also be `click x,y` (a mouse click: a point, or picking an object), `box x1,y1 x2,y2` (a
  window or crossing box), `select <id>…` and `esc`. `U` and `REDO` undo and redo. The reply has the command line's
  messages, the ids of objects added, changed and erased, and what the command waits for next. A waiting command
  keeps running into the next call.
- `render`: a PNG of the drawing as it plots (dark lines on white), of model space, the open layout's sheet or a
  window, with the area shown and its pixels per unit.
- `list_entities`: objects with their id, kind, layer, color and main geometry, filtered by layer, kind or area.
- `drawing_info`: units, extent, layers, layouts, blocks, counts of each kind, and the prompt waiting.
- `layer`: make or change a layer (color, linetype, on, frozen, locked, plot) and make it current.
- `set_property`: set Layer, Color, Linetype or any property the Properties palette edits, on objects by id.
- `open_drawing`, `new_drawing`, `save_drawing`, `list_commands`.

Commands that open a dialog or file box in the window (QSAVE, PLOT, MTEXT, STYLE…) are refused, pointing to the tool
that does the same.

## Layout

| Module | Role |
|---|---|
| `model` | Geometry and entities, blocks, the document and its spatial index, editing geometry, properties |
| `commands` | Each command as an `async fn` that awaits the user's answers; the session that drives it |
| `snap`, `selection` | Object snaps and tracking; picking and window/crossing selection |
| `io` | DXF reading and writing at the group-code level, DWG through the ODA converter |
| `canvas` | Camera, rendering with egui, PDF plotting, PNG pictures |
| `ui` | The window: controller, undo stack, layers, panels |
| `mcp` | The MCP server (`--mcp`) that drives the controller for an AI agent |
