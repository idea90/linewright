import math
from typing import NamedTuple

from linewright.model.entities import Circle, Entity, Polyline
from linewright.model.geometry import Point, bulge_arc

Vertex = tuple[float, float, float]  # x, y, bulge of the segment that starts here


class Measured(NamedTuple):
    area: float
    perimeter: float  # the circumference for a circle


def distance_report(first: Point, second: Point) -> str:
    dx, dy = second[0] - first[0], second[1] - first[1]
    angle = math.degrees(math.atan2(dy, dx)) % 360.0
    length = math.hypot(dx, dy)
    return f"Distance = {length:.4f}, Angle in XY Plane = {angle:.0f}, Delta X = {dx:.4f}, Delta Y = {dy:.4f}"


def _segment(start: Vertex, end: Vertex) -> tuple[float, float]:
    """Signed area under the segment (counter-clockwise positive, with the bulge's cap) and its length."""
    cross = (start[0] * end[1] - end[0] * start[1]) / 2
    bulge = start[2]
    if not bulge:
        return cross, math.dist(start[:2], end[:2])
    arc = bulge_arc(start[:2], end[:2], bulge)
    cap = arc.radius ** 2 / 2 * (arc.sweep - math.sin(arc.sweep))
    return cross + cap, arc.radius * abs(arc.sweep)


def polygon(vertices: list[Vertex]) -> Measured | None:
    """The area and perimeter of the shape the vertices enclose when the last joins back to the first."""
    if len(vertices) < 2:
        return None
    area = perimeter = 0.0
    for index, start in enumerate(vertices):
        segment_area, length = _segment(start, vertices[(index + 1) % len(vertices)])
        area += segment_area
        perimeter += length
    return Measured(abs(area), perimeter)


def measure_entity(entity: Entity) -> Measured | None:
    """Area and perimeter of a circle or closed polyline, or None for anything else."""
    if isinstance(entity, Circle):
        return Measured(math.pi * entity.radius**2, 2 * math.pi * entity.radius)
    if isinstance(entity, Polyline) and entity.closed:
        return polygon(entity.vertices)
    return None


def area_report(measured: Measured) -> str:
    return f"Area = {measured.area:.4f}, Perimeter = {measured.perimeter:.4f}"
