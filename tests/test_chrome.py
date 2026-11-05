import pytest
from helpers import open_and_wait
from PySide6.QtCore import QEvent, QPoint, QPointF, QRect, Qt
from PySide6.QtGui import QGuiApplication
from PySide6.QtWidgets import QApplication, QMenu, QMessageBox, QToolButton

from linewright.canvas.view import ZOOM_STEP
from linewright.commands.registry import DRAWING, EDITING
from linewright.model.entities import Line
from linewright.ui.frame_resize import edges_at
from linewright.ui.ribbon import MORE, PANELS




def type_text(window, text):
    window.command_line._input.setText(text)
    window.command_line._input.returnPressed.emit()


def button_for(window, command):
    for button in window.findChildren(QToolButton):
        action = button.defaultAction()
        if action is not None and action is window._tool_actions.get(command):
            return button
    raise AssertionError(f"no button for {command}")


def test_every_command_has_a_ribbon_button_and_each_is_listed_once(window):
    listed = [name for panel in PANELS for name in (*panel.large, *panel.small)]
    in_menus = [name for names in MORE.values() for name in names]
    assert sorted(listed + in_menus) == sorted((*DRAWING, *EDITING))
    for name in listed:
        assert button_for(window, name) is not None


def test_holding_the_dimension_button_offers_the_other_dimension_commands(window):
    menu = button_for(window, "DIMLINEAR").menu()
    assert [action.text() for action in menu.actions()] == ["Aligned dimension", "Dimension scale"]


def test_ribbon_buttons_start_their_commands_and_light_up(window):
    button = button_for(window, "CIRCLE")
    button.click()
    assert window.controller._session is not None
    assert button.isChecked()
    assert not button_for(window, "LINE").isChecked()


def test_large_buttons_show_their_name_under_the_icon_and_small_ones_beside_it(window):
    assert button_for(window, "LINE").objectName() == "ribbonLarge"
    assert button_for(window, "MOVE").objectName() == "ribbonSmall"
    assert button_for(window, "LINE").text() == "Line"


def test_the_document_tab_follows_the_drawing(window, tmp_path):
    assert window.doc_tab.title == "Drawing1"
    for text in ("rec", "0,0", "5,5"):
        type_text(window, text)
    assert window.doc_tab.title == "Drawing1*"
    window.save_path(tmp_path / "box.dxf")
    assert window.doc_tab.title == "box.dxf"
    type_text(window, "u")
    assert window.doc_tab.title == "box.dxf*"


def test_the_app_menu_holds_the_file_commands(window):
    menus = [b.menu() for b in window.findChildren(QToolButton) if b.objectName() == "appButton"]
    (menu,) = menus
    assert isinstance(menu, QMenu)
    assert [a.text() for a in menu.actions()] == ["New", "Open...", "Save", "Save As...", "Export PDF..."]


def test_quick_access_buttons_show_their_shortcuts_and_undo_follows_the_history(window):
    qat = {b.defaultAction().text(): b for b in window.findChildren(QToolButton) if b.objectName() == "qat"}
    assert list(qat) == ["New", "Open...", "Save", "Export PDF...", "Undo", "Redo"]
    assert "Ctrl+N" in qat["New"].toolTip()
    assert "Ctrl+Z" in qat["Undo"].toolTip()
    assert not qat["Undo"].isEnabled()
    for text in ("l", "0,0", "5,5", ""):
        type_text(window, text)
    assert qat["Undo"].isEnabled()
    qat["Undo"].click()
    assert window.document.entities == []
    assert qat["Redo"].isEnabled()
    qat["Redo"].click()
    assert window.document.entities == [Line((0, 0), (5, 5))]


def test_navigation_buttons_zoom(window):
    before = window.view.zoom_scale
    window.navigation.zoom_in.emit()
    assert window.view.zoom_scale == pytest.approx(before * ZOOM_STEP)
    window.navigation.zoom_out.emit()
    assert window.view.zoom_scale == pytest.approx(before)
    for text in ("rec", "0,0", "1000,500"):
        type_text(window, text)
    window.navigation.zoom_extents.emit()
    assert window.view.zoom_scale < before


