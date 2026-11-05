import math
from functools import lru_cache
from typing import NamedTuple

from ezdxf.colors import aci2rgb
from PySide6.QtCore import QPointF, QRectF
from PySide6.QtGui import QColor, QFont, QFontMetricsF, QPainter, QPainterPath, QPolygonF

from linewright.model.dimension import dimension_parts
from linewright.model.document import Document
from linewright.model.entities import BYLAYER, Arc, Circle, Dimension, Entity, Line, Polyline, Text
from linewright.model.geometry import Point, bulge_arc

FALLBACK_ACI = 7
TEXT_SCALE = 100  # text is drawn this many times too large to keep hinting stable, then scaled back to world units


class TextSpec(NamedTuple):
    origin: Point  # the start of the text's baseline
    rotation: float  # degrees, counter-clockwise
    height: float
    text: str


class Shapes(NamedTuple):
    """What an entity is made of: lines to stroke, filled areas (arrowheads) and text. Each part may be missing."""

    strokes: QPainterPath | None
    fills: QPainterPath | None
    text: TextSpec | None


def resolve_color(entity: Entity, document: Document) -> QColor:
    aci = entity.color
    if aci == BYLAYER or not 1 <= aci <= 255:
        layer = document.layers.get(entity.layer)
        aci = layer.color if layer and 1 <= layer.color <= 255 else FALLBACK_ACI
    return QColor(*aci2rgb(aci))


def _arc_to(path: QPainterPath, center: Point, radius: float, start: float, sweep: float) -> None:
    """Append an arc. Qt measures angles clockwise in scene space, so both angles are negated."""
    box = QRectF(center[0] - radius, center[1] - radius, 2 * radius, 2 * radius)
    path.arcTo(box, -math.degrees(start), -math.degrees(sweep))


def _line_path(entity: Line) -> QPainterPath:
    path = QPainterPath(QPointF(*entity.start))
    path.lineTo(QPointF(*entity.end))
    return path


def _circle_path(entity: Circle) -> QPainterPath:
    path = QPainterPath()
    path.addEllipse(QPointF(*entity.center), entity.radius, entity.radius)
    return path


def _arc_path(entity: Arc) -> QPainterPath:
    start = math.radians(entity.start_angle)
    sweep = math.radians((entity.end_angle - entity.start_angle) % 360.0)
    path = QPainterPath(
        QPointF(
            entity.center[0] + entity.radius * math.cos(start),
            entity.center[1] + entity.radius * math.sin(start),
        )
    )
    _arc_to(path, entity.center, entity.radius, start, sweep)
    return path


def _polyline_path(entity: Polyline) -> QPainterPath:
    vertices = entity.vertices
    path = QPainterPath(QPointF(vertices[0][0], vertices[0][1]))
    segment_count = len(vertices) if entity.closed else len(vertices) - 1
    for index in range(segment_count):
        x, y, bulge = vertices[index]
        next_x, next_y, _ = vertices[(index + 1) % len(vertices)]
        if bulge:
            arc = bulge_arc((x, y), (next_x, next_y), bulge)
            _arc_to(path, arc.center, arc.radius, arc.start_angle, arc.sweep)
        else:
            path.lineTo(QPointF(next_x, next_y))
    return path


_PATH_BUILDERS = {
    Line: _line_path,
    Circle: _circle_path,
    Arc: _arc_path,
    Polyline: _polyline_path,
}


@lru_cache(maxsize=256)
def text_font(pixel_size: int) -> QFont:
    font = QFont()
    font.setPixelSize(max(1, pixel_size))
    return font


def _font_for(height: float) -> QFont:
    return text_font(round(height * TEXT_SCALE))


def text_width(text: str, height: float) -> float:
    return QFontMetricsF(_font_for(height)).horizontalAdvance(text) / TEXT_SCALE


def draw_text(painter: QPainter, spec: TextSpec) -> None:
    painter.save()
    painter.translate(*spec.origin)
    painter.rotate(spec.rotation)
    painter.scale(1 / TEXT_SCALE, -1 / TEXT_SCALE)
    painter.setFont(_font_for(spec.height))
    painter.drawText(QPointF(0, 0), spec.text)
    painter.restore()


def _dimension_shapes(entity: Dimension) -> Shapes:
    parts = dimension_parts(entity)
    strokes = QPainterPath()
    for start, end in (*parts.extension_lines, parts.dimension_line):
        strokes.moveTo(QPointF(*start))
        strokes.lineTo(QPointF(*end))
    fills = QPainterPath()
    for tip, left, right in parts.arrows:
        fills.addPolygon(QPolygonF([QPointF(*tip), QPointF(*left), QPointF(*right)]))
        fills.closeSubpath()
    reading = math.radians(parts.text_angle)
    half_width = text_width(parts.text, parts.text_height) / 2
    origin = (
        parts.text_anchor[0] - half_width * math.cos(reading),
        parts.text_anchor[1] - half_width * math.sin(reading),
    )
    return Shapes(strokes, fills, TextSpec(origin, parts.text_angle, parts.text_height, parts.text))


def entity_shapes(entity: Entity) -> Shapes:
    if isinstance(entity, Text):
        return Shapes(None, None, TextSpec(entity.position, entity.rotation, entity.height, entity.text))
    if isinstance(entity, Dimension):
        return _dimension_shapes(entity)
    return Shapes(_PATH_BUILDERS[type(entity)](entity), None, None)
