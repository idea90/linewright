import math

import pytest

from linewright.model.entities import Arc, Circle, Line, Polyline, Text
from linewright.model.fillet import FilletError, fillet_lines
from linewright.model.trimming import extend_entity, trim_entity

VERTICAL_AT_5 = Line((5, -5), (5, 5))


def points(polyline):
    return [(x, y) for x, y, _ in polyline.vertices]


class TestTrimLine:
    TARGET = Line((0, 0), (10, 0), layer="WALLS", color=3)

    def test_the_side_under_the_click_is_cut_away(self):
        assert trim_entity(self.TARGET, (8, 0), [VERTICAL_AT_5]) == [Line((0, 0), (5, 0), layer="WALLS", color=3)]
        assert trim_entity(self.TARGET, (2, 0), [VERTICAL_AT_5]) == [Line((5, 0), (10, 0), layer="WALLS", color=3)]

    def test_between_two_boundaries_the_middle_goes_and_two_pieces_stay(self):
        edges = [Line((3, -1), (3, 1)), Line((7, -1), (7, 1))]
        assert trim_entity(self.TARGET, (5, 0), edges) == [
            Line((0, 0), (3, 0), layer="WALLS", color=3),
            Line((7, 0), (10, 0), layer="WALLS", color=3),
        ]

    def test_outside_two_boundaries_trims_back_to_the_nearest(self):
        edges = [Line((3, -1), (3, 1)), Line((7, -1), (7, 1))]
        assert trim_entity(self.TARGET, (1, 0), edges) == [Line((3, 0), (10, 0), layer="WALLS", color=3)]

    def test_the_click_need_not_be_exactly_on_the_line(self):
        assert trim_entity(self.TARGET, (8, 3), [VERTICAL_AT_5]) == [Line((0, 0), (5, 0), layer="WALLS", color=3)]

    @pytest.mark.parametrize(
        "boundary",
        [Line((5, 1), (5, 5)), Line((0, 3), (10, 3)), Line((10, -5), (10, 5)), Line((20, -5), (20, 5))],
        ids=["too short to reach", "parallel", "only touches the end", "beyond the end"],
    )
    def test_nothing_to_cut_against_means_no_trim(self, boundary):
        assert trim_entity(self.TARGET, (8, 0), [boundary]) is None

    def test_a_target_is_never_its_own_boundary(self):
        assert trim_entity(self.TARGET, (8, 0), [self.TARGET]) is None
        assert trim_entity(self.TARGET, (8, 0), []) is None

    def test_a_boundary_circle_cuts_where_it_crosses(self):
        assert trim_entity(self.TARGET, (1, 0), [Circle((5, 0), 2)]) == [Line((3, 0), (10, 0), layer="WALLS", color=3)]


class TestTrimRound:
    def test_arc_loses_the_stretch_under_the_click(self):
        arc = Arc((0, 0), 5, 0, 180)
        (left,) = trim_entity(arc, (3.5, 3.5), [Line((0, -10), (0, 10))])
        assert (left.start_angle, left.end_angle) == pytest.approx((90, 180))

    def test_arc_with_two_cuts_keeps_the_ends(self):
        arc = Arc((0, 0), 5, 0, 180)
        edges = [Line((-3, -10), (-3, 10)), Line((3, -10), (3, 10))]
        first, second = trim_entity(arc, (0, 5), edges)
        assert first.start_angle == pytest.approx(0)
        assert first.end_angle == pytest.approx(math.degrees(math.atan2(4, 3)))
        assert second.start_angle == pytest.approx(math.degrees(math.atan2(4, -3)))
        assert second.end_angle == pytest.approx(180)

    def test_circle_becomes_the_arc_that_is_left(self):
        circle = Circle((0, 0), 5, layer="A", color=2)
        edge = Line((-10, 0), (10, 0))
        (upper,) = trim_entity(circle, (0, -5), [edge])
        assert isinstance(upper, Arc)
        assert (upper.center, upper.radius, upper.layer, upper.color) == ((0, 0), 5, "A", 2)
        assert (upper.start_angle, upper.end_angle) == pytest.approx((0, 180))
        (lower,) = trim_entity(circle, (0, 5), [edge])
        assert (lower.start_angle, lower.end_angle) == pytest.approx((180, 0))

    def test_a_circle_needs_two_cuts(self):
        assert trim_entity(Circle((0, 0), 5), (0, 5), [Line((0, 0), (10, 0))]) is None