def test_the_navigation_bar_stays_at_the_right_edge_when_the_window_changes_size(window, qtbot):
    for width, height in ((1100, 700), (900, 640)):
        window.resize(width, height)
        qtbot.wait(20)
        area = window.view.viewport().rect()
        nav = window.navigation.geometry()
        assert area.contains(nav)
        assert nav.right() > area.right() - 30
        assert abs(nav.center().y() - area.center().y()) <= 2


def test_the_grid_button_hides_and_shows_the_grid(window):
    button = window.snap_toggles._buttons["show_grid"]
    assert button.isChecked() and window.view._grid_visible
    button.click()
    assert not window.view._grid_visible
    button.click()
    assert window.view._grid_visible


def test_snap_and_grid_are_separate_switches(window):
    window.snap_toggles._buttons["grid"].click()
    assert window.snap_settings.grid and window.snap_settings.show_grid
    labels = [b.text() for b in window.snap_toggles._buttons.values()]
    assert labels == ["Grid", "Snap", "Ortho", "Polar", "Object snap"]


def test_the_pick_square_shows_only_when_objects_can_be_picked(window):
    assert window.view._pickbox
    window.controller.start_command("LINE")
    assert not window.view._pickbox
    window.controller.cancel()
    window.controller.start_command("ERASE")
    assert window.view._pickbox
    window.controller.cancel()
    assert window.view._pickbox


def test_the_crosshair_follows_the_pointer_and_goes_when_it_leaves(window):
    window.view._move_pointer(QPointF(40, 50))
    assert window.view._pointer == QPointF(40, 50)
    QApplication.sendEvent(window.view.viewport(), QEvent(QEvent.Type.Leave))
    assert window.view._pointer is None


def test_the_command_line_hints_at_what_to_type_only_when_idle(window):
    assert window.command_line._input.placeholderText() == "Type a command, e.g. line, circle, move"
    window.controller.start_command("LINE")
    assert window.command_line._input.placeholderText() == ""


def test_opening_a_drawing_with_content_linewright_cannot_show_says_so_in_the_command_history(window, qtbot, tmp_path):
    import ezdxf

    source = ezdxf.new()
    source.modelspace().add_ellipse((0, 0), major_axis=(5, 0), ratio=0.5)
    path = tmp_path / "ellipse.dxf"
    source.saveas(path)
    open_and_wait(window, qtbot, path)
    assert "1 ELLIPSE" in window.command_line._history.toPlainText()



def window_buttons(window):
    return [b for b in window.header.findChildren(QToolButton) if b.objectName() in ("windowButton", "windowClose")]


def test_the_window_has_no_system_frame_and_the_header_is_its_title_bar(window):
    assert window.windowFlags() & Qt.WindowType.FramelessWindowHint
    assert window.header.height() == 40


def test_the_title_bar_shows_the_drawing_name_and_unsaved_marker(window):
    assert window.header._title.text() == "Drawing1"
    for text in ("c", "0,0", "2"):
        type_text(window, text)
    assert window.header._title.text() == "Drawing1*"
    assert window.windowTitle() == "Drawing1* - Linewright"


def test_minimize_maximize_and_close_buttons_come_last_in_that_order(window):
    assert [b.toolTip() for b in window_buttons(window)] == ["Minimize", "Maximize", "Close"]
    assert window_buttons(window)[-1].objectName() == "windowClose"


def test_the_maximize_button_toggles_and_changes_its_icon_and_tip(window):
    maximize = window_buttons(window)[1]
    maximize.click()
    assert window.isMaximized()
    assert maximize.toolTip() == "Restore Down"
    maximize.click()
    assert not window.isMaximized()
    assert maximize.toolTip() == "Maximize"


