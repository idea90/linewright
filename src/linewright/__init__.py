import sys

from PySide6.QtWidgets import QApplication

from linewright.theme import apply_theme
from linewright.ui.main_window import MainWindow


def main() -> None:
    app = QApplication(sys.argv)
    apply_theme(app)
    window = MainWindow()
    window.show()
    sys.exit(app.exec())
