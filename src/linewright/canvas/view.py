import math
from collections.abc import Callable

from PySide6.QtCore import QEvent, QPointF, QRectF, QSize, Qt, Signal
from PySide6.QtGui import QColor, QPainter, QPen, QTransform, QWheelEvent
from PySide6.QtWidgets import QGraphicsScene, QGraphicsView

from linewright import theme
from linewright.canvas.marker import draw_axes_icon, draw_crosshair, draw_marker, draw_selection_box

WORLD_EXTENT = 1_000_000.0
ZOOM_STEP = 1.2
MIN_SCALE = 1e-6
MAX_SCALE = 1e6
EXTENTS_MARGIN = 0.9
MIN_GRID_PIXELS = 12.0
MAJOR_GRID_EVERY = 10
CROSSHAIR_ARM_FRACTION = 0.04  # of the drawing area's width, each way from the cursor
AXES_ICON_MARGIN = (48.0, 44.0)  # from the left and bottom edges
BACKGROUND = QColor(theme.CANVAS)
MINOR_GRID = QColor(theme.GRID_MINOR)
MAJOR_GRID = QColor(theme.GRID_MAJOR)


def grid_spacing(scale: float) -> float:
    """Smallest power of ten whose on-screen size is at least MIN_GRID_PIXELS."""
    return 10.0 ** math.ceil(math.log10(MIN_GRID_PIXELS / scale))


