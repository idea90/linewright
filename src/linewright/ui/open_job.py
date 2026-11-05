import threading
from pathlib import Path

from PySide6.QtCore import QObject, Signal

from linewright.io.files import FileError, open_drawing


class OpenJob(QObject):
    """Reads a drawing on another thread, so the window stays alive while a big file loads.

    Only the file is read there. The Document it produces is handed to the window's thread through a signal.
    """

    opened = Signal(object)  # the Document
    failed = Signal(str)  # a message for the user

    def __init__(self, path: Path) -> None:
        super().__init__()
        self._path = path

    def start(self) -> None:
        threading.Thread(target=self._run, daemon=True).start()

    def _run(self) -> None:
        try:
            document = open_drawing(self._path)
        except FileError as error:
            self.failed.emit(str(error))
        except Exception as error:  # noqa: BLE001 - whatever goes wrong here must reach the user, not vanish with the thread
            self.failed.emit(f"Could not open {self._path.name}: {error}")
        else:
            self.opened.emit(document)
