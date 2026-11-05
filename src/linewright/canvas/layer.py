import math
import time
from collections.abc import Callable, Iterable
from dataclasses import dataclass
from typing import NamedTuple

from PySide6.QtCore import QPointF, QRectF, Qt, QTimer
from PySide6.QtGui import QColor, QPainter, QPainterPath, QPen
from PySide6.QtWidgets import QGraphicsItem, QStyleOptionGraphicsItem, QWidget
from shiboken6 import isValid

from linewright.canvas.shapes import TextSpec, draw_text, entity_shapes, resolve_color
from linewright.canvas.view import WORLD_EXTENT
from linewright.model.dimension import dimension_parts
from linewright.model.document import Document
from linewright.model.entities import Arc, Circle, Dimension, Entity, Line, Polyline, Text
from linewright.model.geometry import Point, bulge_arc
from linewright.model.styles import DASH_PATTERNS

Bounds = tuple[float, float, float, float]  # x0, y0, x1, y1
Cell = tuple[int, int]

DEFAULT_CELL_SIZE = 1000.0
CELLS_ACROSS = 24  # how many cells the middle of a drawing is split into each way
OUTLIER_FRACTION = 0.01  # a few stray objects far from the rest must not stretch the cells
MIN_CELL_SIZE = 1e-6
MAX_CELLS_PER_ENTITY = 64  # a longer object than this is checked on every query instead of being filed in cells
DETAIL_LEVELS = 6
DETAIL_STEP = 4.0  # each detail level holds objects this many times smaller than the one before
BUILD_SECONDS = 0.03  # how long one repaint may spend preparing tiles before it lets the window breathe
TEXT_DESCENT = 0.3  # of the text height, below the baseline
WHITE = QColor(255, 255, 255)
BLACK = QColor(0, 0, 0)


class Detail(NamedTuple):
    """How small, in screen pixels, something may be before it is not drawn at all."""

    min_stroke: float = 2.0
    min_text: float = 3.0


NO_DETAIL_LIMIT = Detail(0.0, 0.0)


@dataclass(frozen=True)
class Look:
    """How a layer draws its entities. The defaults draw each entity in its own color and linetype."""

    color: QColor | None = None
    width: float = 1.0  # in screen pixels
    dashes: tuple[float, ...] | None = None  # None means each entity's linetype
    ink: bool = False  # white lines are for a dark screen, so on paper they print black


@dataclass(frozen=True)
class Style:
    rgba: int
    dashes: tuple[float, ...]
    width: float

    @property
    def color(self) -> QColor:
        return QColor.fromRgba(self.rgba)


_PENS: dict[Style, QPen] = {}


def _pen(style: Style) -> QPen:
    pen = _PENS.get(style)
    if pen is None:
        pen = QPen(style.color)
        pen.setCosmetic(True)
        pen.setWidthF(style.width)
        if style.dashes:
            pen.setDashPattern(list(style.dashes))
        _PENS[style] = pen
    return pen


@dataclass(eq=False)
class Record:
    entity: Entity
    order: int  # drawing order, lowest first
    bounds: Bounds
    visible: bool
    home: Cell  # the tile that draws it
    cells: list[Cell] | None  # the cells that find it again, None when it is too long to file


class _Tile:
    """The entities homed in one cell, with the batched paths they are drawn from once those are built."""

    def __init__(self) -> None:
        self.records: list[Record] = []
        self.bounds: Bounds | None = None
        self.dirty = True
        self.batches: list[tuple[int, Style, QPainterPath, QPainterPath | None]] = []
        self.texts: list[tuple[TextSpec, Style]] = []

    def grow(self, bounds: Bounds) -> None:
        self.bounds = bounds if self.bounds is None else _union(self.bounds, bounds)

    def shrink_to_fit(self) -> None:
        self.bounds = None
        for record in self.records:
            self.grow(record.bounds)


def _union(first: Bounds, second: Bounds) -> Bounds:
    return (min(first[0], second[0]), min(first[1], second[1]), max(first[2], second[2]), max(first[3], second[3]))


def _rect(bounds: Bounds) -> QRectF:
    return QRectF(bounds[0], bounds[1], bounds[2] - bounds[0], bounds[3] - bounds[1])


def _around(points: Iterable[Point]) -> Bounds:
    xs, ys = zip(*points)
    return (min(xs), min(ys), max(xs), max(ys))


def _text_extent(origin: Point, rotation: float, height: float, length: int) -> Bounds:
    """A generous box around text. The real width depends on the font, so each character gets a full height."""
    width = height * max(length, 1)
    cos, sin = math.cos(math.radians(rotation)), math.sin(math.radians(rotation))
    corners = [(0, -TEXT_DESCENT * height), (width, -TEXT_DESCENT * height), (width, height), (0, height)]
    return _around((origin[0] + u * cos - v * sin, origin[1] + u * sin + v * cos) for u, v in corners)


