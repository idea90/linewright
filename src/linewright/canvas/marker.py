from PySide6.QtCore import QPointF, QRectF, Qt
from PySide6.QtGui import QColor, QPainter, QPen, QPolygonF

from linewright import theme

MARKER_COLOR = QColor(theme.SNAP_MARKER)
HALF_SIZE = 6.0
WINDOW_COLOR = theme.ACCENT
CROSSING_COLOR = theme.CROSSING
BOX_FILL_ALPHA = 0.12
PICKBOX_HALF = 5.0
AXES_LENGTH = 42.0


def draw_marker(painter: QPainter, center: QPointF, kind: str) -> None:
    """Draw an object snap marker at a viewport position, in pixels."""
    pen = QPen(MARKER_COLOR)
    pen.setWidthF(2.0)
    painter.setPen(pen)
    painter.setBrush(Qt.BrushStyle.NoBrush)
    x, y, h = center.x(), center.y(), HALF_SIZE
    if kind == "endpoint":
        painter.drawRect(x - h, y - h, 2 * h, 2 * h)
    elif kind == "midpoint":
        painter.drawPolygon(QPolygonF([QPointF(x, y - h), QPointF(x - h, y + h), QPointF(x + h, y + h)]))
    elif kind == "center":
        painter.drawEllipse(center, h, h)
    elif kind == "intersection":
        painter.drawLine(QPointF(x - h, y - h), QPointF(x + h, y + h))
        painter.drawLine(QPointF(x - h, y + h), QPointF(x + h, y - h))
    else:
        painter.drawPolyline(QPolygonF([QPointF(x - h, y - h), QPointF(x - h, y + h), QPointF(x + h, y + h)]))


def draw_selection_box(painter: QPainter, box: QRectF, crossing: bool) -> None:
    """The drag rectangle in pixels: solid blue for a window, dashed green for a crossing."""
    color = QColor(CROSSING_COLOR if crossing else WINDOW_COLOR)
    pen = QPen(color)
    pen.setStyle(Qt.PenStyle.DashLine if crossing else Qt.PenStyle.SolidLine)
    painter.setPen(pen)
    fill = QColor(color)
    fill.setAlphaF(BOX_FILL_ALPHA)
    painter.setBrush(fill)
    painter.drawRect(box)



def draw_crosshair(painter: QPainter, center: QPointF, arm: float, pickbox: bool) -> None:
    """The drafting cursor: crossed lines, with a small square in the middle while objects can be picked."""
    pen = QPen(QColor(theme.CROSSHAIR))
    painter.setPen(pen)
    painter.setBrush(Qt.BrushStyle.NoBrush)
    x, y = center.x(), center.y()
    painter.drawLine(QPointF(x - arm, y), QPointF(x + arm, y))
    painter.drawLine(QPointF(x, y - arm), QPointF(x, y + arm))
    if pickbox:
        painter.drawRect(QRectF(x - PICKBOX_HALF, y - PICKBOX_HALF, 2 * PICKBOX_HALF, 2 * PICKBOX_HALF))


def draw_axes_icon(painter: QPainter, origin: QPointF) -> None:
    """The X and Y axes icon in a corner of the drawing area."""
    axes = (
        (theme.AXIS_X, QPointF(AXES_LENGTH, 0), QPointF(5, 5), "X"),
        (theme.AXIS_Y, QPointF(0, -AXES_LENGTH), QPointF(-4, -5), "Y"),
    )
    for color, vector, label_offset, label in axes:
        pen = QPen(QColor(color))
        pen.setWidthF(1.6)
        painter.setPen(pen)
        tip = origin + vector
        painter.drawLine(origin, tip)
        painter.drawText(tip + label_offset, label)
    painter.setPen(QPen(QColor(theme.TEXT_DIM)))
    painter.setBrush(Qt.BrushStyle.NoBrush)
    painter.drawRect(QRectF(origin.x() - 3, origin.y() - 3, 6, 6))
