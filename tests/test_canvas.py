import pytest
from PySide6.QtCore import QPointF

from linewright.canvas.view import MAX_SCALE, MIN_GRID_PIXELS, MIN_SCALE, grid_spacing
from linewright.io.files import save_drawing
from linewright.model.document import Document
from linewright.model.entities import Line
from helpers import drawn, open_and_wait




@pytest.mark.parametrize("scale", [1e-3, 0.5, 1.0, 4.0, 250.0])
def test_grid_spacing_is_at_least_min_pixels_and_a_power_of_ten(scale):
    step = grid_spacing(scale)
    assert step * scale >= MIN_GRID_PIXELS
    assert step * scale < MIN_GRID_PIXELS * 10


def test_y_axis_points_up(window):
    below = window.view.mapFromScene(QPointF(0, 0))
    above = window.view.mapFromScene(QPointF(0, 10))
    assert above.y() < below.y()


def test_zoom_in_and_out_changes_scale(window):
    start = window.view.zoom_scale
    window.view.zoom_by(2.0)
    assert window.view.zoom_scale == pytest.approx(start * 2.0)
    window.view.zoom_by(0.5)
    assert window.view.zoom_scale == pytest.approx(start)


def test_zoom_is_clamped(window):
    window.view.zoom_by(1e30)
    assert window.view.zoom_scale == pytest.approx(MAX_SCALE)
    window.view.zoom_by(1e-30)
    assert window.view.zoom_scale == pytest.approx(MIN_SCALE)


def test_open_shows_the_drawing_and_zoom_extents_frames_it(window, qtbot, tmp_path):
    path = tmp_path / "plan.dxf"
    save_drawing(Document(entities=[Line((1000, 1000), (1100, 1050))]), path)
    assert open_and_wait(window, qtbot, path)
    assert len(drawn(window)) == 1
    assert window.windowTitle() == "plan.dxf - Linewright"
    visible = window.view.mapToScene(window.view.viewport().rect()).boundingRect()
    assert visible.contains(QPointF(1000, 1000))
    assert visible.contains(QPointF(1100, 1050))
    assert window.view.zoom_scale > 1.0


def test_open_failure_is_reported_not_raised(window, qtbot, tmp_path, monkeypatch):
    shown = []
    monkeypatch.setattr(window, "show_error", shown.append)
    assert not open_and_wait(window, qtbot, tmp_path / "missing.dxf")
    assert len(shown) == 1


def test_zoom_extents_keeps_y_up(window, qtbot, tmp_path):
    path = tmp_path / "plan.dxf"
    save_drawing(Document(entities=[Line((0, 0), (10, 10))]), path)
    open_and_wait(window, qtbot, path)
    assert window.view.transform().m22() < 0


def test_command_line_emits_trimmed_text(window, qtbot):
    with qtbot.waitSignal(window.command_line.submitted) as blocker:
        window.command_line._input.setText("  LINE ")
        window.command_line._input.returnPressed.emit()
    assert blocker.args == ["LINE"]

