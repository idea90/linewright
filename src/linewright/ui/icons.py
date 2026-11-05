from PySide6.QtCore import QByteArray, QRectF, Qt
from PySide6.QtGui import QIcon, QPainter, QPixmap
from PySide6.QtSvg import QSvgRenderer

from linewright import theme

ICON_SIZES = (16, 20, 24, 32)  # the sizes the ribbon, toolbars and navigation bar ask for
RENDER_SCALE = 2  # drawn at twice the size so it stays sharp on high-DPI screens
HOVER_COLOR = "#ffffff"

# One 24x24 line-icon family: 1.6 stroke, round caps and joins. "currentColor" is replaced when drawn.
_SHAPES = {
    "LINE": '<path d="M5 19 19 5"/><circle cx="5" cy="19" r="1.6" fill="currentColor"/>'
    '<circle cx="19" cy="5" r="1.6" fill="currentColor"/>',
    "PLINE": '<path d="M4 17 9 8l5 6 6-8"/><circle cx="4" cy="17" r="1.5" fill="currentColor"/>'
    '<circle cx="9" cy="8" r="1.5" fill="currentColor"/><circle cx="14" cy="14" r="1.5" fill="currentColor"/>'
    '<circle cx="20" cy="6" r="1.5" fill="currentColor"/>',
    "RECTANGLE": '<rect x="4.5" y="7" width="15" height="10" rx="1"/>',
    "CIRCLE": '<circle cx="12" cy="12" r="7.5"/><circle cx="12" cy="12" r="1" fill="currentColor"/>',
    "ARC": '<path d="M4.5 18a8.5 8.5 0 0 1 15 0"/><circle cx="4.5" cy="18" r="1.5" fill="currentColor"/>'
    '<circle cx="19.5" cy="18" r="1.5" fill="currentColor"/>',
    "TEXT": '<path d="M6 7V5.5h12V7M12 5.5v13M9 18.5h6"/>',
    "ERASE": '<path d="M8 19 4.5 15.5a1.5 1.5 0 0 1 0-2.1L13 5a1.5 1.5 0 0 1 2.1 0l4 4a1.5 1.5 0 0 1 0 2.1L11 19Z"/>'
    '<path d="M8 19h11M8.5 9.5l6 6"/>',
    "MOVE": '<path d="M12 4v16M4 12h16M9 7l3-3 3 3M9 17l3 3 3-3M7 9l-3 3 3 3M17 9l3 3-3 3"/>',
    "COPY": '<rect x="9" y="9" width="10.5" height="10.5" rx="1.5"/>'
    '<path d="M5.5 15V6.5A1.5 1.5 0 0 1 7 5h8.5"/>',
    "ROTATE": '<path d="M19 12a7 7 0 1 1-2.2-5.1"/><path d="M19.5 4v4.5H15"/>',
    "SCALE": '<rect x="4.5" y="11.5" width="8" height="8" rx="1"/><path d="M12.5 11.5 19.5 4.5M14 4.5h5.5V10"/>',
    "MIRROR": '<path d="M12 3.5v17" stroke-dasharray="2 2.5"/><path d="M9 8 4 18h5ZM15 8l5 10h-5Z"/>',
    "OFFSET": '<path d="M5 19a14 14 0 0 1 14-14"/><path d="M5 13.5a8.5 8.5 0 0 1 8.5-8.5"/>'
    '<path d="M5 8.5A3.5 3.5 0 0 1 8.5 5"/>',
    "TRIM": '<path d="M12 4v16"/><path d="M4 12h8"/><path d="M12 12h8" stroke-dasharray="2 2.5"/>',
    "EXTEND": '<path d="M19 5v14"/><path d="M4 12h9"/><path d="M13 12h6" stroke-dasharray="2 2.5"/>',
    "FILLET": '<path d="M5 4v7.5A6.5 6.5 0 0 0 11.5 18H20"/>',
    "DIMLINEAR": '<path d="M5 5v11M19 5v11M5 10.5h14"/><path d="m8.5 8.5-3.5 2 3.5 2M15.5 8.5l3.5 2-3.5 2"/>',
    "DIMALIGNED": '<g transform="rotate(-30 12 12)"><path d="M5 5v11M19 5v11M5 10.5h14"/>'
    '<path d="m8.5 8.5-3.5 2 3.5 2M15.5 8.5l3.5 2-3.5 2"/></g>',
    "DIMSCALE": '<path d="M4 17h16M4 17v-4M8 17v-2M12 17v-4M16 17v-2M20 17v-4"/>'
    '<path d="M7 8h10M9.5 5.5 7 8l2.5 2.5M14.5 5.5 17 8l-2.5 2.5"/>',
    "DISTANCE": '<g transform="rotate(-35 12 12)"><rect x="2.5" y="8.5" width="19" height="7" rx="1"/>'
    '<path d="M6 8.5v3M9.5 8.5v2M13 8.5v3M16.5 8.5v2"/></g>',
    "AREA": '<path d="M5 17.5 7.5 6l11.5 3.5-2 8.5Z"/><path d="m9 14 5-5M12 16.5l4.5-4.5"/>',
    "PLOT": '<path d="M7 9V4h10v5"/><rect x="4" y="9" width="16" height="8" rx="1.5"/><path d="M7 14h10v6H7Z"/>',
    "LAYERS": '<path d="m12 4 8.5 4.5L12 13 3.5 8.5Z"/><path d="m3.5 12.5 8.5 4.5 8.5-4.5"/>'
    '<path d="m3.5 16.5 8.5 4.5 8.5-4.5"/>',
    "MENU": '<path d="M4 7h16M4 12h16M4 17h16"/>',
    "NEW": '<path d="M6.5 3.5h7.5l4.5 4.5v11.5a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1v-15a1 1 0 0 1 1-1Z"/>'
    '<path d="M14 3.5V8h4.5M12 11.5v6M9 14.5h6"/>',
    "OPEN": '<path d="M3.5 7v11a1 1 0 0 0 1 1h15a1 1 0 0 0 1-1V9.5a1 1 0 0 0-1-1h-7.5l-2-2.5h-5.5a1 1 0 0 0-1 1Z"/>',
    "SAVE": '<path d="M5.5 4h10.5l3.5 3.5V19a1 1 0 0 1-1 1h-13a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1Z"/>'
    '<path d="M8 4v4.5h7V4M8 20v-6h8v6"/>',
    "UNDO": '<path d="M9 6 4.5 10.5 9 15M4.5 10.5H15a4.5 4.5 0 0 1 0 9H10"/>',
    "REDO": '<path d="m15 6 4.5 4.5L15 15M19.5 10.5H9a4.5 4.5 0 0 0 0 9h5"/>',
    "ZOOM_EXTENTS": '<path d="M4 9V4h5M15 4h5v5M20 15v5h-5M9 20H4v-5"/>'
    '<rect x="8.5" y="8.5" width="7" height="7" rx="1"/>',
    "WIN_MINIMIZE": '<path d="M6 12h12"/>',
    "WIN_MAXIMIZE": '<rect x="6" y="6" width="12" height="12" rx="1"/>',
    "WIN_RESTORE": '<rect x="5.5" y="9.5" width="9" height="9" rx="1"/>'
    '<path d="M9.5 9.5v-3a1 1 0 0 1 1-1h8a1 1 0 0 1 1 1v8a1 1 0 0 1-1 1h-3"/>',
    "WIN_CLOSE": '<path d="m6 6 12 12M18 6 6 18"/>',
    "PLUS": '<path d="M12 5v14M5 12h14"/>',
    "GRID": '<rect x="4" y="4" width="16" height="16" rx="1.5"/><path d="M4 9.3h16M4 14.7h16M9.3 4v16M14.7 4v16"/>',
    "SNAP": '<path d="m12 4 8 8-8 8-8-8Z"/><circle cx="12" cy="12" r="1.3" fill="currentColor"/>',
    "ORTHO": '<rect x="5" y="5" width="14" height="14" rx="1.5"/>',
    "POLAR": '<circle cx="12" cy="12" r="8"/><path d="m8.5 15.5 7-7"/>',
    "OSNAP": '<circle cx="12" cy="12" r="6.5"/><path d="M12 2.5v5M12 16.5v5M2.5 12h5M16.5 12h5"/>',
    "ZOOM_IN": '<circle cx="10.5" cy="10.5" r="6"/><path d="m15 15 5 5M10.5 8v5M8 10.5h5"/>',
    "ZOOM_OUT": '<circle cx="10.5" cy="10.5" r="6"/><path d="m15 15 5 5M8 10.5h5"/>',
}


