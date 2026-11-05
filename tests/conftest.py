import pytest
from PySide6.QtCore import QEvent
from PySide6.QtWidgets import QApplication

from linewright.ui.main_window import MainWindow


@pytest.fixture
def window(qtbot):
    window = MainWindow()
    window.show()
    yield window
    window.undo_stack.setClean()  # closing an unsaved window would open a modal question
    window.close()
    window.deleteLater()
    QApplication.sendPostedEvents(None, QEvent.Type.DeferredDelete)
