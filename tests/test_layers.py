import pytest
from PySide6.QtCore import QPointF, Qt
from PySide6.QtWidgets import QComboBox

from helpers import drawn, open_and_wait
from linewright.model.entities import Circle, Line
from linewright.model.styles import DASH_PATTERNS
from linewright.ui import layer_palette as lp


def type_text(window, text):
    window.command_line._input.setText(text)
    window.command_line._input.returnPressed.emit()


def draw_line(window, start="0,0", end="10,0"):
    for text in ("l", start, end, ""):
        type_text(window, text)
    return window.document.entities[-1]


def click(window, x, y):
    window.controller.click(QPointF(x, y))
    window.controller.release(QPointF(x, y))


def style_of(window, entity):
    return window.sync.style_of(entity)


def is_drawn(window, entity):
    return any(candidate is entity for candidate in drawn(window))


class TestLayerOperations:
    def test_new_layers_are_numbered_and_undoable(self, window):
        service = window.layer_service
        assert service.create() == "Layer1"
        assert service.create() == "Layer2"
        assert list(window.document.layers) == ["0", "Layer1", "Layer2"]
        window.undo_stack.undo()
        assert list(window.document.layers) == ["0", "Layer1"]

    def test_renaming_moves_the_objects_and_the_current_layer_along(self, window):
        service = window.layer_service
        name = service.create()
        service.set_current(name)
        line = draw_line(window)
        assert line.layer == "Layer1"
        assert service.rename("Layer1", "Walls") is None
        assert (line.layer, window.document.current_layer) == ("Walls", "Walls")
        assert list(window.document.layers) == ["0", "Walls"]
        window.undo_stack.undo()
        assert (line.layer, window.document.current_layer) == ("Layer1", "Layer1")
        assert "Layer1" in window.document.layers

    @pytest.mark.parametrize(
        "old, new, message",
        [
            ("0", "Zero", "Layer 0 cannot be renamed."),
            ("Layer1", "  ", "A layer needs a name."),
            ("Layer1", "LAYER2", "already exists"),
        ],
    )
    def test_renames_that_make_no_sense_are_refused(self, window, old, new, message):
        window.layer_service.create()
        window.layer_service.create()
        assert message in window.layer_service.rename(old, new)
        assert list(window.document.layers) == ["0", "Layer1", "Layer2"]

    def test_renaming_to_the_same_name_does_nothing(self, window):
        window.layer_service.create()
        stack_size = window.undo_stack.count()
        assert window.layer_service.rename("Layer1", "Layer1") is None
        assert window.undo_stack.count() == stack_size

    def test_deleting_an_unused_layer_and_undoing_it(self, window):
        service = window.layer_service
        service.create()
        service.create()
        assert service.delete("Layer1") is None
        assert list(window.document.layers) == ["0", "Layer2"]
        window.undo_stack.undo()
        assert list(window.document.layers) == ["0", "Layer1", "Layer2"]

    def test_deleting_is_refused_for_layer_zero_the_current_layer_and_layers_in_use(self, window):
        service = window.layer_service
        service.create()
        service.create()
        assert "Layer 0" in service.delete("0")
        service.set_current("Layer1")
        assert "current layer" in service.delete("Layer1")
        draw_line(window)
        service.set_current("Layer2")
        assert "has objects" in service.delete("Layer1")
        assert "Layer1" in window.document.layers

    def test_colour_and_linetype_change_the_look_of_its_objects(self, window):
        service = window.layer_service
        service.set_current(service.create())
        line = draw_line(window)
        service.set_color("Layer1", 1)
        assert style_of(window, line).color.name() == "#ff0000"
        service.set_linetype("Layer1", "Dashed")
        assert style_of(window, line).dashes == DASH_PATTERNS["Dashed"]
        window.undo_stack.undo()
        assert style_of(window, line).dashes == ()
        window.undo_stack.undo()
        assert style_of(window, line).color.name() != "#ff0000"

    def test_an_object_colour_beats_its_layers_and_by_layer_follows_it(self, window):
        service = window.layer_service
        service.set_current(service.create())
        service.set_color("Layer1", 1)
        plain = draw_line(window)
        own = draw_line(window, "0,1", "10,1")
        window.controller._selection.replace([own])
        window.controller.change_selected(color=3)
        window.controller.cancel()  # a selected object is drawn in the highlight colour
        assert style_of(window, plain).color.name() == "#ff0000"
        assert style_of(window, own).color.name() == "#00ff00"

    def test_an_object_linetype_beats_its_layers(self, window):
        service = window.layer_service
        service.set_current(service.create())
        service.set_linetype("Layer1", "Dashed")
        line = draw_line(window)
        assert style_of(window, line).dashes == DASH_PATTERNS["Dashed"]
        window.controller._selection.replace([line])
        window.controller.change_selected(linetype="Continuous")
        assert style_of(window, line).dashes == ()
        window.undo_stack.undo()
        assert line.linetype == "ByLayer"

    def test_unknown_linetypes_are_drawn_solid(self, window):
        line = draw_line(window)
        window.controller._selection.replace([line])
        window.controller.change_selected(linetype="Whatever")
        assert style_of(window, line).dashes == ()

    def test_the_current_layer_must_exist_and_be_on(self, window):
        service = window.layer_service
        service.create()
        assert "no layer" in service.set_current("Nope")
        service.set_visible("Layer1", False)
        assert "turned off" in service.set_current("Layer1")
        assert window.document.current_layer == "0"

    def test_the_current_layer_cannot_be_turned_off(self, window):
        assert "current layer" in window.layer_service.set_visible("0", False)
        assert window.document.layers["0"].visible

    def test_every_change_to_a_layer_is_one_undo_step(self, window):
        service = window.layer_service
        service.create()
        service.set_color("Layer1", 2)
        service.set_locked("Layer1", True)
        service.set_visible("Layer1", False)
        for _ in range(3):
            window.undo_stack.undo()
        layer = window.document.layers["Layer1"]
        assert (layer.color, layer.locked, layer.visible) == (7, False, True)

    def test_setting_what_is_already_set_does_nothing(self, window):
        window.layer_service.create()
        count = window.undo_stack.count()
        window.layer_service.set_color("Layer1", 7)
        window.layer_service.set_locked("Layer1", False)
        assert window.undo_stack.count() == count


