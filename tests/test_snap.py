import math

import pytest

from linewright.model.entities import Arc, Circle, Line, Polyline
from linewright.snap.intersect import intersections
from linewright.snap.osnap import find_osnap
from linewright.snap.primitives import distance_to, on_arc, primitives
from linewright.snap.resolve import SnapScene, SnapSettings, resolve_point
from linewright.snap.tracking import constrain_ortho, constrain_polar, snap_to_grid


def sorted_points(points):
    return sorted((round(x, 9), round(y, 9)) for x, y in points)


class TestPrimitives:
    def test_closed_polyline_splits_into_one_segment_per_side(self):
        square = Polyline([(0, 0, 0.0), (4, 0, 0.0), (4, 4, 0.0), (0, 4, 0.0)], closed=True)
        assert len(primitives(square)) == 4
        assert primitives(square)[-1] == Line((0, 4), (0, 0))

    def test_open_polyline_has_one_segment_fewer_than_vertices(self):
        assert len(primitives(Polyline([(0, 0, 0.0), (1, 0, 0.0), (1, 1, 0.0)]))) == 2

    def test_positive_bulge_becomes_a_counter_clockwise_arc_below_the_chord(self):
        (arc,) = primitives(Polyline([(0, 0, 1.0), (2, 0, 0.0)]))
        assert isinstance(arc, Arc)
        assert arc.center == pytest.approx((1, 0))
        assert (arc.start_angle, arc.end_angle) == pytest.approx((180, 0))
        assert on_arc(arc, (1, -1))
        assert not on_arc(arc, (1, 1))

    def test_negative_bulge_arc_is_stored_counter_clockwise_above_the_chord(self):
        (arc,) = primitives(Polyline([(0, 0, -1.0), (2, 0, 0.0)]))
        assert (arc.start_angle, arc.end_angle) == pytest.approx((0, 180))
        assert on_arc(arc, (1, 1))

    def test_arc_across_zero_degrees(self):
        arc = Arc((0, 0), 5, 350, 10)
        assert on_arc(arc, (5, 0))
        assert on_arc(arc, (5, 0.5))
        assert not on_arc(arc, (-5, 0))

    def test_distance_to_each_kind(self):
        assert distance_to(Line((0, 0), (10, 0)), (5, 3)) == pytest.approx(3)
        assert distance_to(Line((0, 0), (10, 0)), (13, 4)) == pytest.approx(5)
        assert distance_to(Circle((0, 0), 5), (8, 0)) == pytest.approx(3)
        assert distance_to(Arc((0, 0), 5, 0, 90), (0, -2)) == pytest.approx(math.hypot(5, 2))

    def test_unsupported_entities_have_no_primitives(self):
        assert primitives(object()) == []


class TestIntersections:
    def test_crossing_lines(self):
        points = intersections(Line((0, 0), (10, 10)), Line((0, 10), (10, 0)))
        assert points == [pytest.approx((5, 5))]

    def test_parallel_and_collinear_lines_do_not_intersect(self):
        assert intersections(Line((0, 0), (10, 0)), Line((0, 1), (10, 1))) == []
        assert intersections(Line((0, 0), (10, 0)), Line((20, 0), (30, 0))) == []

    def test_segments_that_would_cross_if_extended_do_not(self):
        assert intersections(Line((0, 0), (1, 1)), Line((0, 10), (10, 0))) == []

    def test_t_junction_touches_at_the_end_of_one_line(self):
        points = intersections(Line((0, 0), (10, 0)), Line((5, 0), (5, 5)))
        assert points == [pytest.approx((5, 0))]

    def test_line_through_a_circle(self):
        points = intersections(Line((-10, 0), (10, 0)), Circle((0, 0), 5))
        assert sorted_points(points) == [(-5, 0), (5, 0)]

    def test_line_tangent_to_a_circle(self):
        points = intersections(Line((-10, 5), (10, 5)), Circle((0, 0), 5))
        assert sorted_points(points) == [(0, 5)]

    def test_line_that_misses_a_circle(self):
        assert intersections(Line((-10, 6), (10, 6)), Circle((0, 0), 5)) == []

    def test_segment_ending_inside_a_circle_hits_it_once(self):
        points = intersections(Line((0, 0), (10, 0)), Circle((0, 0), 5))
        assert sorted_points(points) == [(5, 0)]

    def test_two_overlapping_circles(self):
        points = intersections(Circle((0, 0), 5), Circle((6, 0), 5))
        assert sorted_points(points) == [(3, -4), (3, 4)]

    def test_externally_tangent_circles(self):
        assert sorted_points(intersections(Circle((0, 0), 1), Circle((2, 0), 1))) == [(1, 0)]

    @pytest.mark.parametrize(
        "second", [Circle((0, 0), 3), Circle((20, 0), 1), Circle((1, 0), 1.5)], ids=["concentric", "apart", "nested"]
    )
    def test_circles_that_do_not_cross(self, second):
        assert intersections(Circle((0, 0), 5), second) == []

    def test_only_the_part_of_the_circle_that_is_an_arc_counts(self):
        arc = Arc((0, 0), 5, 0, 90)
        points = intersections(Line((-10, 3), (10, 3)), arc)
        assert sorted_points(points) == [(4, 3)]

    def test_arc_and_arc(self):
        points = intersections(Arc((0, 0), 5, 0, 180), Arc((6, 0), 5, 90, 270))
        assert sorted_points(points) == [(3, 4)]


