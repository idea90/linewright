from PySide6.QtCore import QSize, Signal
from PySide6.QtWidgets import QFrame, QToolButton, QVBoxLayout

from linewright.canvas.view import CanvasView
from linewright.ui.icons import command_icon

NAV_ICON = 20
RIGHT_MARGIN = 12


class NavigationBar(QFrame):
    """The small zoom buttons floating at the right edge of the drawing area."""

    zoom_extents = Signal()
    zoom_in = Signal()
    zoom_out = Signal()

    def __init__(self, view: CanvasView) -> None:
        # A child of the view, not of its viewport: Qt scrolls a viewport's children along with the drawing.
        super().__init__(view)
        self.setObjectName("navBar")
        layout = QVBoxLayout(self)
        layout.setContentsMargins(3, 3, 3, 3)
        layout.setSpacing(2)
        buttons = (
            ("ZOOM_EXTENTS", "Zoom Extents (Ctrl+E)", self.zoom_extents),
            ("ZOOM_IN", "Zoom In", self.zoom_in),
            ("ZOOM_OUT", "Zoom Out", self.zoom_out),
        )
        for icon_name, tip, signal in buttons:
            button = QToolButton()
            button.setObjectName("navButton")
            button.setIcon(command_icon(icon_name))
            button.setIconSize(QSize(NAV_ICON, NAV_ICON))
            button.setToolTip(tip)
            button.clicked.connect(signal)
            layout.addWidget(button)
        view.viewport_resized.connect(self._place)
        self._place(view.viewport().size())

    def _place(self, canvas_size: QSize) -> None:
        self.adjustSize()
        self.move(
            canvas_size.width() - self.width() - RIGHT_MARGIN, (canvas_size.height() - self.height()) // 2
        )
        self.raise_()

