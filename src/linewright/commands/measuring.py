from linewright.commands.draw import line_preview, polyline_from
from linewright.commands.prompts import Command, Prompt, Report
from linewright.model.geometry import Point
from linewright.model.measure import area_report, distance_report, measure_entity, polygon

OBJECT = "O"
NO_AREA = "That object has no area."
TOO_FEW_POINTS = "An area needs at least three points."


def distance() -> Command:
    first = yield Prompt("point", "Specify first point:")
    if first is None:
        return
    second = yield Prompt("point", "Specify second point:", first, preview=line_preview(first))
    if second is not None:
        yield Report(distance_report(first, second))


def area() -> Command:
    answer = yield Prompt(
        "point", "Specify first corner point or [Object] <Object>:", options=(OBJECT,), default=OBJECT
    )
    if answer == OBJECT:
        yield from _area_of_object()
    elif answer is not None:
        yield from _area_of_points(answer)


def _area_of_points(first: Point) -> Command:
    points = [first]
    while True:
        answer = yield Prompt(
            "point",
            "Specify next corner point or press ENTER for total:",
            points[-1],
            preview=lambda cursor: [polyline_from([*points, cursor], closed=True)],
        )
        if answer is None:
            break
        points.append(answer)
    if len(points) < 3:
        yield Report(TOO_FEW_POINTS)
        return
    yield Report(area_report(polygon([(x, y, 0.0) for x, y in points])))


def _area_of_object() -> Command:
    message = "Select objects:"
    while True:
        picked = yield Prompt("pick", message)
        if picked is None:
            return
        measured = measure_entity(picked.entity)
        if measured is not None:
            yield Report(area_report(measured))
            return
        message = f"{NO_AREA} Select objects:"