class TestTrimPolyline:
    ELBOW = Polyline([(0, 0, 0.0), (10, 0, 0.0), (10, 10, 0.0)], layer="P")

    def test_trimming_the_end_leaves_one_polyline(self):
        (rest,) = trim_entity(self.ELBOW, (8, 0), [VERTICAL_AT_5])
        assert points(rest) == [(0, 0), (5, 0)]
        assert rest.layer == "P" and not rest.closed

    def test_the_middle_goes_and_two_polylines_stay(self):
        edges = [VERTICAL_AT_5, Line((8, 5), (12, 5))]
        first, second = trim_entity(self.ELBOW, (10, 0), edges)
        assert points(first) == [(0, 0), (5, 0)]
        assert points(second) == [(10, 5), (10, 10)]

    def test_a_cut_exactly_at_a_vertex_counts_once(self):
        (rest,) = trim_entity(self.ELBOW, (3, 0), [Line((9, -1), (11, 1))])
        assert points(rest) == [(10, 0), (10, 10)]

    def test_closed_polyline_opens_up(self):
        square = Polyline([(0, 0, 0.0), (10, 0, 0.0), (10, 10, 0.0), (0, 10, 0.0)], closed=True)
        (rest,) = trim_entity(square, (10, 5), [Line((5, -5), (5, 15))])
        assert not rest.closed
        assert points(rest) == [(5, 10), (0, 10), (0, 0), (5, 0)]

    def test_polylines_with_curved_segments_are_not_trimmed_yet(self):
        assert trim_entity(Polyline([(0, 0, 0.5), (10, 0, 0.0)]), (8, 0), [VERTICAL_AT_5]) is None

    def test_other_entities_cannot_be_trimmed(self):
        assert trim_entity(Text((0, 0), "x", 1.0), (0, 0), [VERTICAL_AT_5]) is None


class TestExtend:
    SHORT = Line((0, 0), (5, 0), layer="W")

    def test_the_end_near_the_click_runs_on_to_the_boundary(self):
        assert extend_entity(self.SHORT, (4, 0), [Line((10, -5), (10, 5))]) == Line((0, 0), (10, 0), layer="W")

    def test_the_start_can_be_extended_too(self):
        assert extend_entity(self.SHORT, (1, 0), [Line((-3, -5), (-3, 5))]) == Line((-3, 0), (5, 0), layer="W")

    def test_no_boundary_beyond_that_end_means_no_extension(self):
        assert extend_entity(self.SHORT, (1, 0), [Line((10, -5), (10, 5))]) is None

    def test_the_nearest_boundary_wins_and_one_already_touching_is_skipped(self):
        edges = [Line((20, -5), (20, 5)), Line((10, -5), (10, 5)), Line((5, -5), (5, 5))]
        assert extend_entity(self.SHORT, (4, 0), edges) == Line((0, 0), (10, 0), layer="W")

    def test_a_boundary_too_short_to_be_reached_is_ignored(self):
        assert extend_entity(self.SHORT, (4, 0), [Line((10, 1), (10, 5))]) is None

    def test_the_object_itself_is_not_a_boundary(self):
        assert extend_entity(self.SHORT, (4, 0), [self.SHORT]) is None

    def test_extends_to_a_circle(self):
        assert extend_entity(self.SHORT, (4, 0), [Circle((10, 0), 2)]) == Line((0, 0), (8, 0), layer="W")

    def test_arc_end_runs_round_to_the_boundary(self):
        arc = Arc((0, 0), 5, 0, 90)
        boundary = Line((-3, -10), (-3, 10))
        longer = extend_entity(arc, (0, 5), [boundary])
        assert (longer.start_angle, longer.end_angle) == pytest.approx((0, math.degrees(math.atan2(4, -3))))

    def test_arc_start_runs_back_to_the_boundary(self):
        longer = extend_entity(Arc((0, 0), 5, 0, 90), (5, 0), [Line((-3, -10), (-3, 10))])
        assert longer.end_angle == 90
        assert longer.start_angle == pytest.approx(math.degrees(math.atan2(-4, -3)) % 360)

    def test_open_polyline_extends_its_end_segment(self):
        polyline = Polyline([(0, 0, 0.0), (5, 0, 0.0)])
        longer = extend_entity(polyline, (5, 0), [Line((8, -5), (8, 5))])
        assert points(longer) == [(0, 0), (8, 0)]
        shifted = extend_entity(polyline, (0, 0), [Line((-2, -5), (-2, 5))])
        assert points(shifted) == [(-2, 0), (5, 0)]

    def test_what_cannot_be_extended(self):
        edge = [Line((20, -5), (20, 5))]
        assert extend_entity(Circle((0, 0), 5), (5, 0), edge) is None
        assert extend_entity(Text((0, 0), "x", 1.0), (0, 0), edge) is None
        closed = Polyline([(0, 0, 0.0), (5, 0, 0.0), (5, 5, 0.0)], closed=True)
        assert extend_entity(closed, (5, 0), edge) is None
        assert extend_entity(self.SHORT, (4, 0), []) is None


