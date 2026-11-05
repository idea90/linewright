import math
from dataclasses import dataclass, replace

from linewright.model.entities import Arc, Line
from linewright.model.geometry import Point
from linewright.snap.intersect import line_line_meeting

EPSILON = 1e-9


class FilletError(ValueError):
    """The two lines cannot be joined that way. The message is meant for the user."""


@dataclass(frozen=True)
class FilletResult:
    first: Line
    second: Line
    arc: Arc | None  # None when the radius is zero: the lines just meet at the corner


def _dot(a: Point, b: Point) -> float:
    return a[0] * b[0] + a[1] * b[1]


def _minus(a: Point, b: Point) -> Point:
    return (a[0] - b[0], a[1] - b[1])


def _toward_pick(line: Line, pick: Point, corner: Point) -> Point:
    """The unit direction from the corner along the line, toward the side that was picked."""
    length = math.dist(line.start, line.end)
    unit = ((line.end[0] - line.start[0]) / length, (line.end[1] - line.start[1]) / length)
    side = _dot(_minus(pick, corner), unit)
    if abs(side) < EPSILON:
        reaches = [_dot(_minus(end, corner), unit) for end in (line.start, line.end)]
        side = max(reaches, key=abs)
    sign = 1.0 if side >= 0 else -1.0
    return (unit[0] * sign, unit[1] * sign)


def _keep_side(line: Line, corner: Point, toward: Point, tangent: Point, name: str) -> Line:
    """The line cut back to the tangent point, keeping the end that lies in the picked direction."""
    ends = [line.start, line.end]
    reach = [_dot(_minus(end, corner), toward) for end in ends]
    far = 0 if reach[0] >= reach[1] else 1
    if reach[far] <= _dot(_minus(tangent, corner), toward) + EPSILON:
        raise FilletError(f"The {name} line is too short for that radius.")
    return replace(line, start=tangent, end=line.end) if far == 1 else replace(line, start=line.start, end=tangent)


def fillet_lines(first: Line, first_pick: Point, second: Line, second_pick: Point, radius: float) -> FilletResult:
    """Join two lines with a corner or, for a radius above zero, a rounded corner. Both are cut back to fit."""
    if first.start == first.end or second.start == second.end:
        raise FilletError("A line has no length.")
    meeting = line_line_meeting(first, second)
    if not meeting:
        raise FilletError("The lines are parallel.")
    corner = meeting[0]
    toward_first = _toward_pick(first, first_pick, corner)
    toward_second = _toward_pick(second, second_pick, corner)
    angle = math.acos(max(-1.0, min(1.0, _dot(toward_first, toward_second))))
    if angle < EPSILON or angle > math.pi - EPSILON:
        raise FilletError("The lines are parallel.")
    tangent_length = radius / math.tan(angle / 2)
    tangent_first = (corner[0] + toward_first[0] * tangent_length, corner[1] + toward_first[1] * tangent_length)
    tangent_second = (corner[0] + toward_second[0] * tangent_length, corner[1] + toward_second[1] * tangent_length)
    new_first = _keep_side(first, corner, toward_first, tangent_first, "first")
    new_second = _keep_side(second, corner, toward_second, tangent_second, "second")
    if radius == 0:
        return FilletResult(new_first, new_second, None)
    bisector = (toward_first[0] + toward_second[0], toward_first[1] + toward_second[1])
    scale = radius / math.sin(angle / 2) / math.hypot(*bisector)
    center = (corner[0] + bisector[0] * scale, corner[1] + bisector[1] * scale)
    arc = _arc_between(center, radius, tangent_first, tangent_second)
    return FilletResult(new_first, new_second, replace(arc, layer=first.layer, color=first.color))


def _arc_between(center: Point, radius: float, tangent_first: Point, tangent_second: Point) -> Arc:
    """The short arc from one tangent point to the other, as a counter-clockwise arc."""
    a, b = _minus(tangent_first, center), _minus(tangent_second, center)
    first_angle = math.degrees(math.atan2(a[1], a[0])) % 360.0
    second_angle = math.degrees(math.atan2(b[1], b[0])) % 360.0
    counter_clockwise = a[0] * b[1] - a[1] * b[0] > 0
    start, end = (first_angle, second_angle) if counter_clockwise else (second_angle, first_angle)
    return Arc(center, radius, start, end)



