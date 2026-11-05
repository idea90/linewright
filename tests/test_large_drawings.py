import time

import pytest
from PySide6.QtCore import QPointF, QRectF
from PySide6.QtGui import QColor, QImage, QPainter
from PySide6.QtWidgets import QGraphicsScene

from linewright import theme
from linewright.canvas.layer import Detail, EntityLayer, Look
from linewright.canvas.sync import SceneSync
from linewright.model.document import Document, Layer
from linewright.model.entities import Dimension, Line, Text

ROW_GAP = 10.0


def rows(count):
    """Short horizontal lines stacked in a column, one per row."""
    return [Line((0, i * ROW_GAP), (5, i * ROW_GAP)) for i in range(count)]


def synced(document):
    sync = SceneSync(QGraphicsScene())
    sync.reload(document)
    return sync


class TestSpatialQuery:
    def test_only_entities_touching_the_area_come_back_in_drawing_order(self, qapp):
        document = Document(entities=rows(100))
        sync = synced(document)
        found = sync.entities_in(QRectF(0, 3 * ROW_GAP - 1, 5, 2 * ROW_GAP + 2))
        assert len(found) == 3
        assert all(a is b for a, b in zip(found, document.entities[3:6]))

    def test_nothing_comes_back_from_empty_space(self, qapp):
        assert synced(Document(entities=rows(10))).entities_in(QRectF(500, 500, 10, 10)) == []

    def test_hidden_layers_are_left_out(self, qapp):
        document = Document(entities=[Line((0, 0), (5, 0)), Line((0, 1), (5, 1), layer="Gone")])
        document.layers["Gone"] = Layer("Gone", visible=False)
        found = synced(document).entities_in(QRectF(0, -1, 5, 3))
        assert len(found) == 1
        assert found[0] is document.entities[0]

    def test_removed_and_moved_entities_follow_the_drawing(self, qapp):
        document = Document(entities=rows(3))
        sync = synced(document)
        first, second, _ = document.entities
        sync.remove(first)
        assert sync.entities_in(QRectF(0, -1, 5, 2)) == []
        second.start, second.end = (100, 100), (105, 100)
        sync.refresh(second)
        assert sync.entities_in(QRectF(99, 99, 8, 3))[0] is second
        assert sync.entities_in(QRectF(0, ROW_GAP - 1, 5, 2)) == []

    def test_a_dimensions_own_text_is_not_mistaken_for_another_entity(self, qapp):
        dimension = Dimension((0, 0), (10, 0), (5, 6))
        found = synced(Document(entities=[dimension])).entities_in(QRectF(-5, -5, 30, 30))
        assert len(found) == 1
        assert found[0] is dimension


class TestHighlight:
    def test_selecting_draws_an_entity_in_the_accent_color_and_deselecting_restores_it(self, qapp):
        document = Document(entities=[Line((0, 0), (5, 0), color=1)])
        sync = synced(document)
        entity = document.entities[0]
        before = sync.style_of(entity)
        sync.set_selected([entity])
        assert sync.style_of(entity).color.name() == theme.ACCENT
        assert sync.style_of(entity).width > before.width
        sync.set_selected([])
        assert sync.style_of(entity) == before
        assert sync.selected == []

    def test_a_selected_entity_stays_highlighted_when_it_is_redrawn(self, qapp):
        document = Document(entities=[Line((0, 0), (5, 0))])
        sync = synced(document)
        entity = document.entities[0]
        sync.set_selected([entity])
        entity.end = (9, 0)
        sync.refresh(entity)
        assert sync.style_of(entity).color.name() == theme.ACCENT
        assert sync.selected[0] is entity

    def test_removing_a_selected_entity_drops_it_from_the_selection(self, qapp):
        document = Document(entities=rows(2))
        sync = synced(document)
        sync.set_selected(document.entities)
        sync.remove(document.entities[0])
        assert len(sync.selected) == 1

    def test_only_entities_of_the_drawing_can_be_selected(self, qapp):
        sync = synced(Document(entities=rows(2)))
        sync.set_selected([Line((0, 0), (1, 1))])
        assert sync.selected == []


