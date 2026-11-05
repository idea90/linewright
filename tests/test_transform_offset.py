import pytest

from linewright.model.entities import Arc, Circle, Line, Polyline, Text
from linewright.model.offset import offset_entity
from linewright.model.transform import reflection, rotation, scaling, transform_entity, translation


class TestTransform:
    def test_translate_a_line(self):
        moved = transform_entity(Line((0, 0), (10, 5), layer="A", color=3), translation(2, -1))
        assert moved == Line((2, -1), (12, 4), layer="A", color=3)

    def test_rotate_a_line_a_quarter_turn_about_the_origin(self):
        turned = transform_entity(Line((1, 0), (2, 0)), rotation((0, 0), 90))
        assert turned.start == pytest.approx((0, 1), abs=1e-9)
        assert turned.end == pytest.approx((0, 2), abs=1e-9)

    def test_rotation_leaves_the_center_where_it_is(self):
        turned = transform_entity(Line((5, 5), (6, 5)), rotation((5, 5), 33))
        assert turned.start == pytest.approx((5, 5), abs=1e-9)

    def test_scale_a_circle_about_a_base_point(self):
        scaled = transform_entity(Circle((2, 2), 1), scaling((0, 0), 2))
        assert (scaled.center, scaled.radius) == ((4, 4), 2)

    def test_scale_keeps_the_base_point_fixed(self):
        scaled = transform_entity(Line((10, 10), (20, 10)), scaling((10, 10), 3))
        assert scaled.start == pytest.approx((10, 10))
        assert scaled.end == pytest.approx((40, 10))

    def test_rotate_an_arc_adds_the_angle(self):
        turned = transform_entity(Arc((0, 0), 5, 0, 90), rotation((0, 0), 45))
        assert (turned.start_angle, turned.end_angle) == pytest.approx((45, 135))

    def test_mirror_an_arc_across_the_x_axis_swaps_its_ends(self):
        mirrored = transform_entity(Arc((0, 0), 5, 0, 90), reflection((0, 0), (1, 0)))
        assert (mirrored.start_angle, mirrored.end_angle) == pytest.approx((270, 0), abs=1e-9)

    def test_mirror_across_a_diagonal(self):
        mirrored = transform_entity(Line((4, 0), (6, 0)), reflection((0, 0), (1, 1)))
        assert mirrored.start == pytest.approx((0, 4), abs=1e-9)
        assert mirrored.end == pytest.approx((0, 6), abs=1e-9)

    def test_mirror_flips_the_bulge_of_a_polyline(self):
        mirrored = transform_entity(Polyline([(0, 0, 0.5), (2, 0, 0.0)]), reflection((0, 0), (1, 0)))
        assert mirrored.vertices == [(0, 0, -0.5), (2, 0, 0.0)]

    def test_rotating_a_polyline_keeps_its_bulge(self):
        turned = transform_entity(Polyline([(0, 0, 0.5), (2, 0, 0.0)], closed=True), rotation((0, 0), 90))
        assert turned.closed
        assert turned.vertices[0][2] == 0.5

    def test_text_scales_and_turns_with_its_object(self):
        text = transform_entity(Text((1, 0), "Hi", 2.0, 10.0), rotation((0, 0), 90))
        assert text.position == pytest.approx((0, 1), abs=1e-9)
        assert text.rotation == pytest.approx(100)
        assert transform_entity(Text((0, 0), "Hi", 2.0), scaling((0, 0), 3)).height == pytest.approx(6.0)

    def test_mirrored_text_stays_readable(self):
        text = transform_entity(Text((3, 4), "Hi", 2.0, 30.0), reflection((0, 0), (1, 0)))
        assert text.position == pytest.approx((3, -4))
        assert text.rotation == 30.0

    def test_the_original_is_left_alone(self):
        line = Line((0, 0), (1, 1))
        transform_entity(line, translation(5, 5))
        assert line == Line((0, 0), (1, 1))

    def test_unknown_entity_types_are_refused(self):
        with pytest.raises(TypeError):
            transform_entity(object(), translation(1, 1))


class TestOffset:
    HORIZONTAL = Line((0, 0), (10, 0), layer="WALLS", color=2)

    def test_line_goes_to_the_side_of_the_pick_point(self):
        assert offset_entity(self.HORIZONTAL, 2, (5, 3)) == Line((0, 2), (10, 2), layer="WALLS", color=2)
        assert offset_entity(self.HORIZONTAL, 2, (5, -3)) == Line((0, -2), (10, -2), layer="WALLS", color=2)

    def test_circle_grows_outward_and_shrinks_inward(self):
        circle = Circle((0, 0), 5)
        assert offset_entity(circle, 2, (10, 0)).radius == 7
        assert offset_entity(circle, 2, (1, 0)).radius == 3

    def test_circle_cannot_shrink_to_nothing(self):
        assert offset_entity(Circle((0, 0), 5), 5, (1, 0)) is None
        assert offset_entity(Circle((0, 0), 5), 6, (1, 0)) is None

    def test_arc_keeps_its_angles(self):
        arc = offset_entity(Arc((0, 0), 5, 10, 80), 1, (9, 9))
        assert (arc.radius, arc.start_angle, arc.end_angle) == (6, 10, 80)

    def test_open_polyline_gets_sharp_corners(self):
        elbow = Polyline([(0, 0, 0.0), (10, 0, 0.0), (10, 10, 0.0)])
        result = offset_entity(elbow, 1, (5, 5))
        assert result.vertices == [(0, 1, 0.0), (9, 1, 0.0), (9, 10, 0.0)]

    def test_closed_polyline_offsets_outward_or_inward(self):
        square = Polyline([(0, 0, 0.0), (10, 0, 0.0), (10, 10, 0.0), (0, 10, 0.0)], closed=True)
        outer = offset_entity(square, 1, (-5, 5))
        assert outer.closed
        assert [(x, y) for x, y, _ in outer.vertices] == [(-1, -1), (11, -1), (11, 11), (-1, 11)]
        inner = offset_entity(square, 1, (5, 5))
        assert [(x, y) for x, y, _ in inner.vertices] == [(1, 1), (9, 1), (9, 9), (1, 9)]

    def test_collinear_polyline_segments_stay_joined(self):
        straight = Polyline([(0, 0, 0.0), (5, 0, 0.0), (10, 0, 0.0)])
        assert [(x, y) for x, y, _ in offset_entity(straight, 2, (5, 5)).vertices] == [(0, 2), (5, 2), (10, 2)]

    def test_what_cannot_be_offset(self):
        assert offset_entity(Polyline([(0, 0, 0.5), (2, 0, 0.0)]), 1, (1, 1)) is None
        assert offset_entity(Text((0, 0), "x", 1.0), 1, (1, 1)) is None
        assert offset_entity(Line((1, 1), (1, 1)), 1, (2, 2)) is None
        assert offset_entity(Polyline([(1, 1, 0.0), (1, 1, 0.0)]), 1, (2, 2)) is None