class CanvasView(QGraphicsView):
    cursor_moved = Signal(QPointF)
    clicked = Signal(QPointF)  # left button pressed
    released = Signal(QPointF)  # left button released
    viewport_resized = Signal(QSize)  # the drawing area has its new size

    def __init__(self, scene: QGraphicsScene) -> None:
        super().__init__(scene)
        scene.setSceneRect(-WORLD_EXTENT, -WORLD_EXTENT, 2 * WORLD_EXTENT, 2 * WORLD_EXTENT)
        self.setMouseTracking(True)
        self.setFocusPolicy(Qt.FocusPolicy.NoFocus)  # typing always goes to the command line
        self.setTransformationAnchor(QGraphicsView.ViewportAnchor.AnchorUnderMouse)
        self.setResizeAnchor(QGraphicsView.ViewportAnchor.AnchorViewCenter)
        self.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
        self.setVerticalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
        self.setRenderHint(QPainter.RenderHint.Antialiasing)
        self.setBackgroundBrush(BACKGROUND)
        self.setFrameShape(QGraphicsView.Shape.NoFrame)
        self.viewport().setCursor(Qt.CursorShape.BlankCursor)  # the crosshair is drawn instead
        self.scale(1, -1)
        self.scale(4, 4)
        self._pan_origin: QPointF | None = None
        self._marker: tuple[QPointF, str] | None = None
        self._box: tuple[QPointF, QPointF] | None = None
        self._pointer: QPointF | None = None
        self._pickbox = True
        self._grid_visible = True
        self._extent_source: Callable[[], QRectF] = QRectF

    @property
    def zoom_scale(self) -> float:
        return self.transform().m11()

    def zoom_by(self, factor: float) -> None:
        target = min(max(self.zoom_scale * factor, MIN_SCALE), MAX_SCALE)
        factor = target / self.zoom_scale
        self.scale(factor, factor)

    def set_extent_source(self, source: Callable[[], QRectF]) -> None:
        """Say where to ask for the box around the whole drawing, which Zoom Extents frames."""
        self._extent_source = source

    def zoom_extents(self) -> None:
        bounds = self._extent_source()
        viewport = self.viewport().rect()
        if bounds.isNull() or viewport.isEmpty():
            return
        # Qt's fitInView would drop the Y flip, so build the transform directly.
        width = max(bounds.width(), 1e-9)
        height = max(bounds.height(), 1e-9)
        fit = min(viewport.width() / width, viewport.height() / height) * EXTENTS_MARGIN
        fit = min(max(fit, MIN_SCALE), MAX_SCALE)
        self.setTransform(QTransform.fromScale(fit, -fit))
        self.centerOn(bounds.center())

    def resizeEvent(self, event) -> None:
        super().resizeEvent(event)
        self.viewport_resized.emit(self.viewport().size())

    def wheelEvent(self, event: QWheelEvent) -> None:
        self.zoom_by(ZOOM_STEP if event.angleDelta().y() > 0 else 1 / ZOOM_STEP)

    def mousePressEvent(self, event) -> None:
        if event.button() == Qt.MouseButton.LeftButton:
            self.clicked.emit(self.mapToScene(event.position().toPoint()))
        if event.button() == Qt.MouseButton.MiddleButton:
            self._pan_origin = event.position()
            self.viewport().setCursor(Qt.CursorShape.ClosedHandCursor)
            return
        super().mousePressEvent(event)

    def mouseMoveEvent(self, event) -> None:
        if self._pan_origin is not None:
            delta = event.position() - self._pan_origin
            self._pan_origin = event.position()
            self.translate(delta.x() / self.zoom_scale, -delta.y() / self.zoom_scale)
            return
        self._move_pointer(event.position())
        self.cursor_moved.emit(self.mapToScene(event.position().toPoint()))
        super().mouseMoveEvent(event)

    def viewportEvent(self, event) -> bool:
        if event.type() == QEvent.Type.Leave:
            self._move_pointer(None)
        return super().viewportEvent(event)

    def _move_pointer(self, position: QPointF | None) -> None:
        previous, self._pointer = self._pointer, position
        for spot in (previous, position):
            if spot is not None:
                arm = self._crosshair_arm() + 2
                self.viewport().update(QRectF(spot.x() - arm, spot.y() - arm, 2 * arm, 2 * arm).toAlignedRect())

    def _crosshair_arm(self) -> float:
        return self.viewport().width() * CROSSHAIR_ARM_FRACTION

    def set_pickbox(self, visible: bool) -> None:
        """Show the small square at the crosshair centre, which tells the user objects can be picked."""
        self._pickbox = visible
        self.viewport().update()

    def set_grid_visible(self, visible: bool) -> None:
        self._grid_visible = visible
        self.viewport().update()

    def mouseReleaseEvent(self, event) -> None:
        if event.button() == Qt.MouseButton.LeftButton:
            self.released.emit(self.mapToScene(event.position().toPoint()))
        if event.button() == Qt.MouseButton.MiddleButton and self._pan_origin is not None:
            self._pan_origin = None
            self.viewport().setCursor(Qt.CursorShape.BlankCursor)
            return
        super().mouseReleaseEvent(event)

    def set_snap_marker(self, point: QPointF | None, kind: str | None) -> None:
        self._marker = (point, kind) if point is not None and kind else None
        self.viewport().update()

    def set_selection_box(self, corners: tuple[QPointF, QPointF] | None) -> None:
        """Show the rubber-band box dragged from the first corner to the second, or hide it."""
        self._box = corners
        self.viewport().update()

    def drawForeground(self, painter: QPainter, rect: QRectF) -> None:
        super().drawForeground(painter, rect)
        painter.save()
        painter.resetTransform()
        draw_axes_icon(painter, QPointF(AXES_ICON_MARGIN[0], self.viewport().height() - AXES_ICON_MARGIN[1]))
        if self._box is not None:
            first, second = self._box
            pixels = QRectF(QPointF(self.mapFromScene(first)), QPointF(self.mapFromScene(second))).normalized()
            draw_selection_box(painter, pixels, crossing=second.x() < first.x())
        if self._marker is not None:
            point, kind = self._marker
            draw_marker(painter, QPointF(self.mapFromScene(point)), kind)
        if self._pointer is not None and self._pan_origin is None:
            draw_crosshair(painter, self._pointer, self._crosshair_arm(), self._pickbox)
        painter.restore()

    def drawBackground(self, painter: QPainter, rect: QRectF) -> None:
        super().drawBackground(painter, rect)
        if not self._grid_visible:
            return
        step = grid_spacing(self.zoom_scale)
        self._draw_grid_lines(painter, rect, step, MINOR_GRID)
        self._draw_grid_lines(painter, rect, step * MAJOR_GRID_EVERY, MAJOR_GRID)

    @staticmethod
    def _draw_grid_lines(painter: QPainter, rect: QRectF, step: float, color: QColor) -> None:
        pen = QPen(color)
        pen.setCosmetic(True)
        painter.setPen(pen)
        x = math.floor(rect.left() / step) * step
        while x <= rect.right():
            painter.drawLine(QPointF(x, rect.top()), QPointF(x, rect.bottom()))
            x += step
        y = math.floor(rect.top() / step) * step
        while y <= rect.bottom():
            painter.drawLine(QPointF(rect.left(), y), QPointF(rect.right(), y))
            y += step



