from PySide6.QtCore import QEvent, QObject, QPoint, QRect, Qt
from PySide6.QtGui import QCursor, QGuiApplication, QMouseEvent
from PySide6.QtWidgets import QApplication, QMainWindow

EDGE_WIDTH = 6  # pixels inside the window border that grab a resize

_CURSORS = {
    Qt.Edge.LeftEdge: Qt.CursorShape.SizeHorCursor,
    Qt.Edge.RightEdge: Qt.CursorShape.SizeHorCursor,
    Qt.Edge.TopEdge: Qt.CursorShape.SizeVerCursor,
    Qt.Edge.BottomEdge: Qt.CursorShape.SizeVerCursor,
    Qt.Edge.LeftEdge | Qt.Edge.TopEdge: Qt.CursorShape.SizeFDiagCursor,
    Qt.Edge.RightEdge | Qt.Edge.BottomEdge: Qt.CursorShape.SizeFDiagCursor,
    Qt.Edge.RightEdge | Qt.Edge.TopEdge: Qt.CursorShape.SizeBDiagCursor,
    Qt.Edge.LeftEdge | Qt.Edge.BottomEdge: Qt.CursorShape.SizeBDiagCursor,
}


def edges_at(point: QPoint, frame: QRect, width: int = EDGE_WIDTH) -> Qt.Edge:
    """Which window edges a screen position grabs. Nothing outside the frame or away from its border."""
    edges = Qt.Edge(0)
    if not frame.contains(point):
        return edges
    if point.x() < frame.left() + width:
        edges |= Qt.Edge.LeftEdge
    if point.x() > frame.right() - width:
        edges |= Qt.Edge.RightEdge
    if point.y() < frame.top() + width:
        edges |= Qt.Edge.TopEdge
    if point.y() > frame.bottom() - width:
        edges |= Qt.Edge.BottomEdge
    return edges


class FrameResizer(QObject):
    """Lets a window without a system frame be resized from its edges, using the system's own resize."""

    def __init__(self, window: QMainWindow) -> None:
        super().__init__(window)
        self._window = window
        self._cursor_shown = False
        QApplication.instance().installEventFilter(self)

    def eventFilter(self, watched: QObject, event: QEvent) -> bool:
        if event.type() == QEvent.Type.MouseMove:
            self._update_cursor(self._edges_under(QCursor.pos()))
        elif event.type() == QEvent.Type.MouseButtonPress and self._is_grab(event):
            return self._start_resize(QCursor.pos())
        return False

    def _edges_under(self, position: QPoint) -> Qt.Edge:
        window = self._window
        if window.isMaximized() or window.isFullScreen() or not window.isActiveWindow():
            return Qt.Edge(0)
        return edges_at(position, window.frameGeometry())

    @staticmethod
    def _is_grab(event: QMouseEvent) -> bool:
        return event.button() == Qt.MouseButton.LeftButton

    def _start_resize(self, position: QPoint) -> bool:
        edges = self._edges_under(position)
        handle = self._window.windowHandle()
        return bool(edges) and handle is not None and handle.startSystemResize(edges)

    def _update_cursor(self, edges: Qt.Edge) -> None:
        if not edges:
            if self._cursor_shown:
                QGuiApplication.restoreOverrideCursor()
                self._cursor_shown = False
            return
        shape = _CURSORS.get(edges)
        if shape is None:
            return
        if self._cursor_shown:
            QGuiApplication.changeOverrideCursor(shape)
        else:
            QGuiApplication.setOverrideCursor(shape)
            self._cursor_shown = True
