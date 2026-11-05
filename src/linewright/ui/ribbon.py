from dataclasses import dataclass

from PySide6.QtCore import QSize, Qt
from PySide6.QtGui import QAction
from PySide6.QtWidgets import QFrame, QGridLayout, QHBoxLayout, QLabel, QMenu, QToolButton, QVBoxLayout, QWidget

LARGE_ICON = 28
SMALL_ICON = 20
PROPERTIES_TITLE = "Properties"


@dataclass(frozen=True)
class Panel:
    title: str
    large: tuple[str, ...] = ()  # command names shown as big buttons with the label underneath
    small: tuple[str, ...] = ()  # command names shown as icon-only buttons, filled down each column in turn
    rows: int = 2  # how many small buttons stack in a column


PANELS = (
    Panel("Draw", large=("LINE", "PLINE", "RECTANGLE", "CIRCLE", "ARC")),
    Panel("Modify", small=("MOVE", "MIRROR", "COPY", "OFFSET", "SCALE", "FILLET", "ROTATE", "TRIM", "ERASE", "EXTEND")),
    Panel("Annotate and measure", large=("TEXT", "DIMLINEAR", "DISTANCE", "AREA")),
)

# Commands behind a large button: press and hold it to see them.
MORE = {"DIMLINEAR": ("DIMALIGNED", "DIMSCALE")}


class Ribbon(QWidget):
    """One row of titled groups of command buttons, ending with the layer and property controls."""

    def __init__(self, actions: dict[str, QAction], properties: list[QWidget]) -> None:
        super().__init__()
        self.setObjectName("ribbonBody")
        self.setAttribute(Qt.WidgetAttribute.WA_StyledBackground)
        row = QHBoxLayout(self)
        row.setContentsMargins(10, 8, 10, 6)
        row.setSpacing(0)
        groups = [self._group(panel.title, self._panel(panel, actions)) for panel in PANELS]
        groups.append(self._group(PROPERTIES_TITLE, self._side_by_side(properties)))
        for index, group in enumerate(groups):
            if index:
                row.addWidget(self._divider())
            row.addWidget(group)
        row.addStretch()

    @staticmethod
    def _divider() -> QFrame:
        divider = QFrame()
        divider.setObjectName("panelDivider")
        divider.setFrameShape(QFrame.Shape.NoFrame)  # the stylesheet paints it, a line shape would use palette colors
        return divider

    @staticmethod
    def _group(title: str, content: QWidget) -> QWidget:
        label = QLabel(title)
        label.setObjectName("groupTitle")
        label.setAlignment(Qt.AlignmentFlag.AlignCenter)
        group = QWidget()
        layout = QVBoxLayout(group)
        layout.setContentsMargins(6, 0, 6, 0)
        layout.setSpacing(0)
        layout.addWidget(content, 1)
        layout.addWidget(label)
        return group

    @staticmethod
    def _side_by_side(widgets: list[QWidget]) -> QWidget:
        holder = QWidget()
        layout = QHBoxLayout(holder)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(12)
        for widget in widgets:
            layout.addWidget(widget)
        return holder

    def _panel(self, panel: Panel, actions: dict[str, QAction]) -> QWidget:
        content = QGridLayout()
        content.setContentsMargins(0, 0, 0, 0)
        content.setHorizontalSpacing(2)
        content.setVerticalSpacing(2)
        column = 0
        for name in panel.large:
            button = self._button(actions[name], large=True)
            if name in MORE:
                self._attach_menu(button, [actions[more] for more in MORE[name]])
            content.addWidget(button, 0, column, panel.rows, 1)
            column += 1
        for index, name in enumerate(panel.small):
            row, small_column = index % panel.rows, column + index // panel.rows
            content.addWidget(self._button(actions[name], large=False), row, small_column)
        holder = QWidget()
        holder.setLayout(content)
        return holder

    @staticmethod
    def _attach_menu(button: QToolButton, more: list[QAction]) -> None:
        menu = QMenu(button)
        menu.addActions(more)
        button.setMenu(menu)
        button.setPopupMode(QToolButton.ToolButtonPopupMode.DelayedPopup)
        button.setToolTip(f"{button.toolTip()}. Hold for {' and '.join(action.text() for action in more)}.")

    @staticmethod
    def _button(action: QAction, large: bool) -> QToolButton:
        button = QToolButton()
        button.setDefaultAction(action)
        button.setObjectName("ribbonLarge" if large else "ribbonSmall")
        button.setFocusPolicy(Qt.FocusPolicy.NoFocus)
        if large:
            button.setIconSize(QSize(LARGE_ICON, LARGE_ICON))
            button.setToolButtonStyle(Qt.ToolButtonStyle.ToolButtonTextUnderIcon)
        else:
            button.setIconSize(QSize(SMALL_ICON, SMALL_ICON))
            button.setToolButtonStyle(Qt.ToolButtonStyle.ToolButtonIconOnly)
        return button
