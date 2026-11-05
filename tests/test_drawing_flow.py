import pytest
from PySide6.QtCore import QPointF, Qt
from PySide6.QtGui import QGuiApplication
from PySide6.QtWidgets import QMessageBox

from linewright import theme
from linewright.commands import edit
from linewright.model.document import Document
from linewright.model.entities import Circle, Line
from helpers import drawn




def type_line(window, text):
    window.command_line._input.setText(text)
    window.command_line._input.returnPressed.emit()


def test_typing_line_and_points_draws_and_undo_redo_work(window):
    for text in ("line", "0,0", "10,0", "@0,5", ""):
        type_line(window, text)
    assert window.document.entities == [Line((0, 0), (10, 0)), Line((10, 0), (10, 5))]
    assert len(drawn(window)) == 2

    type_line(window, "u")
    assert window.document.entities == [Line((0, 0), (10, 0))]
    assert len(drawn(window)) == 1
    type_line(window, "redo")
    assert len(window.document.entities) == 2


def test_clicks_answer_prompts_and_the_preview_follows_the_cursor(window):
    window.controller.start_command("CIRCLE")
    window.controller.click(QPointF(5, 5))
    window.controller.move_cursor(QPointF(8, 9))
    assert len(window.sync.previewing) == 1  # the rubber-band circle
    assert drawn(window) == []
    window.controller.click(QPointF(8, 9))
    assert window.document.entities == [Circle((5, 5), 5.0)]
    assert len(drawn(window)) == 1
    assert window.sync.previewing == []


def test_escape_cancels_and_removes_the_preview(window):
    window.controller.start_command("LINE")
    window.controller.click(QPointF(0, 0))
    window.controller.move_cursor(QPointF(4, 4))
    assert len(window.sync.previewing) == 1
    window.controller.cancel()
    assert window.sync.previewing == []
    assert drawn(window) == []
    assert window.document.entities == []
    assert window.command_line.prompt == "Command:"


def test_enter_on_an_empty_prompt_repeats_the_last_command(window):
    for text in ("c", "0,0", "3"):
        type_line(window, text)
    type_line(window, "")
    assert window.command_line.prompt == "Specify center point for circle or [3P/2P]:"


def test_bad_input_prints_a_message_and_keeps_the_prompt(window):
    type_line(window, "l")
    type_line(window, "banana")
    assert "Enter a point" in window.command_line._history.toPlainText()
    assert window.command_line.prompt == "Specify first point:"
    type_line(window, "frobnicate")  # still inside LINE: treated as a bad point
    assert window.controller._session is not None


def test_unknown_command_is_reported(window):
    type_line(window, "frobnicate")
    assert "Unknown command: FROBNICATE" in window.command_line._history.toPlainText()


def test_drawing_marks_the_title_unsaved_and_saving_clears_it(window, tmp_path):
    assert window.windowTitle() == "Drawing1 - Linewright"
    for text in ("rec", "0,0", "5,5"):
        type_line(window, text)
    assert window.windowTitle() == "Drawing1* - Linewright"
    assert window.save_path(tmp_path / "box.dxf")
    assert window.windowTitle() == "box.dxf - Linewright"
    type_line(window, "u")
    assert window.windowTitle() == "box.dxf* - Linewright"


def test_opening_a_drawing_while_unsaved_asks_first(window, monkeypatch):
    for text in ("c", "0,0", "1"):
        type_line(window, text)
    asked = []

    def decline(*args):
        asked.append(args)
        return QMessageBox.StandardButton.Cancel

    monkeypatch.setattr(QMessageBox, "question", decline)
    assert not window._confirm_discard()
    assert len(asked) == 1


def test_discarding_unsaved_changes_is_allowed(window, monkeypatch):
    for text in ("c", "0,0", "1"):
        type_line(window, text)
    monkeypatch.setattr(QMessageBox, "question", lambda *args: QMessageBox.StandardButton.Discard)
    assert window._confirm_discard()


def test_new_document_clears_the_undo_history(window):
    for text in ("c", "0,0", "1"):
        type_line(window, text)
    window.set_document(Document())
    assert not window.undo_stack.canUndo()
    assert window.windowTitle() == "Drawing1 - Linewright"


def test_clicks_snap_to_an_endpoint_of_existing_geometry(window):
    for text in ("l", "0,0", "10,0", ""):
        type_line(window, text)
    window.controller.start_command("CIRCLE")
    window.controller.click(QPointF(0.5, 0.4))
    window.controller.click(QPointF(5, 0))
    assert window.document.entities[-1] == Circle((0, 0), 5.0)


