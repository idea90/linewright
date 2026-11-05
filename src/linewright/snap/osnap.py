import math
from dataclasses import dataclass

from linewright.model.entities import Arc, Circle, Entity, Line
from linewright.model.geometry import Point
from linewright.snap.intersect import intersections
from linewright.snap.primitives import (
    Primitive,
    arc_point,
    arc_sweep,
    distance_to,
    line_parameter,
    on_primitive,
    primitives,
)

# Lower number wins when two snap points are equally close to the cursor.
KIND_PRIORITY = {"endpoint": 0, "intersection": 1, "center": 2, "midpoint": 3, "perpendicular": 4}
MAX_INTERSECTION_PRIMITIVES = 40


@dataclass(frozen=True)
class SnapPoint:
    point: Point
    kind: str


def find_osnap(cursor: Point, aperture: float, entities: list[Entity], base: Point | None) -> SnapPoint | None:
    """The object snap point nearest the cursor within the aperture, or None.

    A perpendicular snap needs a base point and is only used when nothing else is in range.
    """
    # shortcut: rebuilds every primitive on each cursor move, which is fine to a few thousand
    # entities; cache primitives per entity or add a spatial index beyond that.
    every = [primitive for entity in entities for primitive in primitives(entity)]
    near = [p for p in every if distance_to(p, cursor) <= aperture]
    found = _centers(every) + _ends_and_middles(near) + _crossings(near)
    best = _nearest(found, cursor, aperture)
    if best is not None or base is None:
        return best
    return _nearest_foot(near, cursor, base)


def _centers(every: list[Primitive]) -> list[SnapPoint]:
    return [SnapPoint(p.center, "center") for p in every if isinstance(p, (Arc, Circle))]


def _ends_and_middles(near: list[Primitive]) -> list[SnapPoint]:
    found: list[SnapPoint] = []
    for primitive in near:
        if isinstance(primitive, Line):
            middle = (
                (primitive.start[0] + primitive.end[0]) / 2,
                (primitive.start[1] + primitive.end[1]) / 2,
            )
            found += [
                SnapPoint(primitive.start, "endpoint"),
                SnapPoint(primitive.end, "endpoint"),
                SnapPoint(middle, "midpoint"),
            ]
        elif isinstance(primitive, Arc):
            found += [
                SnapPoint(arc_point(primitive, primitive.start_angle), "endpoint"),
                SnapPoint(arc_point(primitive, primitive.end_angle), "endpoint"),
                SnapPoint(arc_point(primitive, primitive.start_angle + arc_sweep(primitive) / 2), "midpoint"),
            ]
    return found


def _crossings(near: list[Primitive]) -> list[SnapPoint]:
    candidates = near[:MAX_INTERSECTION_PRIMITIVES]
    found: list[SnapPoint] = []
    for index, first in enumerate(candidates):
        for second in candidates[index + 1 :]:
            found += [SnapPoint(point, "intersection") for point in intersections(first, second)]
    return found


def _nearest(found: list[SnapPoint], cursor: Point, aperture: float) -> SnapPoint | None:
    in_range = [s for s in found if math.dist(s.point, cursor) <= aperture]
    if not in_range:
        return None
    return min(in_range, key=lambda s: (math.dist(s.point, cursor), KIND_PRIORITY[s.kind]))


def _nearest_foot(near: list[Primitive], cursor: Point, base: Point) -> SnapPoint | None:
    feet = [SnapPoint(foot, "perpendicular") for p in near for foot in _perpendicular_feet(p, base)]
    if not feet:
        return None
    return min(feet, key=lambda s: math.dist(s.point, cursor))


def _perpendicular_feet(primitive: Primitive, base: Point) -> list[Point]:
    """Points on the primitive where a line from base meets it at a right angle."""
    if isinstance(primitive, Line):
        parameter = line_parameter(primitive, base)
        if parameter is None:
            return []
        foot = (
            primitive.start[0] + parameter * (primitive.end[0] - primitive.start[0]),
            primitive.start[1] + parameter * (primitive.end[1] - primitive.start[1]),
        )
        return [foot] if on_primitive(primitive, foot) else []
    reach = math.dist(primitive.center, base)
    if reach == 0:
        return []
    ux = (base[0] - primitive.center[0]) / reach
    uy = (base[1] - primitive.center[1]) / reach
    radius = primitive.radius
    ends = [
        (primitive.center[0] + radius * ux, primitive.center[1] + radius * uy),
        (primitive.center[0] - radius * ux, primitive.center[1] - radius * uy),
    ]
    return [p for p in ends if on_primitive(primitive, p)]