def _polyline_extent(polyline: Polyline) -> Bounds:
    vertices = polyline.vertices
    points = [(x, y) for x, y, _ in vertices]
    for index, (x, y, bulge) in enumerate(vertices):
        following = vertices[(index + 1) % len(vertices)]
        if bulge and (following[0], following[1]) != (x, y):
            arc = bulge_arc((x, y), (following[0], following[1]), bulge)
            radius = arc.radius
            center_x, center_y = arc.center
            points += [(center_x - radius, center_y - radius), (center_x + radius, center_y + radius)]
    return _around(points)


def _dimension_extent(dimension: Dimension) -> Bounds:
    parts = dimension_parts(dimension)
    points = [point for segment in (*parts.extension_lines, parts.dimension_line) for point in segment]
    points += [point for arrow in parts.arrows for point in arrow]
    reading = math.radians(parts.text_angle)
    half = len(parts.text) * parts.text_height / 2
    origin = (parts.text_anchor[0] - half * math.cos(reading), parts.text_anchor[1] - half * math.sin(reading))
    return _union(_around(points), _text_extent(origin, parts.text_angle, parts.text_height, len(parts.text)))


def entity_extent(entity: Entity) -> Bounds:
    """A box that holds the whole entity. It may be larger than needed, for arcs and text, never smaller."""
    if isinstance(entity, Line):
        return _around([entity.start, entity.end])
    if isinstance(entity, (Circle, Arc)):
        (x, y), radius = entity.center, entity.radius
        return (x - radius, y - radius, x + radius, y + radius)
    if isinstance(entity, Polyline):
        return _polyline_extent(entity)
    if isinstance(entity, Text):
        return _text_extent(entity.position, entity.rotation, entity.height, len(entity.text))
    return _dimension_extent(entity)


def _finite(bounds: Bounds) -> Bounds:
    """Damaged files can hold coordinates that are not numbers. They are treated as sitting at the origin."""
    return bounds if all(math.isfinite(value) for value in bounds) else (0.0, 0.0, 0.0, 0.0)


def _middle_span(values: list[float]) -> float:
    ordered = sorted(values)
    cut = int(len(ordered) * OUTLIER_FRACTION)
    trimmed = ordered[cut : len(ordered) - cut] or ordered
    return trimmed[-1] - trimmed[0]


def _noop(_area: QRectF | None) -> None:
    pass


