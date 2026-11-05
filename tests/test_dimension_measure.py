import math

import ezdxf
import pytest
from PySide6.QtCore import QPointF, QRectF
from PySide6.QtGui import QColor, QImage, QPainter
from PySide6.QtWidgets import QGraphicsScene

from functools import partial

from helpers import new_session
from linewright import theme
from linewright.canvas.layer import EntityLayer, Look
from linewright.canvas.shapes import entity_shapes, text_width
from linewright.canvas.sync import SceneSync
from linewright.commands import dimension, measuring
from linewright.commands.defaults import Defaults
from linewright.io.dxf import document_from_drawing, drawing_from_document, mm_per_unit
from linewright.model.dimension import (
    ARROW_LENGTH,
    EXTENSION_GAP,
    EXTENSION_OVERSHOOT,
    TEXT_GAP,
    dimension_parts,
    measurement,
)
from linewright.model.document import Document
from linewright.model.entities import Circle, Dimension, Line, Polyline
from linewright.model.measure import distance_report, measure_entity, polygon
from linewright.model.transform import reflection, rotation, scaling, transform_entity, translation
from linewright.snap.primitives import primitives


DEFAULTS = Defaults()


def flat(segment):
    return tuple(coordinate for point in segment for coordinate in point)


def run(command, *answers):
    host, session = new_session(command)
    for answer in answers:
        assert session.active, f"command ended early before {answer!r}"
        assert session.submit_text(answer) is None
    return host, session


class TestDimensionGeometry:
    def test_horizontal_dimension_puts_the_line_and_text_above_the_points(self):
        parts = dimension_parts(Dimension((0, 0), (10, 0), (5, 6)))
        assert parts.dimension_line == ((0, 6), (10, 6))
        assert parts.extension_lines == [
            ((0, EXTENSION_GAP), (0, 6 + EXTENSION_OVERSHOOT)),
            ((10, EXTENSION_GAP), (10, 6 + EXTENSION_OVERSHOOT)),
        ]
        assert parts.text == "10.00"
        assert parts.text_anchor == pytest.approx((5, 6 + TEXT_GAP))
        assert parts.text_angle == 0

    def test_line_below_the_points_flips_the_extension_lines_down(self):
        parts = dimension_parts(Dimension((0, 0), (10, 0), (5, -6)))
        assert parts.extension_lines[0] == ((0, -EXTENSION_GAP), (0, -6 - EXTENSION_OVERSHOOT))

    def test_arrows_sit_on_the_line_ends_and_point_outward(self):
        parts = dimension_parts(Dimension((0, 0), (10, 0), (5, 6)))
        (first_tip, first_left, _), (second_tip, _, _) = parts.arrows
        assert first_tip == (0, 6)
        assert second_tip == (10, 6)
        assert first_left[0] == pytest.approx(ARROW_LENGTH)

    def test_vertical_dimension_measures_height_and_reads_bottom_to_top(self):
        dim = Dimension((0, 0), (4, 7), (10, 3), angle=90)
        assert measurement(dim) == pytest.approx(7)
        parts = dimension_parts(dim)
        assert flat(parts.dimension_line) == pytest.approx((10, 0, 10, 7))
        assert parts.text_angle == 90
        assert parts.text_anchor == pytest.approx((10 - TEXT_GAP, 3.5))

    @pytest.mark.parametrize("angle", [0, 30, 90, 135, 180, 225, 270, 359])
    def test_text_is_never_upside_down(self, angle):
        assert -90 < dimension_parts(Dimension((0, 0), (5, 5), (1, 1), angle=angle)).text_angle <= 90

    def test_an_aligned_dimension_measures_along_its_own_direction(self):
        dim = Dimension((0, 0), (3, 4), (0, 0), angle=math.degrees(math.atan2(4, 3)))
        assert measurement(dim) == pytest.approx(5)

    def test_scale_sizes_the_text_arrows_and_gaps_but_not_the_measurement(self):
        small = dimension_parts(Dimension((0, 0), (10, 0), (5, 6)))
        big = dimension_parts(Dimension((0, 0), (10, 0), (5, 6), scale=50))
        assert big.text == small.text
        assert big.text_height == 50 * small.text_height
        assert big.arrows[0][1][0] == pytest.approx(50 * small.arrows[0][1][0])
        assert big.extension_lines[0][0][1] == pytest.approx(50 * small.extension_lines[0][0][1])

    def test_the_text_follows_the_measurement(self):
        assert dimension_parts(Dimension((0, 0), (3.14159, 0), (0, 2))).text == "3.14"

    def test_the_mouse_can_pick_the_dimension_line_and_snaps_reach_its_ends(self):
        shapes = primitives(Dimension((0, 0), (10, 0), (5, 6)))
        assert Line((0, 6), (10, 6)) in shapes
        assert len(shapes) == 3


