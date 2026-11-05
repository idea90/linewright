from PySide6.QtCore import QRectF

from linewright.commands.session import CommandSession

EVERYWHERE = QRectF(-100_000, -100_000, 200_000, 200_000)


OPEN_TIMEOUT_MS = 20_000


def open_and_wait(window, qtbot, path):
    """Open a drawing the way the window does, in the background, and wait until that has finished."""
    window.open_in_background(path)
    qtbot.waitUntil(lambda: window._opening is None, timeout=OPEN_TIMEOUT_MS)
    return window.document.path == path


def drawn(window):
    """The entities the window currently draws."""
    return window.sync.entities_in(EVERYWHERE)


class FakeHost:
    """Records what a command asked the drawing to do."""

    def __init__(self):
        self.added, self.removed, self.modified = [], [], []
        self.events = []
        self.reports = []
        self.group_depth = 0
        self.groups_closed = 0

    def add(self, entities):
        self.added.extend(entities)
        self.events.append(("add", list(entities)))

    def add_new(self, entities):
        self.add(entities)

    def remove(self, entities):
        self.removed.extend(entities)
        self.events.append(("remove", list(entities)))

    def modify(self, changes):
        self.modified.extend(changes)
        self.events.append(("modify", list(changes)))

    def retract(self):
        if self.added:
            self.added.pop()
        self.events.append(("retract",))

    def report(self, message):
        self.reports.append(message)

    def begin_group(self):
        self.group_depth += 1
        self.events.append(("begin",))

    def end_group(self):
        self.group_depth -= 1
        self.groups_closed += 1
        self.events.append(("end",))


def new_session(command, *args):
    host = FakeHost()
    return host, CommandSession(command(*args), host)