class TestDetailLimits:
    def painted(self, entity, scale, qapp):
        document = Document(entities=[entity])
        layer = EntityLayer(lambda: document, Look())
        layer.reload(document.entities)
        image = QImage(200, 200, QImage.Format.Format_RGB32)
        image.fill(QColor("black"))
        painter = QPainter(image)
        painter.translate(0, 200)
        painter.scale(scale, -scale)
        layer.paint(painter, QRectF(0, 0, 200 / scale, 200 / scale), scale)
        painter.end()
        return sum(image.pixelColor(x, y) != QColor("black") for x in range(200) for y in range(200))

    def test_text_too_small_to_read_is_not_drawn(self, qapp):
        assert 10 * 0.1 < Detail().min_text
        assert self.painted(Text((1, 1), "WALL", 10), 0.1, qapp) == 0

    def test_readable_text_is_drawn(self, qapp):
        assert self.painted(Text((1, 1), "WALL", 10), 2.0, qapp) > 0

    def test_a_speck_far_smaller_than_a_pixel_is_skipped_but_the_big_geometry_beside_it_is_drawn(self, qapp):
        big = Line((0, 12_500), (100_000, 12_500))
        speck = Line((50_000, 20_000), (50_001, 20_000))
        document = Document(entities=[big, speck])
        layer = EntityLayer(lambda: document, Look())
        layer.reload(document.entities)
        image = QImage(200, 50, QImage.Format.Format_RGB32)
        image.fill(QColor("black"))
        painter = QPainter(image)
        painter.translate(0, 50)
        scale = 200 / 100_000
        painter.scale(scale, -scale)
        layer.paint(painter, QRectF(0, 0, 100_000, 25_000), scale)
        painter.end()
        lit = {(x, y) for x in range(200) for y in range(50) if image.pixelColor(x, y) != QColor("black")}
        assert {y for _, y in lit} == {25}  # only the long line's row, nothing from the speck's row


class TestRemovingMany:
    def test_equal_looking_entities_are_removed_by_identity(self):
        first, second, third = Line((0, 0), (1, 1)), Line((0, 0), (1, 1)), Line((2, 2), (3, 3))
        document = Document(entities=[first, second, third])
        assert document.positions_of([third, first]) == [2, 0]
        document.remove_all([first])
        assert document.entities[0] is second
        assert len(document.entities) == 2

    def test_an_entity_that_is_not_there_is_an_error(self):
        with pytest.raises(ValueError):
            Document(entities=[Line((0, 0), (1, 1))]).positions_of([Line((0, 0), (1, 1))])


class TestBigDrawingsStayResponsive:
    COUNT = 20_000
    MOVES = 20
    BUDGET_SECONDS = 1.0  # the whole-drawing scan this replaced took several seconds for this many

    def test_the_cursor_snaps_quickly_in_a_drawing_with_thousands_of_objects(self, window):
        window.set_document(Document(entities=rows(self.COUNT)))
        window.controller.start_command("LINE")
        window.controller.click(QPointF(1, 1))
        middle = (self.COUNT // 2) * ROW_GAP
        started = time.perf_counter()
        for step in range(self.MOVES):
            window.controller.move_cursor(QPointF(0.5 + step * 0.1, middle + 0.5))
        assert time.perf_counter() - started < self.BUDGET_SECONDS
        assert window.view._marker is not None  # it still found something to snap to

    def test_picking_and_window_selection_find_the_right_objects_in_a_big_drawing(self, window):
        document = Document(entities=rows(self.COUNT))
        window.set_document(document)
        window.controller.click(QPointF(2, 5 * ROW_GAP + 0.2))
        window.controller.release(QPointF(2, 5 * ROW_GAP + 0.2))
        picked = window.controller.selected()
        assert len(picked) == 1
        assert picked[0] is document.entities[5]
        window.controller.cancel()
        window.view.resetTransform()
        window.view.scale(4, -4)  # zoomed in enough that the drag is longer than a click
        window.controller.click(QPointF(-1, 9 * ROW_GAP - 1))
        window.controller.release(QPointF(6, 12 * ROW_GAP + 1))
        assert all(a is b for a, b in zip(window.controller.selected(), document.entities[9:13]))
        assert len(window.controller.selected()) == 4

    def test_erasing_everything_and_undoing_restores_the_order(self, window):
        document = Document(entities=rows(2000))
        original = list(document.entities)
        window.set_document(document)
        window.controller._select_all()
        window.controller.erase_selection()
        assert window.document.entities == []
        window.undo_stack.undo()
        assert len(window.document.entities) == len(original)
        assert all(a is b for a, b in zip(window.document.entities, original))


