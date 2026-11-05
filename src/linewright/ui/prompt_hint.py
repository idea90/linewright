from PySide6.QtCore import QSize
from PySide6.QtWidgets import QFrame, QHBoxLayout, QLabel

from linewright.canvas.view import CanvasView
from linewright.ui.icons import command_icon

HINT_ICON = 16
EDGE_GAP = 16


class PromptHint(QFrame):
    """A chip in the corner of the drawing area naming the running command and what it asks for."""

    def __init__(self, view: CanvasView) -> None:
        # A child of the view, not of its viewport: Qt scrolls a viewport's children along with the drawing.
        super().__init__(view)
        self.setObjectName("promptHint")
        self._icon = QLabel()
        self._command = QLabel()
        self._command.setObjectName("hintCommand")
        self._message = QLabel()
        layout = QHBoxLayout(self)
        layout.setContentsMargins(12, 7, 14, 7)
        layout.setSpacing(8)
        for label in (self._icon, self._command, self._message):
            layout.addWidget(label)
        self.move(EDGE_GAP, EDGE_GAP)
        self.hide()

    def show_prompt(self, icon_name: str, command: str, message: str) -> None:
        self._icon.setPixmap(command_icon(icon_name).pixmap(QSize(HINT_ICON, HINT_ICON)))
        self._command.setText(command)
        self._message.setText(f"\u00b7 {message.rstrip(':')}")
        self.adjustSize()
        self.show()
        self.raise_()