class TestNewObjects:
    def test_new_drawings_use_the_current_layer_colour_and_linetype(self, window):
        service = window.layer_service
        service.set_current(service.create())
        window.document.current_color = 5
        window.document.current_linetype = "Center"
        line = draw_line(window)
        assert (line.layer, line.color, line.linetype) == ("Layer1", 5, "Center")

    def test_copies_keep_the_layer_of_their_source(self, window):
        service = window.layer_service
        service.set_current(service.create())
        line = draw_line(window)
        service.set_current("0")
        window.controller._selection.replace([line])
        for text in ("co", "0,0", "0,5", ""):
            type_text(window, text)
        assert window.document.entities[-1].layer == "Layer1"

    def test_offset_and_fillet_keep_the_source_layer(self, window):
        service = window.layer_service
        service.set_current(service.create())
        draw_line(window)
        service.set_current("0")
        for text in ("o", "2"):
            type_text(window, text)
        click(window, 5, 0.2)
        window.controller.click(QPointF(5, 5))
        assert window.document.entities[-1].layer == "Layer1"


class TestHiddenAndLocked:
    def test_a_hidden_layer_is_not_drawn_snapped_to_or_picked(self, window):
        line = draw_line(window)
        window.layer_service.create()
        window.layer_service.set_current("Layer1")
        window.layer_service.set_visible("0", False)
        assert not is_drawn(window, line)
        window.controller.start_command("LINE")
        window.controller.move_cursor(QPointF(10.2, 0.2))
        assert window.view._marker is None
        window.controller.cancel()
        click(window, 5, 0)
        assert window.controller.selected() == []

    def test_turning_the_layer_back_on_brings_it_back(self, window):
        line = draw_line(window)
        window.layer_service.set_current(window.layer_service.create())
        window.layer_service.set_visible("0", False)
        window.undo_stack.undo()
        assert is_drawn(window, line)
        assert window.controller._document.is_visible(line)

    def test_a_locked_layer_cannot_be_picked_or_boxed_but_can_still_be_seen_and_snapped_to(self, window):
        line = draw_line(window)
        window.layer_service.set_locked("0", True)
        assert is_drawn(window, line)
        click(window, 5, 0)
        assert window.controller.selected() == []
        window.controller.click(QPointF(-1, -1))
        window.controller.move_cursor(QPointF(8, 3))
        window.controller.release(QPointF(8, 3))
        assert window.controller.selected() == []
        window.controller.start_command("LINE")
        window.controller.move_cursor(QPointF(10.2, 0.2))
        assert window.view._marker is not None

    def test_all_skips_locked_and_hidden_objects(self, window):
        locked = draw_line(window)
        window.layer_service.set_current(window.layer_service.create())
        free = draw_line(window, "0,5", "10,5")
        window.layer_service.set_locked("0", True)
        type_text(window, "all")
        assert window.controller.selected() == [free]
        assert locked not in window.controller.selected()

    def test_locking_a_layer_drops_its_objects_from_the_selection(self, window):
        line = draw_line(window)
        window.layer_service.set_current(window.layer_service.create())
        click(window, 5, 0)
        assert window.controller.selected() == [line]
        window.layer_service.set_locked("0", True)
        assert window.controller.selected() == []
        assert window.sync.selected == []

    def test_trim_still_cuts_against_a_locked_layer(self, window):
        edge = draw_line(window, "5,-5", "5,5")
        window.layer_service.set_current(window.layer_service.create())
        target = draw_line(window, "0,0", "10,0")
        window.layer_service.set_locked("0", True)
        type_text(window, "tr")
        type_text(window, "")
        click(window, 8, 0.2)
        assert window.document.entities[-1] == Line((0, 0), (5, 0), layer="Layer1")
        assert edge in window.document.entities and target not in window.document.entities

    def test_locked_objects_are_not_offered_to_edit_commands(self, window):
        draw_line(window)
        window.layer_service.set_locked("0", True)
        type_text(window, "e")
        click(window, 5, 0)
        assert "found" not in window.command_line._history.toPlainText().split("Command: ERASE")[-1]


