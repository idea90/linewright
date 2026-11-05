import pytest
from PySide6.QtCore import QRectF
from PySide6.QtGui import QColor, QImage, QPainter
from PySide6.QtWidgets import QGraphicsScene

from linewright.canvas.layer import EntityLayer, Look, entity_extent
from linewright.canvas.shapes import TextSpec, entity_shapes, resolve_color
from linewright.canvas.sync import SceneSync
from linewright.model.document import Document, Layer
from linewright.model.entities import Arc, Circle, Line, Polyline, Text


def path_of(entity):
    return entity_shapes(entity).strokes


def test_arc_ends_at_the_end_angle():
    end = path_of(Arc((0, 0), 10, 0, 90)).currentPosition()
    assert (end.x(), end.y()) == pytest.approx((0, 10), abs=1e-6)


def test_arc_crossing_zero_degrees_sweeps_counter_clockwise():
    middle = path_of(Arc((0, 0), 10, 270, 90)).pointAtPercent(0.5)
    assert (middle.x(), middle.y()) == pytest.approx((10, 0), abs=0.05)


def test_bulged_polyline_segment_bows_to_the_right():
    middle = path_of(Polyline([(0, 0, 1.0), (2, 0, 0.0)])).pointAtPercent(0.5)
    assert (middle.x(), middle.y()) == pytest.approx((1, -1), abs=0.05)


def test_closed_polyline_returns_to_its_first_vertex():
    end = path_of(Polyline([(0, 0, 0.0), (4, 0, 0.0), (4, 3, 0.0)], closed=True)).currentPosition()
    assert (end.x(), end.y()) == pytest.approx((0, 0))


def test_color_follows_the_layer_unless_overridden():
    document = Document(layers={"0": Layer("0"), "WALLS": Layer("WALLS", 1)})
    assert resolve_color(Line((0, 0), (1, 1), layer="WALLS"), document) == QColor(255, 0, 0)
    assert resolve_color(Line((0, 0), (1, 1), layer="WALLS", color=3), document) == QColor(0, 255, 0)


def test_text_is_described_by_where_its_baseline_starts():
    shapes = entity_shapes(Text((5, 5), "Hi", 2.0, 30.0))
    assert shapes.strokes is None
    assert shapes.text == TextSpec((5, 5), 30.0, 2.0, "Hi")


def test_sync_adds_removes_and_previews_entities():
    sync = SceneSync(QGraphicsScene())
    line = Line((0, 0), (1, 1))
    sync.reload(Document(entities=[line, Circle((0, 0), 1), Text((0, 0), "A", 1.0)]))
    everywhere = QRectF(-10, -10, 20, 20)
    assert len(sync.entities_in(everywhere)) == 3
    sync.remove(line)
    assert len(sync.entities_in(everywhere)) == 2
    sync.set_preview([Line((0, 0), (2, 2)), Line((0, 0), (3, 3))])
    assert len(sync.previewing) == 2
    assert len(sync.entities_in(everywhere)) == 2  # the rubber band is not part of the drawing
    sync.set_preview([Line((0, 0), (2, 2))])
    assert len(sync.previewing) == 1
    sync.clear_preview()
    assert sync.previewing == []


class TestExtents:
    def test_boxes_hold_their_entities(self):
        assert entity_extent(Line((3, 4), (1, 9))) == (1, 4, 3, 9)
        assert entity_extent(Circle((0, 0), 2)) == (-2, -2, 2, 2)

    def test_a_bulged_polyline_box_holds_the_bulge(self):
        x0, y0, x1, y1 = entity_extent(Polyline([(0, 0, 1.0), (2, 0, 0.0)]))
        assert y0 <= -1 and x0 <= 0 and x1 >= 2

    def test_a_polyline_with_a_repeated_vertex_and_a_bulge_does_not_crash(self):
        assert entity_extent(Polyline([(1, 1, 0.5), (1, 1, 0.0)])) == (1, 1, 1, 1)

    def test_text_boxes_reach_above_the_baseline_and_along_it(self):
        x0, y0, x1, y1 = entity_extent(Text((5, 5), "Hi", 2.0))
        assert y1 >= 7 and y0 < 5 and x0 <= 5 and x1 >= 9


def paint_into_image(entity, scale, size=100):
    """Draw one entity on a black square whose drawing units are `scale` pixels, y up, origin bottom-left."""
    document = Document(entities=[entity])
    layer = EntityLayer(lambda: document, Look())
    layer.reload(document.entities)
    image = QImage(size, size, QImage.Format.Format_RGB32)
    image.fill(QColor("black"))
    painter = QPainter(image)
    painter.translate(0, size)
    painter.scale(scale, -scale)
    layer.paint(painter, QRectF(0, 0, size / scale, size / scale), scale)
    painter.end()
    return image


def lit_rows(image):
    black = QColor("black")
    return [y for y in range(image.height()) if any(image.pixelColor(x, y) != black for x in range(image.width()))]


def test_text_sits_above_its_baseline_point():
    image = paint_into_image(Text((1, 3), "Hi", 2.0), scale=10)
    rows = lit_rows(image)
    baseline_row = 100 - 3 * 10
    assert rows and min(rows) < baseline_row - 10  # reaches up toward the text's top
    assert max(rows) <= baseline_row + 3  # nothing hangs far below the baseline
