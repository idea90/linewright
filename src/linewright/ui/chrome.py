from PySide6.QtCore import QSize, Qt, Signal
from PySide6.QtGui import QAction, QMouseEvent
from PySide6.QtWidgets import QFrame, QHBoxLayout, QLabel, QMenu, QToolButton, QWidget

from linewright.ui.icons import command_icon

QAT_ICON = 18
WINDOW_BUTTON_ICON = 16
HEADER_HEIGHT = 40
WINDOW_NAME = "Linewright"
TAB_ICON = 14


class Header(QWidget):
    """The window's title bar: menu button, quick access toolbar, title and the window buttons.

    It stands in for the system title bar, so it also moves the window and toggles maximize.
    """

    def __init__(self, menu: QMenu, quick_actions: list[QAction | None]) -> None:
        super().__init__()
        self.setObjectName("header")
        self.setAttribute(Qt.WidgetAttribute.WA_StyledBackground)
        self.setFixedHeight(HEADER_HEIGHT)
        layout = QHBoxLayout(self)
        layout.setContentsMargins(8, 0, 0, 0)
        layout.setSpacing(2)

        layout.addWidget(self._menu_button(menu))
        layout.addWidget(self._divider())
        for action in quick_actions:
            layout.addWidget(self._divider() if action is None else self._quick_button(action))

        self._title = self._title_label("windowTitle")
        app_name = self._title_label("windowApp")
        app_name.setText(f"· {WINDOW_NAME}")
        layout.addStretch(1)
        layout.addWidget(self._title)
        layout.addSpacing(6)
        layout.addWidget(app_name)
        layout.addStretch(1)

        self._maximize = self._window_button("WIN_MAXIMIZE", "Maximize", self._toggle_maximize)
        layout.addWidget(self._window_button("WIN_MINIMIZE", "Minimize", lambda: self.window().showMinimized()))
        layout.addWidget(self._maximize)
        layout.addWidget(self._window_button("WIN_CLOSE", "Close", lambda: self.window().close(), "windowClose"))

    def set_title(self, name: str) -> None:
        self._title.setText(name)

    def set_maximized(self, maximized: bool) -> None:
        self._maximize.setIcon(command_icon("WIN_RESTORE" if maximized else "WIN_MAXIMIZE"))
        self._maximize.setToolTip("Restore Down" if maximized else "Maximize")

    def mousePressEvent(self, event: QMouseEvent) -> None:
        handle = self.window().windowHandle()
        if event.button() == Qt.MouseButton.LeftButton and handle is not None:
            handle.startSystemMove()

    def mouseDoubleClickEvent(self, event: QMouseEvent) -> None:
        if event.button() == Qt.MouseButton.LeftButton:
            self._toggle_maximize()

    def _toggle_maximize(self) -> None:
        window = self.window()
        if window.isMaximized():
            window.showNormal()
        else:
            window.showMaximized()

    @staticmethod
    def _divider() -> QFrame:
        divider = QFrame()
        divider.setObjectName("headerDivider")
        divider.setFrameShape(QFrame.Shape.NoFrame)
        return divider

    @staticmethod
    def _title_label(object_name: str) -> QLabel:
        label = QLabel()
        label.setObjectName(object_name)
        label.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents)
        return label

    @staticmethod
    def _menu_button(menu: QMenu) -> QToolButton:
        button = QToolButton()
        button.setObjectName("appButton")
        button.setText(WINDOW_NAME)
        button.setIcon(command_icon("MENU"))
        button.setIconSize(QSize(QAT_ICON, QAT_ICON))
        button.setToolButtonStyle(Qt.ToolButtonStyle.ToolButtonTextBesideIcon)
        button.setPopupMode(QToolButton.ToolButtonPopupMode.InstantPopup)
        button.setMenu(menu)
        button.setFocusPolicy(Qt.FocusPolicy.NoFocus)
        return button

    @staticmethod
    def _quick_button(action: QAction) -> QToolButton:
        button = QToolButton()
        button.setObjectName("qat")
        button.setDefaultAction(action)
        button.setIconSize(QSize(QAT_ICON, QAT_ICON))
        button.setFocusPolicy(Qt.FocusPolicy.NoFocus)
        return button

    @staticmethod
    def _window_button(icon_name: str, tip: str, handler, object_name: str = "windowButton") -> QToolButton:
        button = QToolButton()
        button.setObjectName(object_name)
        button.setIcon(command_icon(icon_name))
        button.setIconSize(QSize(WINDOW_BUTTON_ICON, WINDOW_BUTTON_ICON))
        button.setToolTip(tip)
        button.setFocusPolicy(Qt.FocusPolicy.NoFocus)
        button.clicked.connect(handler)
        return button


class DocumentTab(QWidget):
    """The strip under the toolbar with the open drawing's tab and a button for a new drawing."""

    close_requested = Signal()
    new_requested = Signal()

    def __init__(self) -> None:
        super().__init__()
        self.setObjectName("docStrip")
        self.setAttribute(Qt.WidgetAttribute.WA_StyledBackground)
        self._label = QLabel()
        close = self._tab_button("tabClose", "WIN_CLOSE", "Close drawing", self.close_requested)
        tab = QFrame()
        tab.setObjectName("docTab")
        tab_layout = QHBoxLayout(tab)
        tab_layout.setContentsMargins(14, 4, 6, 4)
        tab_layout.setSpacing(10)
        tab_layout.addWidget(self._label)
        tab_layout.addWidget(close)
        layout = QHBoxLayout(self)
        layout.setContentsMargins(8, 6, 8, 0)
        layout.setSpacing(6)
        layout.addWidget(tab)
        layout.addWidget(self._tab_button("tabNew", "PLUS", "New drawing", self.new_requested))
        layout.addStretch()

    @staticmethod
    def _tab_button(object_name: str, icon_name: str, tip: str, signal: Signal) -> QToolButton:
        button = QToolButton()
        button.setObjectName(object_name)
        button.setIcon(command_icon(icon_name))
        button.setIconSize(QSize(TAB_ICON, TAB_ICON))
        button.setToolTip(tip)
        button.setFocusPolicy(Qt.FocusPolicy.NoFocus)
        button.clicked.connect(signal)
        return button

    def set_title(self, name: str, modified: bool) -> None:
        self._label.setText(f"{name}{'*' if modified else ''}")

    @property
    def title(self) -> str:
        return self._label.text()
