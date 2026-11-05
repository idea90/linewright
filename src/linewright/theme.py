from PySide6.QtGui import QColor, QFont, QPalette
from PySide6.QtWidgets import QApplication, QStyleFactory

CANVAS = "#0a0f1e"
GRID_MINOR = "#0e1526"
GRID_MAJOR = "#141c31"
PANEL = "#0e1527"
PANEL_RAISED = "#19233b"
PANEL_DARK = "#0b101f"
BORDER = "#1a2338"
TEXT = "#e8ecf5"
TEXT_DIM = "#8b96ac"
ACCENT = "#4c8dff"
ACCENT_TINT = "rgba(76, 141, 255, 0.18)"
ACCENT_EDGE = "#2c4a8a"
BORDER_STRONG = "#222d48"
ON_ACCENT = "#0a1226"
SNAP_MARKER = "#ffd600"
CROSSING = "#5fd48a"
CROSSHAIR = "#f2f4f7"
AXIS_X = "#5b8cff"
AXIS_Y = "#38b0ff"
ERROR = "#ff7b72"

UI_FONTS = ["Segoe UI Variable Text", "Segoe UI"]
MONO_FONTS = ["Cascadia Mono", "Consolas"]
UI_POINT_SIZE = 9.5

_MONO_CSS = ", ".join(f'"{name}"' for name in MONO_FONTS)

STYLESHEET = f"""
QToolTip {{ background: {PANEL_RAISED}; color: {TEXT}; border: 1px solid {BORDER_STRONG}; padding: 4px 8px; }}
QMenu {{ background: {PANEL_RAISED}; color: {TEXT}; border: 1px solid {BORDER_STRONG}; border-radius: 6px;
    padding: 4px; }}
QMenu::item {{ padding: 6px 28px 6px 12px; border-radius: 4px; }}
QMenu::item:selected {{ background: {ACCENT_TINT}; }}
QMenu::item:disabled {{ color: {TEXT_DIM}; }}
QMenu::separator {{ height: 1px; background: {BORDER}; margin: 4px 8px; }}

QMainWindow {{ border: 1px solid {BORDER_STRONG}; }}
QWidget#header {{ background: {PANEL_DARK}; border-bottom: 1px solid {BORDER}; }}
QLabel#windowTitle {{ color: {TEXT}; }}
QLabel#windowApp {{ color: {TEXT_DIM}; }}
QFrame#headerDivider {{ background: {BORDER_STRONG}; min-width: 1px; max-width: 1px;
    min-height: 18px; max-height: 18px; }}
QToolButton#windowButton, QToolButton#windowClose {{ background: transparent; border: none; border-radius: 0;
    min-width: 46px; min-height: 40px; }}
QToolButton#windowButton:hover {{ background: {PANEL_RAISED}; }}
QToolButton#windowClose:hover {{ background: #c42b1c; }}
QToolButton#appButton {{ background: transparent; color: {ACCENT}; border: none; border-radius: 6px;
    padding: 4px 8px; font-size: 13pt; font-weight: 600; }}
QToolButton#appButton:hover {{ background: {PANEL_RAISED}; }}
QToolButton#appButton::menu-indicator {{ image: none; }}
QToolButton#qat {{ background: transparent; border: none; border-radius: 6px; padding: 5px; }}
QToolButton#qat:hover {{ background: {PANEL_RAISED}; }}
QToolButton#qat:disabled {{ background: transparent; }}

QWidget#ribbonBody {{ background: {PANEL}; border-bottom: 1px solid {BORDER}; }}
QFrame#panelDivider {{ background: {BORDER}; min-width: 1px; max-width: 1px; }}
QLabel#groupTitle {{ color: {TEXT_DIM}; font-size: 8.5pt; padding-top: 2px; }}
QLabel#fieldLabel {{ color: {TEXT_DIM}; padding-right: 4px; }}
QToolButton#ribbonLarge, QToolButton#ribbonSmall, QToolButton#ribbonText {{ background: transparent; color: {TEXT};
    border: 1px solid transparent; border-radius: 6px; }}
QToolButton#ribbonLarge {{ padding: 8px 6px 4px 6px; min-width: 52px; }}
QToolButton#ribbonSmall {{ padding: 5px; min-width: 22px; }}
QToolButton#ribbonText {{ padding: 5px 12px; }}
QToolButton#ribbonLarge:hover, QToolButton#ribbonSmall:hover, QToolButton#ribbonText:hover {{
    background: {PANEL_RAISED}; }}
QToolButton#ribbonLarge:checked, QToolButton#ribbonSmall:checked, QToolButton#ribbonText:checked {{
    background: {ACCENT_TINT}; border-color: {ACCENT_EDGE}; }}
QToolButton#ribbonLarge::menu-indicator {{ image: none; }}

QWidget#docStrip {{ background: {PANEL_DARK}; border-bottom: 1px solid {BORDER}; }}
QFrame#docTab {{ background: {CANVAS}; border: 1px solid {BORDER}; border-bottom: none; border-top: 2px solid {ACCENT};
    border-top-left-radius: 6px; border-top-right-radius: 6px; }}
QFrame#docTab QLabel {{ color: {TEXT}; background: transparent; }}
QToolButton#tabClose, QToolButton#tabNew {{ background: transparent; border: none; border-radius: 5px; padding: 3px; }}
QToolButton#tabClose:hover, QToolButton#tabNew:hover {{ background: {PANEL_RAISED}; }}

QFrame#loadingOverlay {{ background: rgba(10, 15, 30, 0.8); }}
QFrame#loadingCard {{ background: {PANEL_RAISED}; border: 1px solid {BORDER_STRONG}; border-radius: 10px; }}
QFrame#loadingCard QLabel {{ background: transparent; color: {TEXT_DIM}; }}
QLabel#loadingTitle {{ color: {TEXT}; font-weight: 600; font-size: 11pt; }}
QProgressBar {{ background: {CANVAS}; border: none; border-radius: 2px; }}
QProgressBar::chunk {{ background: {ACCENT}; border-radius: 2px; }}
QFrame#promptHint {{ background: rgba(14, 21, 39, 0.94); border: 1px solid {BORDER_STRONG}; border-radius: 6px; }}
QFrame#promptHint QLabel {{ background: transparent; color: {TEXT_DIM}; }}
QLabel#hintCommand {{ color: {TEXT}; font-weight: 600; }}
QFrame#navBar {{ background: rgba(14, 21, 39, 0.94); border: 1px solid {BORDER_STRONG}; border-radius: 8px; }}
QToolButton#navButton {{ background: transparent; border: none; border-radius: 6px; padding: 7px; }}
QToolButton#navButton:hover {{ background: {PANEL_RAISED}; }}

QDockWidget {{ color: {TEXT}; }}
QDockWidget::title {{ background: {PANEL_DARK}; padding: 6px 10px; border-bottom: 1px solid {BORDER}; }}
QTableWidget#layerTable {{ background: {CANVAS}; color: {TEXT}; border: 1px solid {BORDER};
    selection-background-color: {ACCENT_TINT}; selection-color: {TEXT}; gridline-color: {BORDER}; }}
QTableWidget#layerTable QHeaderView::section {{ background: {PANEL_DARK}; color: {TEXT_DIM}; border: none;
    padding: 4px 6px; }}
QComboBox {{ background: {CANVAS}; color: {TEXT}; border: 1px solid {BORDER_STRONG}; border-radius: 6px;
    padding: 5px 10px; }}
QComboBox:hover {{ border-color: {ACCENT}; }}
QComboBox QAbstractItemView {{ background: {PANEL_RAISED}; color: {TEXT}; border: 1px solid {BORDER_STRONG};
    selection-background-color: {ACCENT_TINT}; selection-color: {TEXT}; }}
QPushButton {{ background: {PANEL_RAISED}; color: {TEXT}; border: 1px solid {BORDER_STRONG}; border-radius: 6px;
    padding: 5px 12px; }}
QPushButton:hover {{ border-color: {ACCENT}; }}

QStatusBar {{ background: {PANEL}; color: {TEXT_DIM}; border-top: 1px solid {BORDER}; }}
QStatusBar::item {{ border: none; }}
QLabel#coords {{ color: {TEXT}; font-family: {_MONO_CSS}; font-weight: 600; padding: 0 14px 0 8px; }}
QLabel#statusInfo {{ color: {TEXT_DIM}; padding: 0 10px; }}
QPushButton#snapToggle {{ background: transparent; color: {TEXT_DIM}; border: none; border-radius: 6px;
    padding: 5px 10px; font-weight: 500; }}
QPushButton#snapToggle:hover {{ background: {PANEL_RAISED}; color: {TEXT}; }}
QPushButton#snapToggle:checked {{ background: {ACCENT_TINT}; color: {ACCENT}; }}

QMainWindow::separator {{ background: {BORDER}; width: 1px; height: 1px; }}
QWidget#commandPanel {{ background: {PANEL}; border-top: 1px solid {BORDER}; }}
QPlainTextEdit#history {{ background: transparent; border: none; color: {TEXT_DIM}; font-family: {_MONO_CSS}; }}
QFrame#commandBox {{ background: {PANEL_DARK}; border: 1px solid {BORDER_STRONG}; border-radius: 8px; }}
QLabel#prompt {{ color: {TEXT}; font-weight: 600; background: transparent; }}
QLabel#promptGlyph {{ color: {TEXT_DIM}; font-family: {_MONO_CSS}; background: transparent; }}
QLineEdit#commandInput {{ background: transparent; color: {TEXT}; border: none; padding: 7px 4px;
    font-family: {_MONO_CSS}; selection-background-color: {ACCENT}; selection-color: {ON_ACCENT}; }}
"""


