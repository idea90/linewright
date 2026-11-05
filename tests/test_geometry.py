import math

import pytest

from linewright.model.geometry import arc_through_points, bulge_arc


def point_at(arc, angle):
    return (
        arc.center[0] + arc.radius * math.cos(angle),
        arc.center[1] + arc.radius * math.sin(angle),
    )


def test_positive_bulge_semicircle_bows_to_the_right_of_the_chord():
    arc = bulge_arc((0, 0), (2, 0), 1.0)
    assert arc.center == pytest.approx((1, 0))
    assert arc.radius == pytest.approx(1)
    assert point_at(arc, arc.start_angle + arc.sweep / 2) == pytest.approx((1, -1))


def test_negative_bulge_bows_to_the_left_of_the_chord():
    arc = bulge_arc((0, 0), (2, 0), -1.0)
    assert arc.sweep == pytest.approx(-math.pi)
    assert point_at(arc, arc.start_angle + arc.sweep / 2) == pytest.approx((1, 1))


@pytest.mark.parametrize("bulge", [0.1, 0.5, -0.3, 2.0, -4.0])
def test_arc_runs_from_start_to_end(bulge):
    start, end = (1.0, 2.0), (7.0, -3.0)
    arc = bulge_arc(start, end, bulge)
    assert point_at(arc, arc.start_angle) == pytest.approx(start)
    assert point_at(arc, arc.start_angle + arc.sweep) == pytest.approx(end)


def test_arc_through_points_far_from_the_origin_stays_accurate():
    spec = arc_through_points((1e6 + 10, 1e6), (1e6, 1e6 + 10), (1e6 - 10, 1e6))
    assert spec.center == pytest.approx((1e6, 1e6), abs=1e-6)
    assert spec.radius == pytest.approx(10, abs=1e-6)


def test_arc_through_collinear_or_repeated_points_is_none():
    assert arc_through_points((0, 0), (1, 1), (2, 2)) is None
    assert arc_through_points((0, 0), (0, 0), (2, 2)) is None
