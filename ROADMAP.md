# Linewright roadmap

Where Linewright is heading, in the order the work is planned. The goal stays the same throughout: a 2D CAD program
that someone who knows AutoCAD can use for daily drafting without relearning anything, on Linux and Windows.

There are no dates. Each stage is done when everything in it works, has tests, and is listed in
`rust/README.md`. Items can move between stages as priorities change.

## Where it stands: 1.0 Alpha 3

Released on GitHub as pre-releases with Linux and Windows builds. It covers most everyday 2D drafting:

- More than a hundred AutoCAD commands and aliases for drawing, modifying, blocks, groups, hatches, dimensions,
  multileaders, MTEXT, tables, multilines and wipeouts.
- AutoCAD habits: Space and right-click as Enter, Enter repeats, grips, object snaps and tracking, dynamic input,
  the Properties palette, right-click menus.
- Layers, lineweights, layouts with viewports, and PDF plotting with .ctb plot styles and SHX fonts.
- DXF open and save (R12 to R2018) that keeps whatever it can't show; DWG through the ODA File Converter.

## Alpha 4: safety and finishing what's started

Things an early user is most likely to miss or be hurt by.

- [ ] **Autosave and recovery.** Save a copy every few minutes (`SAVETIME`), and offer to restore it after a crash.
      There is no autosave today, so a crash loses everything since the last save.
- [ ] **`AUDIT` and `RECOVER`** for damaged DXF files: fix what can be fixed and report the rest.
- [ ] **Linetype scale:** `LTSCALE`, `PSLTSCALE` and per-object linetype scale, which drawings from AutoCAD rely on.
- [x] **`OOPS`** to bring back the last erased objects.
- [ ] **MLEDIT's other tools:** cross and tee joints, Cut Single, Cut All and Weld All. Needs gaps inside a
      multiline's lines, saved in its DXF element data.
- [x] **Ribbon overflow:** less common buttons go into a drop-down when the window is too narrow, so nothing
      ends up past the edge (OVERKILL currently does at normal widths).
- [x] **Automatic release builds:** GitHub Actions builds both programs and attaches them to the release when a
      version tag is pushed, so releases no longer depend on one machine.
- [ ] **Window tests:** a way to drive dialogs in tests (the screenshot aid, `LINEWRIGHT_CAPTURE`, already types
      commands), so dialogs such as Delete Duplicate Objects and Multiline Style are covered.
- [ ] **The Python version:** decide whether to archive it or remove it. Its tests no longer run (its environment
      is missing packages), and it confuses what the project is.

## Beta: everyday AutoCAD parity

The commands people reach for daily that are still missing.

**Editing and drafting**
- [ ] `UCS`, user coordinate systems, so drawing at an angle works as in AutoCAD.
- [ ] `ARRAYPATH`, and associative arrays that can be edited after they're made.
- [ ] `COPYBASE`, `PASTEBLOCK`, `PASTEORIG`, `COPYTOLAYER`, `NCOPY`.
- [ ] `REGION` and the Boolean commands `UNION`, `SUBTRACT` and `INTERSECT` for 2D regions; BOUNDARY's Region
      object type.
- [ ] `SKETCH`, and isometric drafting (`ISODRAFT`, isometric snap and grid).
- [ ] `CAL` and a QuickCalc palette, usable inside point and distance prompts.

**Annotation**
- [ ] Annotative scaling for text, dimensions, multileaders and hatches, with `CANNOSCALE` and the scale list.
- [ ] `DIMSPACE` and `DIMBREAK`.
- [ ] `TXT2MTXT`, `TEXTMASK`, `FIELD`.

**Blocks and attributes**
- [ ] `BATTMAN`, `ATTSYNC` and `EATTEDIT` for managing and editing attributes.
- [ ] `BURST`, and the block editor's attribute tools.
- [ ] `XCLIP` for clipping block references and external references.

**Layers and views**
- [ ] `LAYMRG`, `LAYDEL`, `LAYWALK`, and layer states (`LAYERSTATE`).
- [ ] Named views (`VIEW`), restored in viewports too.

## 1.0: ready for real projects

What a firm would need before using it on paid work.

- [ ] **Files:** Asian big fonts in SHX text; named plot style tables (.stb); `PUBLISH` many sheets to one PDF;
      `ETRANSMIT` to package a drawing with its fonts, images and external references; drawing properties
      (`DWGPROPS`).
- [ ] **Settings:** an `OPTIONS` dialog (colors, file locations, autosave, selection and snap settings) and
      `ALIASEDIT` for custom command aliases, both remembered between sessions.
- [ ] **Plotting:** print directly to system printers as well as to PDF; page setups shared between layouts.
- [ ] **Install and update:** a Windows installer and a Linux package (AppImage or Flatpak), file associations
      for .dxf and .dwg, signed builds, and a check for new versions.
- [ ] **Performance:** large drawings (hundreds of thousands of objects) open, pan, zoom and select smoothly; a
      benchmark in CI keeps them that way.
- [ ] **Documentation:** a user guide on the website, covering where Linewright differs from AutoCAD.

## Later

Larger pieces with no stage yet.

- Dynamic blocks (parameters and actions), read from and saved to DXF.
- Geometric and dimensional constraints.
- `PDFIMPORT` and attaching PDFs as underlays.
- Tool palettes and a DesignCenter-style block browser.
- Sheet sets.
- Scripts (`SCRIPT` with .scr files) and a way to add commands without rebuilding the program.
- A macOS build.

## How to help

`AGENTS.md` explains how the code is laid out and how to add a command or a new kind of object. Pick an item,
open an issue to say you're on it, and send a pull request with tests.
