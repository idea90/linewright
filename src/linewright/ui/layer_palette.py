from collections.abc import Callable

from PySide6.QtCore import Qt
from PySide6.QtGui import QColor, QFont
from PySide6.QtWidgets import (
    QAbstractItemView,
    QComboBox,
    QHBoxLayout,
    QHeaderView,
    QPushButton,
    QTableWidget,
    QTableWidgetItem,
    QVBoxLayout,
    QWidget,
)

from linewright import theme
from linewright.model.document import DEFAULT_LAYER_NAME, Document, Layer
from linewright.model.styles import COLOR_CHOICES, LINETYPES
from linewright.ui.layers import LayerService
from linewright.ui.properties_panels import swatch_icon

NAME, VISIBLE, LOCKED, COLOR, LINETYPE = range(5)
HEADERS = ("Name", "On", "Lock", "Color", "Linetype")
NAME_ROLE = Qt.ItemDataRole.UserRole
PALETTE_WIDTH = 420


class LayerPalette(QWidget):
    """A table of every layer with its switches, plus New, Delete and Set Current."""

    def __init__(self, document: Callable[[], Document], service: LayerService, report: Callable[[str], None]):
        super().__init__()
        self._document = document
        self._service = service
        self._report = report
        self._handling = False
        self._refresh_wanted = False

        self._table = QTableWidget(0, len(HEADERS))
        self._table.setObjectName("layerTable")
        self._table.setHorizontalHeaderLabels(HEADERS)
        self._table.verticalHeader().hide()
        self._table.setSelectionBehavior(QAbstractItemView.SelectionBehavior.SelectRows)
        self._table.setSelectionMode(QAbstractItemView.SelectionMode.SingleSelection)
        self._table.setShowGrid(False)
        self._table.setFocusPolicy(Qt.FocusPolicy.NoFocus)
        header = self._table.horizontalHeader()
        header.setSectionResizeMode(NAME, QHeaderView.ResizeMode.Stretch)
        for column in (VISIBLE, LOCKED):
            header.setSectionResizeMode(column, QHeaderView.ResizeMode.ResizeToContents)
        self._table.setColumnWidth(COLOR, 120)
        self._table.setColumnWidth(LINETYPE, 110)
        self._table.itemChanged.connect(self._item_changed)

        buttons = QHBoxLayout()
        for text, handler in (("New", self._new), ("Delete", self._delete), ("Set Current", self._set_current)):
            button = QPushButton(text)
            button.setFocusPolicy(Qt.FocusPolicy.NoFocus)
            button.clicked.connect(handler)
            buttons.addWidget(button)
        buttons.addStretch()
        layout = QVBoxLayout(self)
        layout.setContentsMargins(8, 8, 8, 8)
        layout.addWidget(self._table)
        layout.addLayout(buttons)
        self.setMinimumWidth(PALETTE_WIDTH)

    def refresh(self) -> None:
        if self._handling:
            self._refresh_wanted = True
            return
        document = self._document()
        chosen = self._selected_name()
        self._table.blockSignals(True)
        self._table.setRowCount(0)
        for row, layer in enumerate(document.layers.values()):
            self._table.insertRow(row)
            self._fill_row(row, layer, layer.name == document.current_layer)
            if layer.name == chosen:
                self._table.selectRow(row)
        self._table.blockSignals(False)

    def _fill_row(self, row: int, layer: Layer, current: bool) -> None:
        name = QTableWidgetItem(layer.name)
        name.setData(NAME_ROLE, layer.name)
        editable = Qt.ItemFlag.ItemIsEditable if layer.name != DEFAULT_LAYER_NAME else Qt.ItemFlag(0)
        name.setFlags(Qt.ItemFlag.ItemIsEnabled | Qt.ItemFlag.ItemIsSelectable | editable)
        if current:
            font = QFont(name.font())
            font.setBold(True)
            name.setFont(font)
            name.setForeground(QColor(theme.ACCENT))
        self._table.setItem(row, NAME, name)
        self._table.setItem(row, VISIBLE, self._switch(layer.name, layer.visible))
        self._table.setItem(row, LOCKED, self._switch(layer.name, layer.locked))
        self._table.setCellWidget(row, COLOR, self._color_box(layer))
        self._table.setCellWidget(row, LINETYPE, self._linetype_box(layer))

    @staticmethod
    def _switch(name: str, on: bool) -> QTableWidgetItem:
        item = QTableWidgetItem()
        item.setData(NAME_ROLE, name)
        item.setFlags(Qt.ItemFlag.ItemIsEnabled | Qt.ItemFlag.ItemIsUserCheckable)
        item.setCheckState(Qt.CheckState.Checked if on else Qt.CheckState.Unchecked)
        return item

    def _color_box(self, layer: Layer) -> QComboBox:
        box = QComboBox()
        for aci, label in COLOR_CHOICES:
            box.addItem(swatch_icon(aci), label, aci)
        if box.findData(layer.color) < 0:
            box.addItem(swatch_icon(layer.color), f"Color {layer.color}", layer.color)
        box.setCurrentIndex(box.findData(layer.color))
        box.setFocusPolicy(Qt.FocusPolicy.NoFocus)
        box.activated.connect(lambda index, name=layer.name, box=box: self._set_color(name, box.itemData(index)))
        return box

    def _linetype_box(self, layer: Layer) -> QComboBox:
        box = QComboBox()
        box.addItems(LINETYPES)
        if layer.linetype not in LINETYPES:
            box.addItem(layer.linetype)
        box.setCurrentText(layer.linetype)
        box.setFocusPolicy(Qt.FocusPolicy.NoFocus)
        box.activated.connect(lambda index, name=layer.name, box=box: self._set_linetype(name, box.itemText(index)))
        return box

    def _selected_name(self) -> str | None:
        rows = self._table.selectionModel().selectedRows() if self._table.selectionModel() else []
        item = self._table.item(rows[0].row(), NAME) if rows else None
        return item.data(NAME_ROLE) if item else None

    def _run(self, change: Callable[[], str | None]) -> None:
        """Make a change through the service. The table is rebuilt after, never while its own signal is running."""
        self._handling = True
        try:
            message = change()
        finally:
            self._handling = False
        if message:
            self._report(message)
        self._refresh_wanted = False
        self.refresh()

    def _item_changed(self, item: QTableWidgetItem) -> None:
        name = item.data(NAME_ROLE)
        column = item.column()
        checked = item.checkState() == Qt.CheckState.Checked
        if column == NAME:
            self._run(lambda: self._service.rename(name, item.text()))
        elif column == VISIBLE:
            self._run(lambda: self._service.set_visible(name, checked))
        elif column == LOCKED:
            self._run(lambda: self._service.set_locked(name, checked))

    def _set_color(self, name: str, aci: int) -> None:
        self._run(lambda: self._service.set_color(name, aci))

    def _set_linetype(self, name: str, linetype: str) -> None:
        self._run(lambda: self._service.set_linetype(name, linetype))

    def _new(self) -> None:
        self._service.create()
        self.refresh()
        row = self._table.rowCount() - 1
        self._table.selectRow(row)
        self._table.editItem(self._table.item(row, NAME))

    def _delete(self) -> None:
        name = self._selected_name()
        if name is not None:
            self._run(lambda: self._service.delete(name))

    def _set_current(self) -> None:
        name = self._selected_name()
        if name is not None:
            self._run(lambda: self._service.set_current(name))
