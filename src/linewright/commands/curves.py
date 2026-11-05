import math
from collections.abc import Callable

from linewright.commands.draw import line_preview
from linewright.commands.prompts import Command, Drawn, Prompt
from linewright.model.entities import Arc, Circle, Entity, Line
from linewright.model.geometry import Point, ThreePointArc, arc_through_points

DIAMETER = "D"
THREE_POINTS = "3P"
TWO_POINTS = "2P"
CENTER = "C"
END = "E"
IN_A_LINE = "The points are in a line."
NO_ARC = "That does not make an arc."


def _circle_preview(center: Point) -> Callable[[Point], list[Entity]]:
    return lambda cursor: [Circle(center, math.dist(center, cursor))]


def _diameter_circle(first: Point, second: Point) -> Circle:
    return Circle(((first[0] + second[0]) / 2, (first[1] + second[1]) / 2), math.dist(first, second) / 2)


def circle() -> Command:
    answer = yield Prompt(
        "point", "Specify center point for circle or [3P/2P]:", options=(THREE_POINTS, TWO_POINTS)
    )
    if answer == THREE_POINTS:
        yield from _circle_through_three_points()
    elif answer == TWO_POINTS:
        yield from _circle_on_diameter()
    elif answer is not None:
        yield from _circle_from_center(answer)


def _circle_from_center(center: Point) -> Command:
    radius = yield Prompt(
        "number", "Specify radius of circle or [Diameter]:", center, (DIAMETER,), preview=_circle_preview(center)
    )
    if radius == DIAMETER:
        # A click answers with twice its distance, so the circle always passes through the click.
        diameter = yield Prompt(
            "number", "Specify diameter of circle:", center, click_factor=2.0, preview=_circle_preview(center)
        )
        radius = None if diameter is None else diameter / 2
    if radius is not None:
        yield Drawn([Circle(center, radius)])


def _circle_on_diameter() -> Command:
    first = yield Prompt("point", "Specify first end point of circle's diameter:")
    if first is None:
        return
    message = "Specify second end point of circle's diameter:"
    while True:
        second = yield Prompt(
            "point", message, first, preview=lambda cursor: [_diameter_circle(first, cursor)]
        )
        if second is None:
            return
        if second != first:
            yield Drawn([_diameter_circle(first, second)])
            return
        message = "The end points are the same. Specify second end point of circle's diameter:"


def _whole_circle(spec: ThreePointArc) -> Circle:
    return Circle(spec.center, spec.radius)


def _three_point_preview(
    first: Point, second: Point, shape: Callable[[ThreePointArc], Entity]
) -> Callable[[Point], list[Entity]]:
    def preview(cursor: Point) -> list[Entity]:
        spec = arc_through_points(first, second, cursor)
        if spec is None:
            return [Line(first, second), Line(second, cursor)]
        return [shape(spec)]

    return preview


def _last_of_three_points(
    first: Point, second: Point, label: str, shape: Callable[[ThreePointArc], Entity]
) -> Command:
    """Ask for the third point until the three are not in a line, then draw the shape they define."""
    message = f"Specify {label}:"
    while True:
        third = yield Prompt("point", message, second, preview=_three_point_preview(first, second, shape))
        if third is None:
            return
        spec = arc_through_points(first, second, third)
        if spec is not None:
            yield Drawn([shape(spec)])
            return
        message = f"{IN_A_LINE} Specify {label}:"


def _circle_through_three_points() -> Command:
    first = yield Prompt("point", "Specify first point on circle:")
    if first is None:
        return
    second = yield Prompt("point", "Specify second point on circle:", first, preview=line_preview(first))
    if second is not None:
        yield from _last_of_three_points(first, second, "third point on circle", _whole_circle)


def _arc_about_center(center: Point, start: Point, end: Point) -> Arc | None:
    """The counter-clockwise arc around the center from the start point toward the end point's direction."""
    radius = math.dist(center, start)
    if radius == 0 or end == center:
        return None
    start_angle = math.degrees(math.atan2(start[1] - center[1], start[0] - center[0])) % 360.0
    end_angle = math.degrees(math.atan2(end[1] - center[1], end[0] - center[0])) % 360.0
    if math.isclose(start_angle, end_angle, abs_tol=1e-9):
        return None
    return Arc(center, radius, start_angle, end_angle)


def _about_center_preview(center: Point, start: Point) -> Callable[[Point], list[Entity]]:
    def preview(cursor: Point) -> list[Entity]:
        arc = _arc_about_center(center, start, cursor)
        return [arc] if arc else [Line(center, start)]

    return preview


def arc() -> Command:
    start = yield Prompt("point", "Specify start point of arc or [Center]:", options=(CENTER,))
    if start == CENTER:
        yield from _arc_from_center()
    elif start is not None:
        yield from _arc_from_start(start)


def _arc_from_start(start: Point) -> Command:
    second = yield Prompt(
        "point",
        "Specify second point of arc or [Center/End]:",
        start,
        (CENTER, END),
        preview=line_preview(start),
    )
    if second == CENTER:
        yield from _arc_start_center_end(start)
    elif second == END:
        yield from _arc_start_end_center(start)
    elif second is not None:
        yield from _last_of_three_points(start, second, "end point of arc", lambda spec: Arc(*spec))


def _arc_from_center() -> Command:
    center = yield Prompt("point", "Specify center point of arc:")
    if center is None:
        return
    message = "Specify start point of arc:"
    while True:
        start = yield Prompt("point", message, center, preview=line_preview(center))
        if start is None:
            return
        if start != center:
            yield from _arc_end_about_center(center, start)
            return
        message = "The start point is the center. Specify start point of arc:"


def _arc_start_center_end(start: Point) -> Command:
    message = "Specify center point of arc:"
    while True:
        center = yield Prompt("point", message, start, preview=line_preview(start))
        if center is None:
            return
        if center != start:
            yield from _arc_end_about_center(center, start)
            return
        message = "The center is the start point. Specify center point of arc:"


def _arc_start_end_center(start: Point) -> Command:
    end = yield Prompt("point", "Specify end point of arc:", start, preview=line_preview(start))
    if end is None:
        return
    message = "Specify center point of arc:"
    while True:
        center = yield Prompt(
            "point", message, end, preview=lambda cursor: [Line(cursor, start), Line(cursor, end)]
        )
        if center is None:
            return
        arc_found = _arc_about_center(center, start, end)
        if arc_found is not None:
            yield Drawn([arc_found])
            return
        message = f"{NO_ARC} Specify center point of arc:"


def _arc_end_about_center(center: Point, start: Point) -> Command:
    message = "Specify end point of arc:"
    while True:
        end = yield Prompt("point", message, center, preview=_about_center_preview(center, start))
        if end is None:
            return
        arc_found = _arc_about_center(center, start, end)
        if arc_found is not None:
            yield Drawn([arc_found])
            return
        message = f"{NO_ARC} Specify end point of arc:"

