from PySide6.QtCore import Qt, Signal
from PySide6.QtGui import QKeyEvent
from PySide6.QtWidgets import QFrame, QHBoxLayout, QLabel, QLineEdit, QPlainTextEdit, QVBoxLayout, QWidget

HISTORY_LINES = 3
MAX_HISTORY_BLOCKS = 500


class CommandInput(QLineEdit):
    delete_on_empty = Signal()

    def keyPressEvent(self, event: QKeyEvent) -> None:
        """Delete with nothing typed erases the selected drawing objects, as in AutoCAD."""
        if event.key() == Qt.Key.Key_Delete and not self.text():
            self.delete_on_empty.emit()
            return
        super().keyPressEvent(event)


class CommandLine(QWidget):
    submitted = Signal(str)
    delete_pressed = Signal()

    def __init__(self) -> None:
        super().__init__()
        self.setObjectName("commandPanel")
        self.setAttribute(Qt.WidgetAttribute.WA_StyledBackground)
        self._history = QPlainTextEdit()
        self._history.setObjectName("history")
        self._history.setReadOnly(True)
        self._history.setFrameShape(QPlainTextEdit.Shape.NoFrame)
        self._history.setVerticalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
        self._history.setMaximumBlockCount(MAX_HISTORY_BLOCKS)
        self._history.setFixedHeight(self._history.fontMetrics().lineSpacing() * HISTORY_LINES + 8)
        self._prompt = QLabel("Command:")
        self._prompt.setObjectName("prompt")
        self._input = CommandInput()
        self._input.setObjectName("commandInput")
        self._input.returnPressed.connect(self._submit)
        self._input.delete_on_empty.connect(self.delete_pressed)

        glyph = QLabel(">_")
        glyph.setObjectName("promptGlyph")
        box = QFrame()
        box.setObjectName("commandBox")
        row = QHBoxLayout(box)
        row.setContentsMargins(14, 2, 10, 2)
        row.setSpacing(10)
        row.addWidget(glyph)
        row.addWidget(self._prompt)
        row.addWidget(self._input, 1)
        layout = QVBoxLayout(self)
        layout.setContentsMargins(16, 8, 16, 10)
        layout.setSpacing(8)
        layout.addWidget(self._history)
        layout.addWidget(box)

    def print(self, message: str) -> None:
        self._history.appendPlainText(message)

    @property
    def prompt(self) -> str:
        return self._prompt.text()

    def set_prompt(self, prompt: str) -> None:
        self._prompt.setText(prompt)

    def set_hint(self, hint: str) -> None:
        self._input.setPlaceholderText(hint)

    def focus(self) -> None:
        self._input.setFocus()

    def _submit(self) -> None:
        text = self._input.text().strip()
        self._input.clear()
        self.submitted.emit(text)

