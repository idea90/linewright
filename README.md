# Linewright

A 2D CAD program in the style of AutoCAD: draw, edit, dimension and plot DXF and DWG drawings. It runs natively on
Linux (X11 and Wayland) and Windows.

Download the latest build from [Releases](https://github.com/idea90/linewright/releases):

- **Windows:** run `linewright-setup-windows-x86_64.exe`. It installs for you alone without asking for an
  administrator (or for everyone, if you choose), adds Linewright to the Start menu, can open DXF and DWG files
  with it, and is removed from Settings → Apps. `linewright-windows-x86_64.exe` is the bare program, to run without
  installing.
- **Linux:** unpack `linewright-linux-x86_64.tar.gz` and run `./install.sh` in it. That puts Linewright in
  `~/.local` with a menu entry and icon that opens DXF and DWG files; `sudo ./install.sh --system` installs it in
  `/usr/local` for everyone, and `./install.sh --uninstall` removes it. `linewright-linux-x86_64` is the bare program.

Or build it yourself:

```sh
./compile                            # Linux and Windows programs, in dist/
cd rust && cargo run --release       # or run it straight away
```

The app lives in [`rust/`](rust/); its [README](rust/README.md) lists every command and feature. It was first
written in Python with Qt; that version is in the git history up to 1.0 Alpha 3.

What comes next is in the [roadmap](ROADMAP.md).

Licensed under the [MIT License](LICENSE). The built-in typefaces, Public Sans and Martian Mono, are under the SIL Open Font License; their
licenses are in [`rust/assets/fonts`](rust/assets/fonts).