def apply_theme(app: QApplication) -> None:
    app.setStyle(QStyleFactory.create("Fusion"))
    font = QFont()
    font.setFamilies(UI_FONTS)
    font.setPointSizeF(UI_POINT_SIZE)
    app.setFont(font)
    app.setPalette(_palette())
    app.setStyleSheet(STYLESHEET)


def _palette() -> QPalette:
    palette = QPalette()
    roles = {
        QPalette.ColorRole.Window: PANEL,
        QPalette.ColorRole.WindowText: TEXT,
        QPalette.ColorRole.Base: CANVAS,
        QPalette.ColorRole.AlternateBase: PANEL,
        QPalette.ColorRole.Text: TEXT,
        QPalette.ColorRole.Button: PANEL_RAISED,
        QPalette.ColorRole.ButtonText: TEXT,
        QPalette.ColorRole.Highlight: ACCENT,
        QPalette.ColorRole.HighlightedText: ON_ACCENT,
        QPalette.ColorRole.ToolTipBase: PANEL_RAISED,
        QPalette.ColorRole.ToolTipText: TEXT,
        QPalette.ColorRole.PlaceholderText: TEXT_DIM,
    }
    for role, color in roles.items():
        palette.setColor(role, QColor(color))
    for role in (QPalette.ColorRole.WindowText, QPalette.ColorRole.Text, QPalette.ColorRole.ButtonText):
        palette.setColor(QPalette.ColorGroup.Disabled, role, QColor(TEXT_DIM))
    return palette