class TestPropertiesPanels:
    def test_the_layer_list_shows_the_current_layer_when_nothing_is_selected(self, window):
        window.layer_service.create()
        window.layer_service.set_current("Layer1")
        combo = window.layer_panel._combo
        assert combo.currentText() == "Layer1"
        assert [combo.itemText(i) for i in range(combo.count())] == ["0", "Layer1"]

    def test_choosing_a_layer_with_nothing_selected_makes_it_current(self, window):
        window.layer_service.create()
        combo = window.layer_panel._combo
        combo.setCurrentIndex(1)
        combo.activated.emit(1)
        assert window.document.current_layer == "Layer1"

    def test_choosing_a_layer_with_objects_selected_moves_them(self, window):
        line = draw_line(window)
        window.layer_service.create()
        click(window, 5, 0)
        combo = window.layer_panel._combo
        assert combo.currentText() == "0"
        combo.activated.emit(1)
        assert line.layer == "Layer1"
        assert window.document.current_layer == "0"
        window.undo_stack.undo()
        assert line.layer == "0"

    def test_the_layer_list_says_varies_when_the_selection_spans_layers(self, window):
        draw_line(window)
        window.layer_service.set_current(window.layer_service.create())
        draw_line(window, "0,5", "10,5")
        type_text(window, "all")
        combo = window.layer_panel._combo
        assert combo.currentText() == "*VARIES*"
        combo.activated.emit(combo.findText("*VARIES*"))
        assert {e.layer for e in window.document.entities} == {"0", "Layer1"}

    def test_a_layer_that_cannot_be_current_is_explained(self, window):
        window.layer_service.create()
        window.layer_service.set_visible("Layer1", False)
        combo = window.layer_panel._combo
        combo.activated.emit(1)
        assert "turned off" in window.command_line._history.toPlainText()
        assert combo.currentText() == "0"

    def test_colour_and_linetype_set_what_new_objects_get_when_nothing_is_selected(self, window):
        panel = window.properties_panel
        panel._color.activated.emit(panel._color.findData(1))
        panel._linetype.activated.emit(panel._linetype.findData("Center"))
        line = draw_line(window)
        assert (line.color, line.linetype) == (1, "Center")

    def test_colour_and_linetype_change_the_selection_and_undo_together_with_nothing_else(self, window):
        line = draw_line(window)
        click(window, 5, 0)
        panel = window.properties_panel
        panel._color.activated.emit(panel._color.findData(2))
        panel._linetype.activated.emit(panel._linetype.findData("Hidden"))
        assert (line.color, line.linetype) == (2, "Hidden")
        window.undo_stack.undo()
        assert (line.color, line.linetype) == (2, "ByLayer")
        window.undo_stack.undo()
        assert line.color == 256
        assert window.document.current_color == 256

    def test_the_combos_show_the_selected_objects_values_and_varies(self, window):
        first = draw_line(window)
        second = draw_line(window, "0,5", "10,5")
        window.controller._selection.replace([first, second])
        window.controller.change_selected(color=1)
        window.controller._selection.replace([first])
        window.controller._publish_selection()
        panel = window.properties_panel
        assert panel._color.currentText() == "Red"
        window.controller._selection.replace([first, second])
        window.controller.change_selected(color=3)
        second.color = 4
        window.controller._publish_selection()
        assert panel._color.currentText() == "*VARIES*"
        assert panel._linetype.currentText() == "ByLayer"

    def test_a_colour_outside_the_standard_list_still_shows(self, window):
        line = draw_line(window)
        line.color = 30
        window.controller._selection.replace([line])
        window.controller._publish_selection()
        assert window.properties_panel._color.currentText() == "Color 30"

    def test_the_panels_are_in_the_ribbon(self, window):
        assert window.layer_panel.isVisible() and window.properties_panel.isVisible()
        assert window.layer_panel.findChildren(QComboBox) == [window.layer_panel._combo]


