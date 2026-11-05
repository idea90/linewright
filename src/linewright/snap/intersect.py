import math

from linewright.model.entities import Line
from linewright.model.geometry import Point
from linewright.snap.primitives import Primitive, on_primitive

TOLERANCE = 1e-9


def intersections(first: Primitive, second: Primitive) -> list[Point]:
    """Points where two lines, arcs or circles cross, limited to the parts that actually exist."""
    if isinstance(first, Line) and isinstance(second, Line):
        points = line_line_meeting(first, second)
    elif isinstance(first, Line):
        points = _line_circle(first, second.center, second.radius)
    elif isinstance(second, Line):
        points = _line_circle(second, first.center, first.radius)
    else:
        points = _circle_circle(first.center, first.radius, second.center, second.radius)
    return [p for p in points if on_primitive(first, p) and on_primitive(second, p)]


def line_line_meeting(first: Line, second: Line) -> list[Point]:
    """Where the two lines would meet if extended without end (nothing if parallel)."""
    d1 = (first.end[0] - first.start[0], first.end[1] - first.start[1])
    d2 = (second.end[0] - second.start[0], second.end[1] - second.start[1])
    denominator = d1[0] * d2[1] - d1[1] * d2[0]
    if abs(denominator) <= TOLERANCE * math.hypot(*d1) * math.hypot(*d2):
        return []
    gap = (second.start[0] - first.start[0], second.start[1] - first.start[1])
    along = (gap[0] * d2[1] - gap[1] * d2[0]) / denominator
    return [(first.start[0] + along * d1[0], first.start[1] + along * d1[1])]


def _line_circle(line: Line, center: Point, radius: float) -> list[Point]:
    dx, dy = line.end[0] - line.start[0], line.end[1] - line.start[1]
    fx, fy = line.start[0] - center[0], line.start[1] - center[1]
    a = dx * dx + dy * dy
    if a == 0:
        return []
    b = 2 * (fx * dx + fy * dy)
    c = fx * fx + fy * fy - radius * radius
    discriminant = b * b - 4 * a * c
    if discriminant < -TOLERANCE * max(b * b, 4 * a * abs(c)):
        return []
    root = math.sqrt(max(discriminant, 0.0))
    parameters = {(-b - root) / (2 * a), (-b + root) / (2 * a)}
    return [(line.start[0] + t * dx, line.start[1] + t * dy) for t in sorted(parameters)]


def _circle_circle(c1: Point, r1: float, c2: Point, r2: float) -> list[Point]:
    distance = math.dist(c1, c2)
    slack = TOLERANCE * (r1 + r2 + distance)
    if distance == 0 or distance > r1 + r2 + slack or distance < abs(r1 - r2) - slack:
        return []
    along = (r1 * r1 - r2 * r2 + distance * distance) / (2 * distance)
    height = math.sqrt(max(r1 * r1 - along * along, 0.0))
    ux, uy = (c2[0] - c1[0]) / distance, (c2[1] - c1[1]) / distance
    middle = (c1[0] + along * ux, c1[1] + along * uy)
    if height <= slack:
        return [middle]
    return [(middle[0] - height * uy, middle[1] + height * ux), (middle[0] + height * uy, middle[1] - height * ux)]