class TestObjectSnap:
    HORIZONTAL = Line((0, 0), (10, 0))

    def test_endpoint(self):
        hit = find_osnap((0.5, 0.3), 1.0, [self.HORIZONTAL], None)
        assert (hit.kind, hit.point) == ("endpoint", (0, 0))

    def test_midpoint(self):
        hit = find_osnap((5.2, 0.2), 1.0, [self.HORIZONTAL], None)
        assert (hit.kind, hit.point) == ("midpoint", (5, 0))

    def test_center_of_a_circle_even_though_the_cursor_is_far_from_the_circle(self):
        hit = find_osnap((3.2, 2.9), 1.0, [Circle((3, 3), 2)], None)
        assert (hit.kind, hit.point) == ("center", (3, 3))

    def test_center_and_midpoint_and_endpoints_of_an_arc(self):
        arc = Arc((0, 0), 5, 0, 90)
        kinds = {
            "endpoint": find_osnap((5.1, 0.1), 1.0, [arc], None),
            "midpoint": find_osnap((3.5, 3.6), 1.0, [arc], None),
            "center": find_osnap((0.1, 0.1), 1.0, [arc], None),
        }
        assert {k: v.kind for k, v in kinds.items()} == {k: k for k in kinds}
        assert kinds["midpoint"].point == pytest.approx((5 * math.cos(math.pi / 4), 5 * math.sin(math.pi / 4)))

    def test_intersection(self):
        lines = [Line((0, 0), (20, 20)), Line((0, 10), (20, 0))]
        hit = find_osnap((6.7, 6.6), 1.0, lines, None)
        assert hit.kind == "intersection"
        assert hit.point == pytest.approx((20 / 3, 20 / 3))

    def test_intersection_of_a_line_and_a_circle(self):
        hit = find_osnap((5.1, 0.1), 1.0, [Line((-10, 0), (3, 0)), Circle((0, 0), 5)], None)
        assert hit is None  # the line ends inside the circle, so there is no crossing

        hit = find_osnap((5.1, 0.1), 1.0, [Line((-10, 0), (9, 0)), Circle((0, 0), 5)], None)
        assert (hit.kind, hit.point) == ("intersection", pytest.approx((5, 0)))

    def test_nothing_in_range(self):
        assert find_osnap((50, 50), 1.0, [self.HORIZONTAL, Circle((3, 3), 2)], None) is None

    def test_the_closest_candidate_wins(self):
        lines = [Line((0, 0), (10, 0)), Line((0.8, 0), (10, 5))]
        assert find_osnap((0.7, 0.1), 2.0, lines, None).point == (0.8, 0)

    def test_endpoint_beats_midpoint_at_the_same_spot(self):
        # (0,0) is an endpoint of the first line and the midpoint of the second
        lines = [Line((0, 0), (4, 0)), Line((-4, 0), (4, 0))]
        assert find_osnap((0.1, 0), 1.0, lines, None).kind == "endpoint"

    def test_polyline_vertices_and_segment_midpoints(self):
        square = Polyline([(0, 0, 0.0), (10, 0, 0.0), (10, 10, 0.0), (0, 10, 0.0)], closed=True)
        assert find_osnap((10.2, 9.9), 1.0, [square], None).point == (10, 10)
        assert find_osnap((0.2, 5.1), 1.0, [square], None) == find_osnap((0.0, 5.0), 1.0, [square], None)
        assert find_osnap((0.2, 5.1), 1.0, [square], None).kind == "midpoint"

    def test_polyline_arc_segment_has_a_center(self):
        rounded = Polyline([(0, 0, 1.0), (2, 0, 0.0)])
        assert find_osnap((1.1, 0.1), 0.5, [rounded], None).kind == "center"

    def test_perpendicular_to_a_line_needs_a_base_point(self):
        assert find_osnap((7, 0.3), 1.0, [self.HORIZONTAL], None) is None
        hit = find_osnap((7, 0.3), 1.0, [self.HORIZONTAL], (4, 5))
        assert (hit.kind, hit.point) == ("perpendicular", pytest.approx((4, 0)))

    def test_perpendicular_foot_must_lie_on_the_line_segment(self):
        assert find_osnap((3, 0.1), 1.0, [self.HORIZONTAL], (-5, 3)) is None

    def test_perpendicular_to_a_circle_picks_the_foot_nearest_the_cursor(self):
        hit = find_osnap((4.9, 0.3), 1.0, [Circle((0, 0), 5)], (10, 0))
        assert (hit.kind, hit.point) == ("perpendicular", pytest.approx((5, 0)))
        hit = find_osnap((-4.9, 0.3), 1.0, [Circle((0, 0), 5)], (10, 0))
        assert hit.point == pytest.approx((-5, 0))

    def test_perpendicular_is_only_used_when_nothing_else_is_in_range(self):
        hit = find_osnap((0.2, 0.1), 1.0, [self.HORIZONTAL], (4, 5))
        assert hit.kind == "endpoint"