def test_cursor_over_an_endpoint_shows_a_marker_and_the_snapped_coordinates(window):
    for text in ("l", "0,0", "10,0", ""):
        type_line(window, text)
    window.controller.move_cursor(QPointF(10.4, 0.3))
    assert window.view._marker is None  # nothing to snap to while no command is asking for a point
    window.controller.start_command("LINE")
    assert window.view._marker == (QPointF(10, 0), "endpoint")
    assert window._coords.text() == "10.0000, 0.0000"
    window.controller.move_cursor(QPointF(40, 40))
    assert window.view._marker is None


def test_ortho_constrains_clicks_to_the_base_point(window):
    window.snap_settings.set("ortho", True)
    window.controller.start_command("LINE")
    window.controller.click(QPointF(0, 0))
    window.controller.click(QPointF(10, 3))
    assert window.document.entities == [Line((0, 0), (10, 0))]


def test_snap_buttons_drive_the_settings_and_ortho_excludes_polar(window):
    buttons = window.snap_toggles._buttons
    buttons["ortho"].click()
    assert window.snap_settings.ortho and buttons["ortho"].isChecked()
    buttons["polar"].click()
    assert window.snap_settings.polar and not window.snap_settings.ortho
    assert not buttons["ortho"].isChecked()
    buttons["osnap"].click()
    assert not window.snap_settings.osnap


def test_toggling_a_snap_mode_updates_the_rubber_band_at_once(window):
    window.snap_settings.set("osnap", False)
    window.controller.start_command("LINE")
    window.controller.click(QPointF(0, 0))
    window.controller.move_cursor(QPointF(10, 3))
    window.snap_toggles._buttons["ortho"].click()
    (preview,) = window.sync.previewing
    assert preview.end == (10, 0)



def test_u_inside_line_takes_back_the_last_segment_from_the_drawing(window):
    for text in ("l", "0,0", "10,0", "10,10"):
        type_line(window, text)
    assert len(window.document.entities) == 2
    type_line(window, "u")
    assert window.document.entities == [Line((0, 0), (10, 0))]
    assert len(drawn(window)) == 1
    assert window.command_line.prompt == "Specify next point or [Undo]:"
    type_line(window, "20,0")
    type_line(window, "")
    assert window.document.entities == [Line((0, 0), (10, 0)), Line((10, 0), (20, 0))]
    type_line(window, "u")
    assert len(window.document.entities) == 1  # the whole last segment is one undo step



def draw_two_lines(window):
    for text in ("l", "0,0", "10,0", "", "l", "0,5", "10,5", ""):
        type_line(window, text)
    return window.document.entities


def click(window, x, y):
    window.controller.click(QPointF(x, y))
    window.controller.release(QPointF(x, y))


def drag(window, start, end):
    window.controller.click(QPointF(*start))
    window.controller.move_cursor(QPointF(*end))
    window.controller.release(QPointF(*end))


def selected(window):
    return list(window.controller._selection)


def test_clicking_an_entity_selects_it_and_highlights_it(window):
    lower, upper = draw_two_lines(window)
    click(window, 5, 0.3)
    assert selected(window) == [lower]
    assert window.sync.style_of(lower).color.name() == theme.ACCENT
    assert window.sync.style_of(upper).color.name() != theme.ACCENT


def test_clicking_empty_space_clears_the_selection(window):
    draw_two_lines(window)
    click(window, 5, 0)
    click(window, 50, 50)
    assert selected(window) == []


def test_a_window_drag_selects_what_is_entirely_inside(window):
    lower, upper = draw_two_lines(window)
    drag(window, (-1, -1), (11, 1))
    assert selected(window) == [lower]


def test_a_crossing_drag_right_to_left_selects_what_it_touches(window):
    lower, upper = draw_two_lines(window)
    drag(window, (5, 6), (4, -1))
    assert selected(window) == [lower, upper]


def test_shift_click_toggles_an_entity(window, monkeypatch):
    lower, upper = draw_two_lines(window)
    click(window, 5, 0)
    monkeypatch.setattr(QGuiApplication, "keyboardModifiers", lambda: Qt.KeyboardModifier.ShiftModifier)
    click(window, 5, 5)
    assert selected(window) == [lower, upper]
    click(window, 5, 0)
    assert selected(window) == [upper]


def test_escape_clears_the_selection_when_idle(window):
    draw_two_lines(window)
    click(window, 5, 0)
    window.controller.cancel()
    assert selected(window) == []