class TestFillet:
    HORIZONTAL = Line((0, 0), (10, 0), layer="A", color=1)
    VERTICAL = Line((0, 0), (0, 10))

    def test_a_rounded_corner_between_square_lines(self):
        result = fillet_lines(self.HORIZONTAL, (8, 0), self.VERTICAL, (0, 8), 2)
        assert result.first.start == pytest.approx((2, 0))
        assert result.first.end == (10, 0)
        assert (result.first.layer, result.first.color) == ("A", 1)
        assert result.second.start == pytest.approx((0, 2))
        assert result.second.end == (0, 10)
        arc = result.arc
        assert arc.center == pytest.approx((2, 2))
        assert (arc.radius, arc.layer, arc.color) == (2, "A", 1)
        assert (arc.start_angle, arc.end_angle) == pytest.approx((180, 270))

    def test_radius_zero_makes_a_sharp_corner(self):
        result = fillet_lines(self.HORIZONTAL, (8, 0), self.VERTICAL, (0, 8), 0)
        assert result.arc is None
        assert result.first == Line((0, 0), (10, 0), layer="A", color=1)

    def test_crossing_lines_keep_the_sides_that_were_picked(self):
        across, down = Line((-5, 0), (5, 0)), Line((0, -5), (0, 5))
        right_up = fillet_lines(across, (4, 0), down, (0, 4), 0)
        assert (right_up.first, right_up.second) == (Line((0, 0), (5, 0)), Line((0, 0), (0, 5)))
        left_down = fillet_lines(across, (-4, 0), down, (0, -4), 0)
        assert (left_down.first, left_down.second) == (Line((-5, 0), (0, 0)), Line((0, -5), (0, 0)))

    def test_the_arc_goes_in_the_corner_that_was_picked(self):
        across, down = Line((-5, 0), (5, 0)), Line((0, -5), (0, 5))
        arc = fillet_lines(across, (4, 0), down, (0, -4), 1).arc
        assert arc.center == pytest.approx((1, -1))
        assert (arc.start_angle, arc.end_angle) == pytest.approx((90, 180))

    def test_lines_that_do_not_reach_each_other_are_extended(self):
        short, upright = Line((0, 0), (4, 0)), Line((6, -3), (6, 3))
        result = fillet_lines(short, (3, 0), upright, (6, 2), 0)
        assert result.first == Line((0, 0), (6, 0))
        assert result.second == Line((6, 0), (6, 3))

    def test_a_sharp_angle(self):
        diagonal = Line((0, 0), (10, 10))
        result = fillet_lines(self.HORIZONTAL, (8, 0), diagonal, (8, 8), 1)
        assert result.arc.center == pytest.approx((1 + math.sqrt(2), 1))
        assert result.first.start == pytest.approx((1 + math.sqrt(2), 0))

    def test_parallel_lines(self):
        with pytest.raises(FilletError, match="parallel"):
            fillet_lines(self.HORIZONTAL, (5, 0), Line((0, 3), (10, 3)), (5, 3), 1)

    def test_a_line_too_short_for_the_radius(self):
        with pytest.raises(FilletError, match="too short"):
            fillet_lines(Line((0, 0), (3, 0)), (2, 0), self.VERTICAL, (0, 8), 5)

    def test_a_line_with_no_length(self):
        with pytest.raises(FilletError, match="no length"):
            fillet_lines(Line((1, 1), (1, 1)), (1, 1), self.VERTICAL, (0, 8), 1)