class TestTracking:
    def test_ortho_keeps_the_dominant_axis(self):
        assert constrain_ortho((0, 0), (10, 3)) == (10, 0)
        assert constrain_ortho((0, 0), (2, 9)) == (0, 9)
        assert constrain_ortho((5, 5), (-4, 6)) == (-4, 5)

    def test_polar_pulls_the_cursor_onto_an_increment_ray(self):
        angle = math.radians(3)
        point = constrain_polar((0, 0), (10 * math.cos(angle), 10 * math.sin(angle)), 45)
        assert point == pytest.approx((10 * math.cos(angle), 0), abs=1e-9)

    def test_polar_leaves_the_cursor_alone_outside_the_tolerance(self):
        angle = math.radians(20)
        raw = (10 * math.cos(angle), 10 * math.sin(angle))
        assert constrain_polar((0, 0), raw, 45) == raw

    def test_polar_on_the_diagonal_and_across_the_180_degree_seam(self):
        diagonal = constrain_polar((1, 1), (11, 10.5), 45)
        assert diagonal[0] - 1 == pytest.approx(diagonal[1] - 1)
        seam = math.radians(-179)
        raw = (10 * math.cos(seam), 10 * math.sin(seam))
        assert constrain_polar((0, 0), raw, 90)[1] == pytest.approx(0, abs=1e-9)

    def test_polar_at_the_base_point_is_unchanged(self):
        assert constrain_polar((3, 3), (3, 3), 45) == (3, 3)

    def test_grid_rounds_each_coordinate(self):
        assert snap_to_grid((13, -4), 10) == (10, 0)
        assert snap_to_grid((0.26, 0.74), 0.5) == (0.5, 0.5)


class TestResolve:
    SCENE = SnapScene(aperture=1.0, grid_step=10.0, entities=[Line((0, 0), (10, 0))])

    def test_object_snap_beats_ortho_and_grid(self):
        settings = SnapSettings(ortho=True, grid=True)
        result = resolve_point((0.4, 0.3), (5, 5), settings, self.SCENE)
        assert (result.point, result.kind) == ((0, 0), "endpoint")

    def test_ortho_and_grid_apply_when_no_object_snap_is_in_range(self):
        settings = SnapSettings(osnap=False, ortho=True, grid=True)
        assert resolve_point((23, 7), (0, 0), settings, self.SCENE).point == (20, 0)

    def test_ortho_and_polar_do_nothing_without_a_base_point(self):
        settings = SnapSettings(osnap=False, ortho=True)
        assert resolve_point((23, 7), None, settings, self.SCENE).point == (23, 7)

    def test_turning_on_ortho_turns_off_polar_and_the_other_way_round(self):
        settings = SnapSettings(polar=True)
        settings.set("ortho", True)
        assert (settings.ortho, settings.polar) == (True, False)
        settings.set("polar", True)
        assert (settings.ortho, settings.polar) == (False, True)
        settings.set("polar", False)
        assert not settings.polar

    def test_object_snap_can_be_turned_off(self):
        settings = SnapSettings(osnap=False)
        result = resolve_point((0.4, 0.3), None, settings, self.SCENE)
        assert (result.point, result.kind) == ((0.4, 0.3), None)
