#!/usr/bin/env sh
# Install Linewright on Linux: the program, its icon, and a menu entry that opens DXF and DWG drawings.
#
#   ./install.sh              for you alone, in ~/.local (no sudo)
#   sudo ./install.sh --system  for everyone, in /usr/local
#   ./install.sh --uninstall  remove it again (with --system for a system-wide install)
#
# The program is the linewright file beside this script, as in the release's linewright-linux-x86_64.tar.gz.

set -eu

here="$(cd "$(dirname "$0")" && pwd)"
action=install
prefix="$HOME/.local"
for arg in "$@"; do
    case "$arg" in
        --system) prefix=/usr/local ;;
        --uninstall) action=uninstall ;;
        --prefix=*) prefix="${arg#--prefix=}" ;;
        -h|--help) sed -n '2,8p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "Unknown option: $arg (try --help)" >&2; exit 2 ;;
    esac
done

bin="$prefix/bin/linewright"
icon="$prefix/share/icons/hicolor/256x256/apps/linewright.png"
desktop="$prefix/share/applications/linewright.desktop"

# Menus and file managers read these caches; they are brought up to date where the tools are installed.
refresh() {
    command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database -q "$prefix/share/applications" 2>/dev/null || true
    command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t "$prefix/share/icons/hicolor" 2>/dev/null || true
}

if [ "$action" = uninstall ]; then
    rm -f "$bin" "$icon" "$desktop"
    refresh
    echo "Linewright is removed from $prefix."
    exit 0
fi

program="$here/linewright"
[ -f "$program" ] || program="$here/linewright-linux-x86_64"
if [ ! -f "$program" ]; then
    echo "The linewright program is not beside this script. Unpack the whole linewright-linux-x86_64.tar.gz and run it from there." >&2
    exit 1
fi

mkdir -p "$(dirname "$bin")" "$(dirname "$icon")" "$(dirname "$desktop")"
install -m 755 "$program" "$bin"
install -m 644 "$here/linewright.png" "$icon"
cat > "$desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Linewright
GenericName=2D CAD
Comment=Draw, edit, dimension and plot DXF and DWG drawings
Exec="$bin" %f
Icon=linewright
Terminal=false
Categories=Graphics;Engineering;2DGraphics;
MimeType=image/vnd.dxf;image/vnd.dwg;image/x-dwg;application/dxf;application/acad;
StartupWMClass=linewright
DESKTOP
chmod 644 "$desktop"
refresh

echo "Linewright is installed in $prefix. Find it in your applications menu, or run: $bin"
case ":$PATH:" in
    *":$prefix/bin:"*) ;;
    *) echo "Note: $prefix/bin is not on your PATH, so typing 'linewright' in a terminal will not find it yet." ;;
esac