def test_double_clicking_the_title_bar_toggles_maximize(window, qtbot):
    middle = QPoint(window.header.width() // 2, 20)
    qtbot.mouseDClick(window.header, Qt.MouseButton.LeftButton, pos=middle)
    assert window.isMaximized()
    qtbot.mouseDClick(window.header, Qt.MouseButton.LeftButton, pos=middle)
    assert not window.isMaximized()


def test_the_close_button_closes_a_clean_window(window):
    window_buttons(window)[-1].click()
    assert not window.isVisible()


def test_closing_with_unsaved_changes_asks_first_and_cancel_keeps_the_window(window, monkeypatch):
    for text in ("c", "0,0", "2"):
        type_text(window, text)
    monkeypatch.setattr(QMessageBox, "question", lambda *args: QMessageBox.StandardButton.Cancel)
    window_buttons(window)[-1].click()
    assert window.isVisible()


class TestResizeEdges:
    FRAME = QRect(100, 100, 800, 600)

    @pytest.mark.parametrize(
        "point, expected",
        [
            (QPoint(500, 400), Qt.Edge(0)),
            (QPoint(102, 400), Qt.Edge.LeftEdge),
            (QPoint(897, 400), Qt.Edge.RightEdge),
            (QPoint(500, 101), Qt.Edge.TopEdge),
            (QPoint(500, 698), Qt.Edge.BottomEdge),
            (QPoint(102, 102), Qt.Edge.LeftEdge | Qt.Edge.TopEdge),
            (QPoint(897, 698), Qt.Edge.RightEdge | Qt.Edge.BottomEdge),
            (QPoint(897, 102), Qt.Edge.RightEdge | Qt.Edge.TopEdge),
            (QPoint(102, 698), Qt.Edge.LeftEdge | Qt.Edge.BottomEdge),
            (QPoint(50, 400), Qt.Edge(0)),
            (QPoint(500, 800), Qt.Edge(0)),
        ],
    )
    def test_edges_at(self, point, expected):
        assert edges_at(point, self.FRAME) == expected

    def test_the_grab_width_is_adjustable(self):
        assert edges_at(QPoint(110, 400), self.FRAME, 12) == Qt.Edge.LeftEdge
        assert edges_at(QPoint(110, 400), self.FRAME, 6) == Qt.Edge(0)

    def test_the_resize_cursor_follows_the_edge_and_is_given_back(self, window):
        resizer = window._frame_resizer
        resizer._update_cursor(Qt.Edge.LeftEdge)
        assert QGuiApplication.overrideCursor().shape() == Qt.CursorShape.SizeHorCursor
        resizer._update_cursor(Qt.Edge.LeftEdge | Qt.Edge.TopEdge)
        assert QGuiApplication.overrideCursor().shape() == Qt.CursorShape.SizeFDiagCursor
        resizer._update_cursor(Qt.Edge(0))
        assert QGuiApplication.overrideCursor() is None

    def test_a_maximized_window_has_no_resize_edges(self, window):
        window.showMaximized()
        assert window._frame_resizer._edges_under(window.frameGeometry().topLeft() + QPoint(1, 1)) == Qt.Edge(0)





def test_the_running_command_and_its_question_show_on_the_canvas_until_it_ends(window):
    assert window.hint.isHidden()
    window.controller.start_command("LINE")
    assert not window.hint.isHidden()
    assert window.hint._command.text() == "Line"
    assert window.hint._message.text() == "\u00b7 Specify first point"
    window.controller.cancel()
    assert window.hint.isHidden()


def test_the_status_bar_names_the_current_layer_and_units(window):
    assert window._layer_info.text() == "Layer 0"
    assert window._units_info.text() == "Units mm"
    window.layer_service.create()
    window.layer_service.set_current("Layer1")
    assert window._layer_info.text() == "Layer Layer1"


def test_a_new_drawing_says_so_in_the_command_history(window):
    assert "Drawing1 created. Units: millimeters." in window.command_line._history.toPlainText()


def test_the_tab_buttons_start_a_new_drawing(window):
    for text in ("c", "0,0", "2"):
        type_text(window, text)
    window.undo_stack.setClean()
    window.doc_tab.new_requested.emit()
    assert window.document.entities == []
