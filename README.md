# Linewright

A 2D CAD program in the style of AutoCAD: draw, edit, dimension and plot DXF and DWG drawings. It runs natively on
Linux (X11 and Wayland) and Windows.

Download the latest build from [Releases](https://github.com/idea90/linewright/releases), or build it yourself:

```sh
./compile                            # Linux and Windows programs, in dist/
cd rust && cargo run --release       # or run it straight away
```

The app lives in [`rust/`](rust/); its [README](rust/README.md) lists every command and feature. It was first
written in Python with Qt; that version is in the git history up to 1.0 Alpha 3.

What comes next is in the [roadmap](ROADMAP.md).

Licensed under the [MIT License](LICENSE). The built-in typefaces, Public Sans and Martian Mono, are under the SIL Open Font License; their
licenses are in [`rust/assets/fonts`](rust/assets/fonts).
