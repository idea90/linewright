import math
from dataclasses import replace

from linewright.model.entities import Arc, Circle, Entity, Line, Polyline
from linewright.model.geometry import Point
from linewright.snap.intersect import line_line_meeting
from linewright.snap.primitives import distance_to


def offset_entity(entity: Entity, distance: float, side: Point) -> Entity | None:
    """A copy moved sideways by the distance, toward the side point. None if it cannot be offset."""
    if isinstance(entity, Line):
        return _offset_line(entity, distance, side)
    if isinstance(entity, (Circle, Arc)):
        return _offset_round(entity, distance, side)
    if isinstance(entity, Polyline):
        return _offset_polyline(entity, distance, side)
    return None


def _left_normal(start: Point, end: Point) -> Point:
    length = math.dist(start, end)
    return (-(end[1] - start[1]) / length, (end[0] - start[0]) / length)


def _side_sign(start: Point, end: Point, side: Point) -> float:
    normal = _left_normal(start, end)
    return 1.0 if (side[0] - start[0]) * normal[0] + (side[1] - start[1]) * normal[1] >= 0 else -1.0


def _shifted(start: Point, end: Point, amount: float) -> Line:
    normal = _left_normal(start, end)
    shift = (normal[0] * amount, normal[1] * amount)
    return Line((start[0] + shift[0], start[1] + shift[1]), (end[0] + shift[0], end[1] + shift[1]))


def _offset_line(line: Line, distance: float, side: Point) -> Entity | None:
    if line.start == line.end:
        return None
    moved = _shifted(line.start, line.end, distance * _side_sign(line.start, line.end, side))
    return replace(line, start=moved.start, end=moved.end)


def _offset_round(entity: Circle | Arc, distance: float, side: Point) -> Entity | None:
    inside = math.dist(entity.center, side) < entity.radius
    radius = entity.radius - distance if inside else entity.radius + distance
    return replace(entity, radius=radius) if radius > 0 else None


def _distinct_vertices(polyline: Polyline) -> list[Point]:
    points: list[Point] = []
    for x, y, _ in polyline.vertices:
        if not points or (x, y) != points[-1]:
            points.append((x, y))
    if polyline.closed and len(points) > 1 and points[0] == points[-1]:
        points.pop()
    return points


def _segments(points: list[Point], closed: bool) -> list[tuple[Point, Point]]:
    count = len(points) if closed else len(points) - 1
    return [(points[i], points[(i + 1) % len(points)]) for i in range(count)]


def _offset_polyline(polyline: Polyline, distance: float, side: Point) -> Entity | None:
    # shortcut: straight segments only, joined with sharp corners and no trimming of loops that
    # collapse; curved segments need arc offsetting and trimming (add with Trim/Fillet).
    if any(bulge for _, _, bulge in polyline.vertices):
        return None
    points = _distinct_vertices(polyline)
    if len(points) < 2:
        return None
    segments = _segments(points, polyline.closed)
    nearest = min(segments, key=lambda s: distance_to(Line(*s), side))
    amount = distance * _side_sign(*nearest, side)
    moved = [_shifted(start, end, amount) for start, end in segments]
    corners = [_corner(moved[i - 1], moved[i]) for i in range(1, len(moved))]
    if polyline.closed:
        vertices = [_corner(moved[-1], moved[0]), *corners]
    else:
        vertices = [moved[0].start, *corners, moved[-1].end]
    return replace(polyline, vertices=[(x, y, 0.0) for x, y in vertices])


def _corner(before: Line, after: Line) -> Point:
    """Where two offset segments meet. Collinear segments just keep the end of the first."""
    meeting = line_line_meeting(before, after)
    return meeting[0] if meeting else before.end
