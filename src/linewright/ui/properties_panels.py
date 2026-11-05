from collections.abc import Callable

from ezdxf.colors import aci2rgb
from PySide6.QtCore import QSize, Qt
from PySide6.QtGui import QAction, QColor, QIcon, QPixmap
from PySide6.QtWidgets import QComboBox, QLabel, QGridLayout, QToolButton, QWidget

from linewright.model.document import Document
from linewright.model.entities import BYLAYER
from linewright.model.styles import BYLAYER_LINETYPE, COLOR_CHOICES, LINETYPES
from linewright.ui.controller import DrawingController
from linewright.ui.layers import LayerService

SWATCH = 14
VARIES = "*VARIES*"
BYLAYER_LABEL = "ByLayer"
PANEL_ICON = 18
LAYER_COMBO_WIDTH = 150
PROPERTY_COMBO_WIDTH = 120
FIELD_SPACING = 6
ROW_SPACING = 6


def swatch_icon(aci: int) -> QIcon:
    pixmap = QPixmap(SWATCH, SWATCH)
    if 1 <= aci <= 255:
        pixmap.fill(QColor(*aci2rgb(aci)))
    else:
        pixmap.fill(Qt.GlobalColor.transparent)
    return QIcon(pixmap)


def _field_label(text: str) -> QLabel:
    label = QLabel(text)
    label.setObjectName("fieldLabel")
    return label


def _common(values: list):
    """The one value they all share, or None when they differ or there are none."""
    return values[0] if values and all(value == values[0] for value in values) else None


class LayerPanel(QWidget):
    """The toolbar's layer list and the button that opens the layer palette."""

    def __init__(
        self,
        document: Callable[[], Document],
        controller: DrawingController,
        service: LayerService,
        report: Callable[[str], None],
        palette_action: QAction,
    ) -> None:
        super().__init__()
        self._document = document
        self._controller = controller
        self._service = service
        self._report = report
        self._combo = QComboBox()
        self._combo.setObjectName("layerCombo")
        self._combo.setMinimumWidth(LAYER_COMBO_WIDTH)
        self._combo.setFocusPolicy(Qt.FocusPolicy.NoFocus)
        self._combo.activated.connect(self._chosen)
        button = QToolButton()
        button.setObjectName("ribbonText")
        button.setDefaultAction(palette_action)
        button.setIconSize(QSize(PANEL_ICON, PANEL_ICON))
        button.setToolButtonStyle(Qt.ToolButtonStyle.ToolButtonTextBesideIcon)
        button.setFocusPolicy(Qt.FocusPolicy.NoFocus)
        layout = QGridLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setHorizontalSpacing(FIELD_SPACING)
        layout.setVerticalSpacing(ROW_SPACING)
        layout.addWidget(_field_label("Layer"), 0, 0)
        layout.addWidget(self._combo, 0, 1)
        layout.addWidget(button, 1, 0, 1, 2, Qt.AlignmentFlag.AlignHCenter)

    def refresh(self) -> None:
        document = self._document()
        selected = self._controller.selected()
        shown = _common([entity.layer for entity in selected]) if selected else document.current_layer
        self._combo.clear()
        for layer in document.layers.values():
            self._combo.addItem(swatch_icon(layer.color), layer.name, layer.name)
        if shown is None:
            self._combo.addItem(VARIES, None)
            shown = VARIES
        self._combo.setCurrentIndex(max(self._combo.findText(shown), 0))

    def _chosen(self, index: int) -> None:
        name = self._combo.itemData(index)
        if name is None:
            return
        if self._controller.selected():
            self._controller.change_selected(layer=name)
            return
        message = self._service.set_current(name)
        if message:
            self._report(message)
        self.refresh()


class PropertiesPanel(QWidget):
    """The toolbar's color and linetype of the selection, or of what is drawn next."""

    def __init__(self, document: Callable[[], Document], controller: DrawingController) -> None:
        super().__init__()
        self._document = document
        self._controller = controller
        self._color = self._combo("colorCombo", self._color_chosen)
        self._linetype = self._combo("linetypeCombo", self._linetype_chosen)
        layout = QGridLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setHorizontalSpacing(FIELD_SPACING)
        layout.setVerticalSpacing(ROW_SPACING)
        layout.addWidget(_field_label("Color"), 0, 0)
        layout.addWidget(self._color, 0, 1)
        layout.addWidget(_field_label("Linetype"), 1, 0)
        layout.addWidget(self._linetype, 1, 1)

    @staticmethod
    def _combo(name: str, handler: Callable[[int], None]) -> QComboBox:
        combo = QComboBox()
        combo.setObjectName(name)
        combo.setMinimumWidth(PROPERTY_COMBO_WIDTH)
        combo.setFocusPolicy(Qt.FocusPolicy.NoFocus)
        combo.activated.connect(handler)
        return combo

    def refresh(self) -> None:
        document = self._document()
        selected = self._controller.selected()
        color = _common([e.color for e in selected]) if selected else document.current_color
        linetype = _common([e.linetype for e in selected]) if selected else document.current_linetype
        self._fill_colors(color)
        self._fill_linetypes(linetype)

    def _fill_colors(self, shown: int | None) -> None:
        self._color.clear()
        self._color.addItem(swatch_icon(BYLAYER), BYLAYER_LABEL, BYLAYER)
        for aci, name in COLOR_CHOICES:
            self._color.addItem(swatch_icon(aci), name, aci)
        if shown is None:
            self._color.addItem(VARIES, None)
        elif self._color.findData(shown) < 0:
            self._color.addItem(swatch_icon(shown), f"Color {shown}", shown)
        self._color.setCurrentIndex(self._color.findData(shown) if shown is not None else self._color.count() - 1)

    def _fill_linetypes(self, shown: str | None) -> None:
        self._linetype.clear()
        for name in (BYLAYER_LINETYPE, *LINETYPES):
            self._linetype.addItem(name, name)
        if shown is None:
            self._linetype.addItem(VARIES, None)
        elif self._linetype.findData(shown) < 0:
            self._linetype.addItem(shown, shown)
        index = self._linetype.findData(shown) if shown is not None else self._linetype.count() - 1
        self._linetype.setCurrentIndex(index)

    def _color_chosen(self, index: int) -> None:
        aci = self._color.itemData(index)
        if aci is None:
            return
        if self._controller.selected():
            self._controller.change_selected(color=aci)
        else:
            self._document().current_color = aci

    def _linetype_chosen(self, index: int) -> None:
        name = self._linetype.itemData(index)
        if name is None:
            return
        if self._controller.selected():
            self._controller.change_selected(linetype=name)
        else:
            self._document().current_linetype = name
