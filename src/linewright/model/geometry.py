import math
from typing import NamedTuple

Point = tuple[float, float]


class BulgeArc(NamedTuple):
    center: Point
    radius: float
    start_angle: float  # radians, counter-clockwise from +X
    sweep: float  # radians, positive counter-clockwise


class ThreePointArc(NamedTuple):
    center: Point
    radius: float
    start_angle: float  # degrees, counter-clockwise from +X
    end_angle: float


def arc_through_points(first: Point, second: Point, third: Point) -> ThreePointArc | None:
    """Counter-clockwise arc that runs through the three points in order, or None if they are in a line."""
    bx, by = second[0] - first[0], second[1] - first[1]
    cx, cy = third[0] - first[0], third[1] - first[1]
    turn = bx * (cy - by) - by * (cx - bx)
    if abs(turn) <= 1e-9 * math.hypot(bx, by) * math.hypot(cx - bx, cy - by):
        return None
    b_sq, c_sq = bx * bx + by * by, cx * cx + cy * cy
    d = 2 * (bx * cy - by * cx)
    center = (first[0] + (cy * b_sq - by * c_sq) / d, first[1] + (bx * c_sq - cx * b_sq) / d)

    def angle_to(point: Point) -> float:
        return math.degrees(math.atan2(point[1] - center[1], point[0] - center[0])) % 360.0

    radius = math.hypot(first[0] - center[0], first[1] - center[1])
    if turn > 0:
        return ThreePointArc(center, radius, angle_to(first), angle_to(third))
    return ThreePointArc(center, radius, angle_to(third), angle_to(first))


def bulge_arc(start: Point, end: Point, bulge: float) -> BulgeArc:
    """Arc through start and end for a DXF polyline bulge (tan of a quarter of the sweep)."""
    dx = end[0] - start[0]
    dy = end[1] - start[1]
    chord = math.hypot(dx, dy)
    offset = (chord / 2) * (1 - bulge * bulge) / (2 * bulge)
    center = (
        (start[0] + end[0]) / 2 - dy / chord * offset,
        (start[1] + end[1]) / 2 + dx / chord * offset,
    )
    radius = (chord / 2) * (1 + bulge * bulge) / (2 * abs(bulge))
    start_angle = math.atan2(start[1] - center[1], start[0] - center[0])
    return BulgeArc(center, radius, start_angle, 4 * math.atan(bulge))