class TestDimensionTransforms:
    DIM = Dimension((0, 0), (10, 0), (5, 6))

    def test_move_keeps_the_measurement(self):
        moved = transform_entity(self.DIM, translation(100, 50))
        assert moved.start == (100, 50)
        assert measurement(moved) == pytest.approx(10)

    def test_rotate_turns_the_dimension_line(self):
        turned = transform_entity(self.DIM, rotation((0, 0), 90))
        assert turned.angle == pytest.approx(90)
        assert measurement(turned) == pytest.approx(10)

    def test_scale_changes_the_measurement(self):
        assert measurement(transform_entity(self.DIM, scaling((0, 0), 2))) == pytest.approx(20)

    def test_mirror_keeps_the_measurement(self):
        mirrored = transform_entity(self.DIM, reflection((0, 0), (1, 1)))
        assert mirrored.angle == pytest.approx(90)
        assert measurement(mirrored) == pytest.approx(10)


class TestDimensionCommands:
    def test_points_above_the_measured_points_make_a_horizontal_dimension(self):
        host, session = run(partial(dimension.linear, [], DEFAULTS), "0,0", "10,3", "5,8")
        assert host.added == [Dimension((0, 0), (10, 3), (5, 8), 0.0)]
        assert not session.active

    def test_a_point_beside_the_measured_points_makes_a_vertical_dimension(self):
        host, _ = run(partial(dimension.linear, [], DEFAULTS), "0,0", "10,3", "15,1")
        assert host.added[0].angle == 90.0

    def test_aligned_follows_the_two_points(self):
        host, _ = run(partial(dimension.aligned, [], DEFAULTS), "0,0", "3,4", "0,5")
        assert host.added[0].angle == pytest.approx(math.degrees(math.atan2(4, 3)))

    def test_the_same_point_twice_is_refused(self):
        _, session = run(partial(dimension.linear, [], DEFAULTS), "2,2", "2,2")
        assert session.prompt.message.startswith("The points are the same.")

    def test_enter_before_the_location_draws_nothing(self):
        host, session = run(partial(dimension.linear, [], DEFAULTS), "0,0", "10,0", "")
        assert host.added == []
        assert not session.active

    def test_dimensions_take_the_remembered_scale(self):
        defaults = Defaults()
        host, _ = new_session(dimension.set_scale, [], defaults)
        _, session = new_session(dimension.set_scale, [], defaults)
        assert session.prompt.message == "Enter new value for DIMSCALE <1>:"
        session.submit_text("50")
        assert defaults.dimension_scale == 50
        host, _ = run(partial(dimension.linear, [], defaults), "0,0", "10,0", "5,8")
        assert host.added[0].scale == 50

    def test_dimscale_must_be_positive_and_enter_keeps_the_value(self):
        defaults = Defaults(dimension_scale=20)
        _, session = new_session(dimension.set_scale, [], defaults)
        assert session.submit_text("0") is not None
        session.submit_text("")
        assert defaults.dimension_scale == 20

    def test_the_location_prompt_previews_the_dimension(self):
        _, session = run(partial(dimension.linear, [], DEFAULTS), "0,0", "10,0")
        assert session.prompt.preview((5, 4)) == [Dimension((0, 0), (10, 0), (5, 4), 0.0)]