class TestLayerPalette:
    def rows(self, window):
        table = window.layer_palette._table
        return [table.item(row, lp.NAME).text() for row in range(table.rowCount())]

    def test_the_palette_starts_hidden_and_the_ribbon_button_opens_and_closes_it(self, window):
        assert not window.layer_dock.isVisible()
        window.layer_dock.toggleViewAction().trigger()
        assert window.layer_dock.isVisible()
        window.layer_dock.toggleViewAction().trigger()
        assert not window.layer_dock.isVisible()

    def test_it_lists_the_layers_with_their_switches(self, window):
        service = window.layer_service
        service.create()
        service.set_locked("Layer1", True)
        service.set_color("Layer1", 5)
        table = window.layer_palette._table
        assert self.rows(window) == ["0", "Layer1"]
        assert table.item(1, lp.VISIBLE).checkState() == Qt.CheckState.Checked
        assert table.item(1, lp.LOCKED).checkState() == Qt.CheckState.Checked
        assert table.cellWidget(1, lp.COLOR).currentText() == "Blue"
        assert table.cellWidget(1, lp.LINETYPE).currentText() == "Continuous"

    def test_the_current_layer_is_picked_out(self, window):
        window.layer_service.create()
        window.layer_service.set_current("Layer1")
        table = window.layer_palette._table
        assert table.item(1, lp.NAME).font().bold()
        assert not table.item(0, lp.NAME).font().bold()

    def test_clicking_the_on_and_lock_boxes_changes_the_layer(self, window):
        window.layer_service.create()
        table = window.layer_palette._table
        table.item(1, lp.VISIBLE).setCheckState(Qt.CheckState.Unchecked)
        assert not window.document.layers["Layer1"].visible
        table.item(1, lp.LOCKED).setCheckState(Qt.CheckState.Checked)
        assert window.document.layers["Layer1"].locked
        assert table.item(1, lp.VISIBLE).checkState() == Qt.CheckState.Unchecked

    def test_typing_a_new_name_renames_the_layer(self, window):
        window.layer_service.create()
        window.layer_palette._table.item(1, lp.NAME).setText("Doors")
        assert list(window.document.layers) == ["0", "Doors"]
        assert self.rows(window) == ["0", "Doors"]

    def test_a_refused_rename_says_why_and_puts_the_old_name_back(self, window):
        window.layer_service.create()
        window.layer_service.create()
        window.layer_palette._table.item(1, lp.NAME).setText("layer2")
        assert "already exists" in window.command_line._history.toPlainText()
        assert self.rows(window) == ["0", "Layer1", "Layer2"]

    def test_layer_zero_has_no_editable_name(self, window):
        flags = window.layer_palette._table.item(0, lp.NAME).flags()
        assert not flags & Qt.ItemFlag.ItemIsEditable

    def test_the_colour_and_linetype_boxes_change_the_layer(self, window):
        window.layer_service.create()
        table = window.layer_palette._table
        color_box = table.cellWidget(1, lp.COLOR)
        color_box.setCurrentIndex(color_box.findText("Green"))
        color_box.activated.emit(color_box.currentIndex())
        assert window.document.layers["Layer1"].color == 3
        linetype_box = window.layer_palette._table.cellWidget(1, lp.LINETYPE)
        linetype_box.setCurrentIndex(linetype_box.findText("Phantom"))
        linetype_box.activated.emit(linetype_box.currentIndex())
        assert window.document.layers["Layer1"].linetype == "Phantom"

    def test_new_adds_a_layer_and_starts_editing_its_name(self, window):
        window.layer_palette._new()
        assert self.rows(window) == ["0", "Layer1"]
        assert window.layer_palette._selected_name() == "Layer1"

    def test_delete_and_set_current_work_on_the_selected_row(self, window):
        window.layer_service.create()
        window.layer_service.create()
        palette = window.layer_palette
        palette._table.selectRow(1)
        palette._set_current()
        assert window.document.current_layer == "Layer1"
        palette._table.selectRow(2)
        palette._delete()
        assert self.rows(window) == ["0", "Layer1"]

    def test_delete_explains_a_refusal(self, window):
        window.layer_palette._table.selectRow(0)
        window.layer_palette._delete()
        assert "Layer 0 cannot be deleted." in window.command_line._history.toPlainText()

    def test_undo_updates_the_palette(self, window):
        window.layer_service.create()
        window.undo_stack.undo()
        assert self.rows(window) == ["0"]

    def test_a_new_drawing_clears_the_list(self, window):
        window.layer_service.create()
        from linewright.model.document import Document

        window.set_document(Document())
        assert self.rows(window) == ["0"]
        assert window.layer_panel._combo.count() == 1

    def test_a_layer_with_a_linetype_from_a_file_that_linewright_does_not_know_still_lists(self, window):
        window.layer_service.create()
        window.document.layers["Layer1"].linetype = "GAS_LINE"
        window.layer_palette.refresh()
        assert window.layer_palette._table.cellWidget(1, lp.LINETYPE).currentText() == "GAS_LINE"


def test_opening_a_drawing_with_hidden_and_locked_layers_shows_them_that_way(window, qtbot, tmp_path):
    from linewright.io.files import save_drawing
    from linewright.model.document import Document, Layer

    document = Document(entities=[Line((0, 0), (5, 0), layer="Hidden"), Circle((0, 0), 2, layer="Locked")])
    document.layers["Hidden"] = Layer("Hidden", visible=False)
    document.layers["Locked"] = Layer("Locked", locked=True, color=1)
    path = tmp_path / "states.dxf"
    save_drawing(document, path)
    assert open_and_wait(window, qtbot, path)
    hidden, circle = window.document.entities
    assert not is_drawn(window, hidden)
    assert is_drawn(window, circle)
    assert window.controller._document.editable_entities() == []