class EntityLayer:
    """Draws a set of entities fast, and finds the ones near a spot fast, however many there are.

    Entities are filed in a grid. Each grid cell draws its own entities as a few big paths (one per color and
    detail level) instead of one graphics item each. Cells are built only when they come into view, a little at a
    time, and cells or detail too small to see are skipped. Entities further apart than a cell are drawn in no
    particular order relative to each other.
    """

    def __init__(
        self,
        document: Callable[[], Document],
        look: Look = Look(),
        honor_layers: bool = True,
        notify: Callable[[QRectF | None], None] = _noop,
    ) -> None:
        self._document = document
        self._look = look
        self._honor_layers = honor_layers
        self._notify = notify
        self.cell_size = DEFAULT_CELL_SIZE
        self._records: dict[int, Record] = {}
        self._cells: dict[Cell, list[Record]] = {}
        self._oversized: list[Record] = []
        self._tiles: dict[Cell, _Tile] = {}
        self._next_order = 0

    def __len__(self) -> int:
        return len(self._records)

    def __contains__(self, entity: Entity) -> bool:
        return id(entity) in self._records

    def reload(self, entities: Iterable[Entity], cell_size: float | None = None) -> None:
        before = self.extent()
        self._records.clear()
        self._cells.clear()
        self._oversized.clear()
        self._tiles.clear()
        self._next_order = 0
        measured = [(entity, _finite(entity_extent(entity))) for entity in entities]
        self.cell_size = cell_size or self._cell_size_for([bounds for _, bounds in measured])
        for entity, bounds in measured:
            self._file(self._record(entity, bounds))
        changed = before.united(self.extent())
        if not changed.isNull():
            self._notify(changed)

    def clear(self) -> None:
        self.reload([], self.cell_size)

    def retile(self, cell_size: float) -> None:
        """Use a different cell size, refiling everything."""
        records = sorted(self._records.values(), key=lambda record: record.order)
        self.reload([record.entity for record in records], cell_size)

    def add(self, entity: Entity) -> None:
        record = self._record(entity, _finite(entity_extent(entity)))
        self._file(record)
        self._notify(_rect(record.bounds))

    def remove(self, entity: Entity) -> None:
        record = self._records.pop(id(entity))
        self._unfile(record)
        self._notify(_rect(record.bounds))

    def refresh(self, entity: Entity) -> None:
        """Pick up changes made to an entity in place. It keeps its place in the drawing order."""
        record = self._records[id(entity)]
        before = record.bounds
        self._unfile(record)
        record.bounds = _finite(entity_extent(entity))
        record.visible = self._is_visible(entity)
        record.home = self._home_of(record.bounds)
        self._file(record)
        self._notify(_rect(_union(before, record.bounds)))

    def refresh_all(self) -> None:
        """Look again at every entity's layer, after layers changed what they look like."""
        for record in self._records.values():
            record.visible = self._is_visible(record.entity)
        for tile in self._tiles.values():
            tile.dirty = True
        self._notify(None)

    def entities(self) -> list[Entity]:
        return [record.entity for record in sorted(self._records.values(), key=lambda record: record.order)]

    def style_of(self, entity: Entity) -> Style:
        document = self._document()
        color = self._look.color or resolve_color(entity, document)
        if self._look.ink and color == WHITE:
            color = BLACK
        dashes = self._look.dashes
        if dashes is None:
            dashes = DASH_PATTERNS.get(document.effective_linetype(entity), ())
        return Style(color.rgba(), tuple(dashes), self._look.width)

    def entities_in(self, area: QRectF) -> list[Entity]:
        """The visible entities whose box touches the area, in drawing order."""
        area = area.normalized()
        size = self.cell_size
        columns = range(math.floor(area.left() / size), math.floor(area.right() / size) + 1)
        rows = range(math.floor(area.top() / size), math.floor(area.bottom() / size) + 1)
        if len(columns) * len(rows) > len(self._cells):
            candidates = list(self._records.values())
        else:
            candidates = [r for column in columns for row in rows for r in self._cells.get((column, row), ())]
            candidates += self._oversized
        found = {id(r.entity): r for r in candidates if r.visible and self._touches(r.bounds, area)}
        return [r.entity for r in sorted(found.values(), key=lambda r: r.order)]

    def extent(self) -> QRectF:
        """The box around everything visible, or an empty rectangle for an empty drawing."""
        boxes = [record.bounds for record in self._records.values() if record.visible]
        if not boxes:
            return QRectF()
        return _rect(boxes[0] if len(boxes) == 1 else self._union_all(boxes))

    def paint(
        self,
        painter: QPainter,
        exposed: QRectF,
        pixels_per_unit: float,
        detail: Detail = Detail(),
        deadline: float | None = None,
    ) -> bool:
        """Draw what lies in the exposed area. Returns True when tiles were left unbuilt for lack of time."""
        left, top, right, bottom = exposed.left(), exposed.top(), exposed.right(), exposed.bottom()
        unfinished = False
        painter.setBrush(Qt.BrushStyle.NoBrush)
        for tile in self._tiles.values():
            bounds = tile.bounds
            if bounds is None or bounds[2] < left or bounds[0] > right or bounds[3] < top or bounds[1] > bottom:
                continue
            if tile.dirty:
                if deadline is not None and time.perf_counter() > deadline:
                    unfinished = True
                    continue
                self._build(tile)
            self._draw(painter, tile, exposed, pixels_per_unit, detail)
        return unfinished

    @staticmethod
    def _union_all(boxes: list[Bounds]) -> Bounds:
        return (min(b[0] for b in boxes), min(b[1] for b in boxes), max(b[2] for b in boxes), max(b[3] for b in boxes))

    @staticmethod
    def _touches(bounds: Bounds, area: QRectF) -> bool:
        left, top, right, bottom = area.left(), area.top(), area.right(), area.bottom()
        return not (bounds[2] < left or bounds[0] > right or bounds[3] < top or bounds[1] > bottom)

    def _cell_size_for(self, boxes: list[Bounds]) -> float:
        if not boxes:
            return DEFAULT_CELL_SIZE
        span = max(
            _middle_span([(box[0] + box[2]) / 2 for box in boxes]),
            _middle_span([(box[1] + box[3]) / 2 for box in boxes]),
        )
        return max(span / CELLS_ACROSS, MIN_CELL_SIZE) if span > 0 else DEFAULT_CELL_SIZE

    def _is_visible(self, entity: Entity) -> bool:
        return not self._honor_layers or self._document().is_visible(entity)

    def _home_of(self, bounds: Bounds) -> Cell:
        size = self.cell_size
        return (math.floor((bounds[0] + bounds[2]) / 2 / size), math.floor((bounds[1] + bounds[3]) / 2 / size))

    def _record(self, entity: Entity, bounds: Bounds) -> Record:
        record = Record(entity, self._next_order, bounds, self._is_visible(entity), self._home_of(bounds), None)
        self._next_order += 1
        self._records[id(entity)] = record
        return record

    def _cells_for(self, bounds: Bounds) -> list[Cell] | None:
        size = self.cell_size
        first_column, last_column = math.floor(bounds[0] / size), math.floor(bounds[2] / size)
        first_row, last_row = math.floor(bounds[1] / size), math.floor(bounds[3] / size)
        if (last_column - first_column + 1) * (last_row - first_row + 1) > MAX_CELLS_PER_ENTITY:
            return None
        return [(c, r) for c in range(first_column, last_column + 1) for r in range(first_row, last_row + 1)]

    def _file(self, record: Record) -> None:
        record.cells = self._cells_for(record.bounds)
        if record.cells is None:
            self._oversized.append(record)
        for cell in record.cells or ():
            self._cells.setdefault(cell, []).append(record)
        tile = self._tiles.setdefault(record.home, _Tile())
        tile.records.append(record)
        tile.grow(record.bounds)
        tile.dirty = True

    def _unfile(self, record: Record) -> None:
        if record.cells is None:
            self._oversized.remove(record)
        for cell in record.cells or ():
            members = self._cells[cell]
            members.remove(record)
            if not members:
                del self._cells[cell]
        tile = self._tiles[record.home]
        tile.records.remove(record)
        tile.dirty = True
        if tile.records:
            tile.shrink_to_fit()
        else:
            del self._tiles[record.home]

    def _level(self, bounds: Bounds) -> int:
        diagonal = math.hypot(bounds[2] - bounds[0], bounds[3] - bounds[1])
        if diagonal >= self.cell_size:
            return 0
        if diagonal <= 0:
            return DETAIL_LEVELS
        return min(DETAIL_LEVELS, int(math.log(self.cell_size / diagonal, DETAIL_STEP)))

    def _build(self, tile: _Tile) -> None:
        groups: dict[tuple[int, Style], tuple[QPainterPath, QPainterPath]] = {}
        texts: list[tuple[TextSpec, Style]] = []
        for record in tile.records:
            if not record.visible:
                continue
            shapes = entity_shapes(record.entity)
            style = self.style_of(record.entity)
            if shapes.text is not None:
                texts.append((shapes.text, style))
            if shapes.strokes is None:
                continue
            strokes, fills = groups.setdefault((self._level(record.bounds), style), (QPainterPath(), QPainterPath()))
            strokes.addPath(shapes.strokes)
            if shapes.fills is not None:
                strokes.addPath(shapes.fills)
                fills.addPath(shapes.fills)
        tile.batches = [
            (level, style, strokes, None if fills.isEmpty() else fills)
            for (level, style), (strokes, fills) in groups.items()
        ]
        tile.texts = sorted(texts, key=lambda item: -item[0].height)
        tile.dirty = False

    def _draw(self, painter: QPainter, tile: _Tile, exposed: QRectF, pixels_per_unit: float, detail: Detail) -> None:
        for level, style, strokes, fills in tile.batches:
            if level and self.cell_size / DETAIL_STEP**level * pixels_per_unit < detail.min_stroke:
                continue
            painter.setPen(_pen(style))
            painter.drawPath(strokes)
            if fills is not None:
                painter.fillPath(fills, style.color)
        for spec, style in tile.texts:
            if spec.height * pixels_per_unit < detail.min_text:
                break
            reach = spec.height * len(spec.text)
            if not exposed.adjusted(-reach, -reach, reach, reach).contains(QPointF(*spec.origin)):
                continue
            painter.setPen(_pen(style))
            draw_text(painter, spec)


