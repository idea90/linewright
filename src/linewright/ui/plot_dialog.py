from PySide6.QtGui import QPageSize
from PySide6.QtWidgets import QComboBox, QDialog, QDialogButtonBox, QFormLayout, QLabel, QVBoxLayout, QWidget

from linewright import theme
from linewright.canvas.plot import FIT, PAPER_SIZES, parse_scale

DIALOG_TITLE = "Export PDF"
DEFAULT_PAPER = "A4"
LANDSCAPE, PORTRAIT = "Landscape", "Portrait"
SCALES = (FIT, "1:1", "1:2", "1:5", "1:10", "1:20", "1:50", "1:100", "1:200", "1:500", "2:1")


class PlotDialog(QDialog):
    """Asks for the paper, its orientation and the scale. The chosen values are read back after it is accepted."""

    def __init__(self, parent: QWidget, unit_name: str) -> None:
        super().__init__(parent)
        self.setWindowTitle(DIALOG_TITLE)
        self.scale: float | None = None
        self.scale_text = FIT

        self._paper = QComboBox()
        self._paper.addItems(PAPER_SIZES)
        self._paper.setCurrentText(DEFAULT_PAPER)
        self._orientation = QComboBox()
        self._orientation.addItems((LANDSCAPE, PORTRAIT))
        self._scale = QComboBox()
        self._scale.setEditable(True)
        self._scale.addItems(SCALES)
        units = QLabel(f"Drawing units: {unit_name}")
        units.setStyleSheet(f"color: {theme.TEXT_DIM};")
        self._error = QLabel()
        self._error.setStyleSheet(f"color: {theme.ERROR};")

        form = QFormLayout()
        form.addRow("Paper", self._paper)
        form.addRow("Orientation", self._orientation)
        form.addRow("Scale", self._scale)
        form.addRow(units)
        buttons = QDialogButtonBox(QDialogButtonBox.StandardButton.Ok | QDialogButtonBox.StandardButton.Cancel)
        buttons.accepted.connect(self.accept)
        buttons.rejected.connect(self.reject)
        layout = QVBoxLayout(self)
        layout.addLayout(form)
        layout.addWidget(self._error)
        layout.addWidget(buttons)

    @property
    def paper(self) -> QPageSize.PageSizeId:
        return PAPER_SIZES[self._paper.currentText()]

    @property
    def landscape(self) -> bool:
        return self._orientation.currentText() == LANDSCAPE

    def accept(self) -> None:
        self.scale_text = self._scale.currentText().strip()
        try:
            self.scale = parse_scale(self.scale_text)
        except ValueError as error:
            self._error.setText(str(error))
            return
        super().accept()
