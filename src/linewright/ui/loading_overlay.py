from PySide6.QtCore import Qt
from PySide6.QtWidgets import QFrame, QLabel, QProgressBar, QVBoxLayout

from linewright.canvas.view import CanvasView

CARD_WIDTH = 340


class LoadingOverlay(QFrame):
    """Covers the drawing area with a message and a moving bar while a file is opening."""

    def __init__(self, view: CanvasView) -> None:
        # A child of the view, not of its viewport: Qt scrolls a viewport's children along with the drawing.
        super().__init__(view)
        self.setObjectName("loadingOverlay")
        self.setAttribute(Qt.WidgetAttribute.WA_NoMousePropagation)  # clicks and the wheel must not reach the drawing
        self._title = QLabel()
        self._title.setObjectName("loadingTitle")
        bar = QProgressBar()
        bar.setRange(0, 0)
        bar.setTextVisible(False)
        bar.setFixedHeight(4)
        hint = QLabel("Large drawings can take a little while.")
        card = QFrame()
        card.setObjectName("loadingCard")
        card.setFixedWidth(CARD_WIDTH)
        card_layout = QVBoxLayout(card)
        card_layout.setContentsMargins(20, 18, 20, 18)
        card_layout.setSpacing(12)
        for widget in (self._title, bar, hint):
            card_layout.addWidget(widget)
        layout = QVBoxLayout(self)
        layout.addWidget(card, alignment=Qt.AlignmentFlag.AlignCenter)
        view.viewport_resized.connect(self._cover_view)
        self.hide()

    def show_opening(self, name: str) -> None:
        self._title.setText(f"Opening {name}")
        self._cover_view()
        self.show()
        self.raise_()

    def _cover_view(self) -> None:
        self.setGeometry(self.parentWidget().rect())