class TestMeasuring:
    def test_distance_reports_length_angle_and_deltas(self):
        host, session = run(measuring.distance, "1,1", "4,5")
        assert host.reports == [distance_report((1, 1), (4, 5))]
        assert host.reports[0] == "Distance = 5.0000, Angle in XY Plane = 53, Delta X = 3.0000, Delta Y = 4.0000"
        assert not session.active

    def test_area_from_points_reports_area_and_perimeter(self):
        host, _ = run(measuring.area, "0,0", "10,0", "10,10", "0,10", "")
        assert host.reports == ["Area = 100.0000, Perimeter = 40.0000"]

    def test_area_needs_three_points(self):
        host, _ = run(measuring.area, "0,0", "10,0", "")
        assert host.reports == [measuring.TOO_FEW_POINTS]

    def test_area_by_object_measures_a_circle(self):
        _, session = run(measuring.area, "")
        session.submit_pick(Circle((0, 0), 2), (2, 0))
        assert not session.active

    def test_area_by_object_refuses_an_open_polyline_and_asks_again(self):
        host, session = run(measuring.area, "o")
        session.submit_pick(Polyline([(0, 0, 0.0), (5, 0, 0.0), (5, 5, 0.0)]), (1, 0))
        assert session.active
        assert measuring.NO_AREA in session.prompt.message
        session.submit_pick(Polyline([(0, 0, 0.0), (4, 0, 0.0), (4, 3, 0.0)], closed=True), (1, 0))
        assert host.reports == ["Area = 6.0000, Perimeter = 12.0000"]

    def test_triangle_area_is_the_same_whichever_way_it_is_drawn(self):
        corners = [(0, 0, 0.0), (4, 0, 0.0), (4, 3, 0.0)]
        assert polygon(corners).area == pytest.approx(6)
        assert polygon(corners[::-1]).area == pytest.approx(6)

    def test_circle_area_and_circumference(self):
        measured = measure_entity(Circle((5, 5), 2))
        assert measured.area == pytest.approx(math.pi * 4)
        assert measured.perimeter == pytest.approx(math.pi * 4)

    def test_a_bulged_segment_adds_its_cap(self):
        half_disc = Polyline([(0, 0, 1.0), (2, 0, 0.0)], closed=True)
        measured = measure_entity(half_disc)
        assert measured.area == pytest.approx(math.pi / 2)
        assert measured.perimeter == pytest.approx(math.pi + 2)

    def test_a_bulge_adds_area_outward_and_takes_it_away_inward(self):
        sides = [(2, 0, 0.0), (2, 2, 0.0), (0, 2, 0.0)]  # counter-clockwise, so outside is right of the first side
        assert polygon([(0, 0, 1.0), *sides]).area == pytest.approx(4 + math.pi / 2)
        assert polygon([(0, 0, -1.0), *sides]).area == pytest.approx(4 - math.pi / 2)

    def test_lines_and_open_polylines_have_no_area(self):
        assert measure_entity(Line((0, 0), (1, 1))) is None


class TestDimensionFiles:
    def test_a_dimension_survives_save_and_reopen(self, tmp_path):
        dim = Dimension((0, 0), (10, 0), (5, 6), 0.0, layer="0")
        path = tmp_path / "plan.dxf"
        drawing_from_document(Document(entities=[dim, Line((0, 0), (1, 1))])).saveas(path)
        reopened = document_from_drawing(ezdxf.readfile(path))
        assert not reopened.skipped
        loaded = reopened.entities[0]
        assert (loaded.start, loaded.end, loaded.angle) == ((0, 0), (10, 0), 0.0)
        assert measurement(loaded) == pytest.approx(10)
        assert flat(dimension_parts(loaded).dimension_line) == pytest.approx((0, 6, 10, 6))

    def test_a_vertical_dimension_keeps_its_direction(self, tmp_path):
        path = tmp_path / "plan.dxf"
        drawing_from_document(Document(entities=[Dimension((0, 0), (4, 7), (10, 3), 90.0)])).saveas(path)
        loaded = document_from_drawing(ezdxf.readfile(path)).entities[0]
        assert loaded.angle == 90.0
        assert measurement(loaded) == pytest.approx(7)

    def test_the_dimension_scale_survives_save_and_reopen(self, tmp_path):
        path = tmp_path / "plan.dxf"
        drawing_from_document(Document(entities=[Dimension((0, 0), (6000, 0), (3000, -700), scale=50)])).saveas(path)
        assert document_from_drawing(ezdxf.readfile(path)).entities[0].scale == 50

    def test_an_aligned_dimension_from_another_program_is_read_along_its_points(self):
        drawing = ezdxf.new("R2018", setup=True)
        drawing.modelspace().add_aligned_dim(p1=(0, 0), p2=(3, 4), distance=2, dimstyle="Standard").render()
        loaded = document_from_drawing(drawing).entities[0]
        assert measurement(loaded) == pytest.approx(5)

    def test_saving_again_does_not_pile_up_dimension_blocks(self):
        document = Document(entities=[Dimension((0, 0), (10, 0), (5, 6))])
        drawing = drawing_from_document(document)
        before = len(drawing.blocks)
        for _ in range(3):
            drawing_from_document(document)
        assert len(drawing.blocks) == before
        assert len(drawing.modelspace()) == 1

    def test_drawing_units_come_from_the_file(self):
        assert mm_per_unit(Document()) == 1.0
        assert mm_per_unit(Document(source=drawing_from_document(Document()))) == 1.0
        meters = ezdxf.new("R2018")
        meters.header["$INSUNITS"] = 6
        assert mm_per_unit(Document(source=meters)) == 1000.0
        unknown = ezdxf.new("R2018")
        unknown.header["$INSUNITS"] = 99
        assert mm_per_unit(Document(source=unknown)) == 1.0