def test_select_then_move_does_not_ask_for_objects(window):
    lower, _ = draw_two_lines(window)
    click(window, 5, 0)
    type_line(window, "m")
    assert window.command_line.prompt == "Specify base point:"
    type_line(window, "0,0")
    type_line(window, "@3,4")
    assert lower == Line((3, 4), (13, 4))
    assert selected(window) == []
    assert window.sync.selected == []


def test_move_undo_and_redo_restore_the_drawing_and_its_items(window):
    lower, _ = draw_two_lines(window)
    click(window, 5, 0)
    for text in ("m", "0,0", "@3,4"):
        type_line(window, text)
    type_line(window, "u")
    assert lower == Line((0, 0), (10, 0))
    type_line(window, "redo")
    assert lower == Line((3, 4), (13, 4))
    assert len(drawn(window)) == 2


def test_verb_then_noun_with_a_click_and_enter(window):
    lower, upper = draw_two_lines(window)
    type_line(window, "e")
    assert window.command_line.prompt == "Select objects:"
    click(window, 5, 0)
    click(window, 5, 5)
    assert "2 total" in window.command_line._history.toPlainText()
    type_line(window, "")
    assert window.document.entities == []
    type_line(window, "u")
    assert window.document.entities == [lower, upper]
    assert window.document.entities[0] is lower


def test_erase_with_nothing_chosen_and_enter_does_nothing(window):
    draw_two_lines(window)
    type_line(window, "e")
    type_line(window, "")
    assert len(window.document.entities) == 2
    assert window.controller._session is None


def test_all_selects_everything_at_a_select_prompt(window):
    draw_two_lines(window)
    type_line(window, "e")
    type_line(window, "all")
    assert window.command_line.prompt == "Select objects:"
    type_line(window, "")
    assert window.document.entities == []


def test_typing_all_when_idle_selects_everything_for_the_next_command(window):
    draw_two_lines(window)
    type_line(window, "all")
    assert len(selected(window)) == 2
    type_line(window, "m")
    for text in ("0,0", "0,10"):
        type_line(window, text)
    assert [e.start for e in window.document.entities] == [(0, 10), (0, 15)]


def test_the_delete_key_erases_the_selection_and_undo_puts_it_back_in_order(window):
    lower, upper = draw_two_lines(window)
    click(window, 5, 0)
    window.command_line.delete_pressed.emit()
    assert window.document.entities == [upper]
    type_line(window, "u")
    assert window.document.entities[0] is lower
    assert window.document.entities[1] is upper


def test_delete_with_nothing_selected_does_nothing(window):
    draw_two_lines(window)
    window.command_line.delete_pressed.emit()
    assert len(window.document.entities) == 2 and window.controller._session is None


def test_undoing_a_drawing_removes_it_from_the_selection(window):
    draw_two_lines(window)
    click(window, 5, 5)
    assert len(selected(window)) == 1
    type_line(window, "u")
    assert selected(window) == []
    assert window.sync.selected == []


def test_copy_keeps_the_originals_and_adds_new_entities(window):
    draw_two_lines(window)
    click(window, 5, 0)
    for text in ("co", "0,0", "0,10", ""):
        type_line(window, text)
    assert len(window.document.entities) == 3
    assert window.document.entities[-1] == Line((0, 10), (10, 10))


def test_offset_picks_with_clicks_and_places_on_the_clicked_side(window):
    lower, _ = draw_two_lines(window)
    type_line(window, "o")
    type_line(window, "2")
    click(window, 5, 0.2)
    window.controller.click(QPointF(5, -3.6))  # clear of the midpoint snap range
    assert window.document.entities[-1] == Line((0, -2), (10, -2))
    assert window.command_line.prompt == "Select object to offset or <exit>:"
    window.controller.cancel()


def test_rotate_by_clicking_the_angle(window):
    lower, _ = draw_two_lines(window)
    click(window, 5, 0)
    type_line(window, "ro")
    type_line(window, "0,0")
    window.controller.click(QPointF(0, 7))
    assert lower.end == pytest.approx((0, 10), abs=1e-9)


def test_drawing_commands_ignore_the_selection_but_it_stays_until_they_finish(window):
    lower, _ = draw_two_lines(window)
    click(window, 5, 0)
    window.controller.start_command("LINE")
    assert selected(window) == [lower]
    window.controller.cancel()
    assert selected(window) == []


def test_clicks_in_a_text_prompt_do_not_start_a_selection(window):
    draw_two_lines(window)
    window.controller.start_command("TEXT")
    window.controller.click(QPointF(1, 1))
    window.controller.start_command("TEXT")
    for text in ("0,0", "", ""):
        type_line(window, text)
    click(window, 5, 0)
    assert selected(window) == []





