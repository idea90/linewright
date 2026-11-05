from PySide6.QtCore import QObject, QSize, Qt, Signal
from PySide6.QtGui import QKeySequence, QShortcut
from PySide6.QtWidgets import QMainWindow, QPushButton

from linewright.snap.resolve import SnapSettings
from linewright.ui.icons import command_icon

TOGGLE_ICON = 16

# label, setting name, shortcut, icon
TOGGLES = (
    ("Grid", "show_grid", "F7", "GRID"),
    ("Snap", "grid", "F9", "SNAP"),
    ("Ortho", "ortho", "F8", "ORTHO"),
    ("Polar", "polar", "F10", "POLAR"),
    ("Object snap", "osnap", "F3", "OSNAP"),
)


class SnapToggles(QObject):
    """Status bar buttons and function keys for the snap modes."""

    changed = Signal()

    def __init__(self, window: QMainWindow, settings: SnapSettings) -> None:
        super().__init__(window)
        self._settings = settings
        self._buttons: dict[str, QPushButton] = {}
        for label, name, key, icon in TOGGLES:
            button = QPushButton(label)
            button.setIcon(command_icon(icon))
            button.setIconSize(QSize(TOGGLE_ICON, TOGGLE_ICON))
            button.setCheckable(True)
            button.setObjectName("snapToggle")
            button.setFocusPolicy(Qt.FocusPolicy.NoFocus)
            button.clicked.connect(lambda on, mode=name: self._set(mode, on))
            QShortcut(QKeySequence(key), window, button.click)
            window.statusBar().addPermanentWidget(button)
            self._buttons[name] = button
        self._sync_buttons()

    def _set(self, name: str, on: bool) -> None:
        self._settings.set(name, on)
        self._sync_buttons()
        self.changed.emit()

    def _sync_buttons(self) -> None:
        for name, button in self._buttons.items():
            button.setChecked(getattr(self._settings, name))

