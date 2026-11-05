import threading

import pytest

from helpers import open_and_wait
from linewright.io.files import save_drawing
from linewright.model.document import Document
from linewright.model.entities import Line
from linewright.ui import open_job


@pytest.fixture
def plan(tmp_path):
    path = tmp_path / "plan.dxf"
    save_drawing(Document(entities=[Line((0, 0), (10, 0))]), path)
    return path


@pytest.fixture
def slow_open(monkeypatch):
    """Make the file read wait until the test lets it finish, to look at the window while it is busy."""
    release = threading.Event()
    real = open_job.open_drawing

    def waiting(path):
        release.wait(timeout=10)
        return real(path)

    monkeypatch.setattr(open_job, "open_drawing", waiting)
    return release


def test_the_drawing_arrives_and_replaces_the_current_one(window, qtbot, plan):
    assert open_and_wait(window, qtbot, plan)
    assert window.document.entities == [Line((0, 0), (10, 0))]
    assert window.loading.isHidden()


def test_while_a_file_loads_the_window_shows_progress_and_holds_still(window, qtbot, plan, slow_open):
    window.open_in_background(plan)
    assert not window.loading.isHidden()
    assert "plan.dxf" in window.loading._title.text()
    assert not window.command_line.isEnabled()
    assert not window.ribbon.isEnabled()
    assert all(not action.isEnabled() for action in window._file_actions)
    slow_open.set()
    qtbot.waitUntil(lambda: window._opening is None, timeout=10_000)
    assert window.loading.isHidden()
    assert window.command_line.isEnabled()
    assert window.ribbon.isEnabled()
    assert all(action.isEnabled() for action in window._file_actions)


def test_a_second_open_or_a_new_drawing_is_ignored_while_one_is_loading(window, qtbot, plan, slow_open):
    window.open_in_background(plan)
    first = window._opening
    window.open_in_background(plan)
    window._new()
    assert window._opening is first
    slow_open.set()
    qtbot.waitUntil(lambda: window._opening is None, timeout=10_000)
    assert window.document.path == plan


def test_a_file_that_cannot_be_read_is_reported_and_the_window_comes_back(window, qtbot, tmp_path, monkeypatch):
    shown = []
    monkeypatch.setattr(window, "show_error", shown.append)
    broken = tmp_path / "broken.dxf"
    broken.write_text("not a drawing")
    assert not open_and_wait(window, qtbot, broken)
    assert len(shown) == 1
    assert window.command_line.isEnabled()
    assert window.loading.isHidden()


def test_a_crash_inside_the_reader_still_reaches_the_user(window, qtbot, plan, monkeypatch):
    def explode(path):
        raise RuntimeError("boom")

    shown = []
    monkeypatch.setattr(open_job, "open_drawing", explode)
    monkeypatch.setattr(window, "show_error", shown.append)
    open_and_wait(window, qtbot, plan)
    assert shown == ["Could not open plan.dxf: boom"]
    assert window.command_line.isEnabled()