class LayerItem(QGraphicsItem):
    """Puts an EntityLayer in a graphics scene. It is one item however many entities the layer holds."""

    def __init__(
        self, document: Callable[[], Document], look: Look, honor_layers: bool, z_value: float
    ) -> None:
        super().__init__()
        self.layer = EntityLayer(document, look, honor_layers, self.request_update)
        self.setZValue(z_value)
        self.setAcceptedMouseButtons(Qt.MouseButton.NoButton)
        self.setFlag(QGraphicsItem.GraphicsItemFlag.ItemUsesExtendedStyleOption)

    def boundingRect(self) -> QRectF:
        return QRectF(-WORLD_EXTENT, -WORLD_EXTENT, 2 * WORLD_EXTENT, 2 * WORLD_EXTENT)

    def request_update(self, area: QRectF | None) -> None:
        if area is None:
            self.update()
        else:
            self.update(area)

    def _continue_building(self) -> None:
        if isValid(self):  # the window may have closed while tiles were still being prepared
            self.update()

    def paint(self, painter: QPainter, option: QStyleOptionGraphicsItem, widget: QWidget | None = None) -> None:
        pixels_per_unit = option.levelOfDetailFromTransform(painter.worldTransform())
        deadline = time.perf_counter() + BUILD_SECONDS
        if self.layer.paint(painter, option.exposedRect, pixels_per_unit, deadline=deadline):
            QTimer.singleShot(0, self._continue_building)