def _pixmap(name: str, color: str, size: int) -> QPixmap:
    svg = (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="{color}" '
        f'stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">'
        f'{_SHAPES[name].replace("currentColor", color)}</svg>'
    )
    pixels = size * RENDER_SCALE
    pixmap = QPixmap(pixels, pixels)
    pixmap.fill(Qt.GlobalColor.transparent)
    painter = QPainter(pixmap)
    QSvgRenderer(QByteArray(svg.encode())).render(painter, QRectF(0, 0, pixels, pixels))
    painter.end()
    pixmap.setDevicePixelRatio(RENDER_SCALE)
    return pixmap


def command_icon(name: str) -> QIcon:
    """Light at rest, white on hover, accent colored while the command runs. Sharp at every toolbar size."""
    icon = QIcon()
    for size in ICON_SIZES:
        icon.addPixmap(_pixmap(name, theme.TEXT, size), QIcon.Mode.Normal, QIcon.State.Off)
        icon.addPixmap(_pixmap(name, HOVER_COLOR, size), QIcon.Mode.Active, QIcon.State.Off)
        icon.addPixmap(_pixmap(name, theme.ACCENT, size), QIcon.Mode.Normal, QIcon.State.On)
        icon.addPixmap(_pixmap(name, theme.ACCENT, size), QIcon.Mode.Active, QIcon.State.On)
    return icon