@pytest.mark.usefixtures("qapp")  # text metrics need a running application
class TestDimensionOnScreen:
    DIMENSION = Dimension((0, 0), (10, 0), (5, 6))

    def test_it_is_lines_and_filled_arrowheads_with_the_measurement_as_text(self):
        shapes = entity_shapes(self.DIMENSION)
        assert not shapes.strokes.isEmpty()
        assert not shapes.fills.isEmpty()
        assert shapes.text.text == "10.00"

    def test_the_text_is_centered_on_the_dimension_line_and_above_it(self):
        spec = entity_shapes(self.DIMENSION).text
        middle = spec.origin[0] + text_width(spec.text, spec.height) / 2
        assert middle == pytest.approx(5, abs=0.01)
        assert spec.origin[1] > 6

    def test_a_dimension_draws_arrowheads_and_text_on_screen(self):
        document = Document(entities=[Dimension((0, 0), (40, 0), (20, 8), scale=4)])
        layer = EntityLayer(lambda: document, Look())
        layer.reload(document.entities)
        image = QImage(200, 100, QImage.Format.Format_RGB32)
        image.fill(QColor("black"))
        painter = QPainter(image)
        painter.translate(0, 100)
        painter.scale(4, -4)
        layer.paint(painter, QRectF(0, 0, 50, 25), 4.0)
        painter.end()
        lit = {(x, y) for x in range(200) for y in range(100) if image.pixelColor(x, y) != QColor("black")}
        line_row = 100 - 8 * 4
        assert any(y == line_row for _, y in lit)
        assert any(y < line_row - 8 for _, y in lit)  # the text sits above the line
        assert any(abs(y - line_row) <= 5 and 5 < x < 40 for x, y in lit)  # the arrowhead is thicker than the line

    def test_selecting_it_highlights_the_whole_dimension(self):
        sync = SceneSync(QGraphicsScene())
        dimension = Dimension((0, 0), (10, 0), (5, 6), color=1)
        sync.reload(Document(entities=[dimension]))
        sync.set_selected([dimension])
        assert sync.selected == [dimension]
        assert sync.style_of(dimension).color.name() == theme.ACCENT
        sync.set_selected([])
        assert sync.style_of(dimension).color.name() == "#ff0000"

    def test_a_dimension_can_be_dragged_through_a_window(self, window):
        for text in ("dli", "0,0", "10,0", "5,6"):
            window.command_line._input.setText(text)
            window.command_line._input.returnPressed.emit()
        assert window.document.entities == [Dimension((0, 0), (10, 0), (5, 6), 0.0)]
        window.controller.click(QPointF(5, 6.0))
        window.controller.release(QPointF(5, 6.0))
        assert window.controller.selected() == window.document.entities

    def test_distance_prints_on_the_command_line(self, window):
        for text in ("di", "0,0", "3,4"):
            window.command_line._input.setText(text)
            window.command_line._input.returnPressed.emit()
        assert "Distance = 5.0000" in window.command_line._history.toPlainText()