def draw_cross(window):
    for text in ("l", "0,0", "10,0", "", "l", "5,-5", "5,5", ""):
        type_line(window, text)
    return window.document.entities


def test_trim_cuts_the_clicked_side_and_one_undo_puts_it_back(window):
    horizontal, vertical = draw_cross(window)
    type_line(window, "tr")
    type_line(window, "")  # Enter chooses every object as a cutting edge
    click(window, 8, 0.3)
    assert window.document.entities == [vertical, Line((0, 0), (5, 0))]
    window.controller.cancel()
    window.undo_stack.undo()
    assert window.document.entities == [horizontal, vertical]
    assert window.document.entities[0] is horizontal
    assert len(drawn(window)) == 2


def test_u_inside_trim_undoes_the_whole_trim_not_half_of_it(window):
    horizontal, vertical = draw_cross(window)
    type_line(window, "tr")
    type_line(window, "")
    click(window, 8, 0.3)
    type_line(window, "u")
    assert window.document.entities == [horizontal, vertical]
    click(window, 2, 0.3)
    assert window.document.entities[-1] == Line((5, 0), (10, 0))


def test_trim_leaves_the_pieces_in_the_drawing_and_keeps_them_as_cutting_edges(window):
    horizontal, vertical = draw_cross(window)
    type_line(window, "tr")
    type_line(window, "")
    click(window, 8, 0.3)
    click(window, 5, -4)
    assert window.document.entities[-1] == Line((5, 0), (5, 5))


def test_selected_objects_become_the_cutting_edges_without_a_prompt(window):
    horizontal, vertical = draw_cross(window)
    type_line(window, "all")
    type_line(window, "tr")
    assert window.command_line.prompt == edit.TRIM_PROMPT


def test_shift_click_in_trim_extends_instead(window, monkeypatch):
    for text in ("l", "0,0", "5,0", "", "l", "10,-5", "10,5", ""):
        type_line(window, text)
    short, edge = window.document.entities
    type_line(window, "tr")
    type_line(window, "")
    monkeypatch.setattr(QGuiApplication, "keyboardModifiers", lambda: Qt.KeyboardModifier.ShiftModifier)
    click(window, 4, 0.3)
    assert short == Line((0, 0), (10, 0))
    window.undo_stack.undo()
    assert short == Line((0, 0), (5, 0))


def test_extend_runs_a_line_on_to_the_boundary_and_undoes(window):
    for text in ("l", "0,0", "5,0", "", "l", "10,-5", "10,5", ""):
        type_line(window, text)
    short, edge = window.document.entities
    type_line(window, "ex")
    type_line(window, "")
    click(window, 4, 0.3)
    assert short == Line((0, 0), (10, 0))
    type_line(window, "u")
    assert short == Line((0, 0), (5, 0))


def test_fillet_rounds_the_corner_in_one_undo_step(window):
    for text in ("l", "0,0", "10,0", "", "l", "0,0", "0,10", ""):
        type_line(window, text)
    first, second = window.document.entities
    type_line(window, "f")
    type_line(window, "r")
    type_line(window, "2")
    click(window, 8, 0.2)
    click(window, 0.2, 8)
    assert len(window.document.entities) == 3
    arc = window.document.entities[-1]
    assert arc.center == pytest.approx((2, 2))
    assert first.start == pytest.approx((2, 0))
    window.undo_stack.undo()
    assert window.document.entities == [first, second]
    assert first == Line((0, 0), (10, 0)) and second == Line((0, 0), (0, 10))


def test_the_fillet_radius_is_remembered_for_the_next_fillet(window):
    for text in ("l", "0,0", "10,0", "", "l", "0,0", "0,10", ""):
        type_line(window, text)
    for text in ("f", "r", "3"):
        type_line(window, text)
    window.controller.cancel()
    type_line(window, "f")
    assert "radius 3" in window.command_line.prompt


def test_a_fillet_problem_is_shown_in_the_prompt(window):
    for text in ("l", "0,0", "10,0", "", "l", "0,3", "10,3", ""):
        type_line(window, text)
    type_line(window, "f")
    click(window, 5, 0.2)
    click(window, 5, 3.2)
    assert "parallel" in window.command_line.prompt
    assert len(window.document.entities) == 2


def test_trim_extend_and_fillet_are_on_the_ribbon_and_start_from_their_buttons(window):
    for name in ("TRIM", "EXTEND", "FILLET"):
        window._tool_actions[name].trigger()
        assert window.controller._session is not None
        window.controller.cancel()


