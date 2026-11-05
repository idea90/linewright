import math
from dataclasses import replace

from linewright.model.entities import Arc, Circle, Entity, Line, Polyline
from linewright.model.geometry import Point
from linewright.snap.intersect import intersections
from linewright.snap.primitives import Primitive, arc_sweep, distance_to, line_parameter, primitives

PARAMETER_EPSILON = 1e-9  # in the units of the parameter: fractions of a line, or degrees
DISTANCE_EPSILON = 1e-9
EXTEND_REACH = 1e6  # how many lengths of the object an extension may reach when looking for a boundary


class LineCurve:
    """A line as a path from 0 to 1."""

    closed = False
    span = 1.0

    def __init__(self, line: Line) -> None:
        self._line = line

    def param_of(self, point: Point) -> float:
        parameter = line_parameter(self._line, point)
        return min(max(parameter, 0.0), 1.0) if parameter is not None else 0.0

    def cut_params(self, edges: list[Primitive]) -> list[float]:
        points = [p for edge in edges for p in intersections(self._line, edge)]
        return [line_parameter(self._line, p) or 0.0 for p in points]

    def piece(self, start: float, end: float) -> Entity | None:
        return replace(self._line, start=_lerp(self._line, start), end=_lerp(self._line, end))


class ArcCurve:
    """An arc as a path measured in degrees from its start."""

    closed = False

    def __init__(self, arc: Arc) -> None:
        self._arc = arc
        self.span = arc_sweep(arc)

    def _offset(self, point: Point) -> float:
        angle = math.degrees(math.atan2(point[1] - self._arc.center[1], point[0] - self._arc.center[0]))
        return (angle - self._arc.start_angle) % 360.0

    def param_of(self, point: Point) -> float:
        offset = self._offset(point)
        if offset <= self.span:
            return offset
        return self.span if offset - self.span < 360.0 - offset else 0.0

    def cut_params(self, edges: list[Primitive]) -> list[float]:
        return [min(self._offset(p), self.span) for edge in edges for p in intersections(self._arc, edge)]

    def piece(self, start: float, end: float) -> Entity | None:
        first, last = self._arc.start_angle + start, self._arc.start_angle + end
        return replace(self._arc, start_angle=first % 360.0, end_angle=last % 360.0)


class CircleCurve:
    """A circle as a closed path measured in degrees from the right-hand side."""

    closed = True
    span = 360.0

    def __init__(self, circle: Circle) -> None:
        self._circle = circle

    def _angle(self, point: Point) -> float:
        center = self._circle.center
        return math.degrees(math.atan2(point[1] - center[1], point[0] - center[0])) % 360.0

    def param_of(self, point: Point) -> float:
        return self._angle(point)

    def cut_params(self, edges: list[Primitive]) -> list[float]:
        return [self._angle(p) for edge in edges for p in intersections(self._circle, edge)]

    def piece(self, start: float, end: float) -> Entity | None:
        circle = self._circle
        return Arc(circle.center, circle.radius, start % 360.0, end % 360.0, layer=circle.layer, color=circle.color)


class PolylineCurve:
    """A polyline of straight segments as a path where segment i covers i to i + 1."""

    def __init__(self, polyline: Polyline) -> None:
        self._polyline = polyline
        self._points = [(x, y) for x, y, _ in polyline.vertices]
        self.closed = polyline.closed
        self.span = float(len(self._points) if polyline.closed else len(self._points) - 1)

    def _segment(self, index: int) -> Line:
        return Line(self._points[index], self._points[(index + 1) % len(self._points)])

    def param_of(self, point: Point) -> float:
        count = int(self.span)
        nearest = min(range(count), key=lambda i: distance_to(self._segment(i), point))
        segment = self._segment(nearest)
        fraction = line_parameter(segment, point)
        return nearest + (min(max(fraction, 0.0), 1.0) if fraction is not None else 0.0)

    def cut_params(self, edges: list[Primitive]) -> list[float]:
        found = []
        for index in range(int(self.span)):
            segment = self._segment(index)
            if segment.start == segment.end:
                continue
            for edge in edges:
                found += [index + line_parameter(segment, p) for p in intersections(segment, edge)]
        return found

    def _point_at(self, u: float) -> Point:
        if self.closed:
            u %= self.span
        index = min(int(math.floor(u)), int(self.span) - 1)
        return _lerp(self._segment(index), u - index)

    def piece(self, start: float, end: float) -> Entity | None:
        points = [self._point_at(start)]
        vertex = math.floor(start) + 1
        while vertex < end - PARAMETER_EPSILON:
            points.append(self._points[int(vertex) % len(self._points)])
            vertex += 1
        points.append(self._point_at(end))
        distinct = [p for i, p in enumerate(points) if i == 0 or math.dist(p, points[i - 1]) > DISTANCE_EPSILON]
        if len(distinct) < 2:
            return None
        return replace(self._polyline, vertices=[(x, y, 0.0) for x, y in distinct], closed=False)


def _lerp(line: Line, t: float) -> Point:
    return (line.start[0] + t * (line.end[0] - line.start[0]), line.start[1] + t * (line.end[1] - line.start[1]))


def make_curve(entity: Entity) -> LineCurve | ArcCurve | CircleCurve | PolylineCurve | None:
    if isinstance(entity, Line):
        return LineCurve(entity)
    if isinstance(entity, Arc):
        return ArcCurve(entity)
    if isinstance(entity, Circle):
        return CircleCurve(entity)
    if isinstance(entity, Polyline) and len(entity.vertices) >= 2 and not any(b for _, _, b in entity.vertices):
        return PolylineCurve(entity)
    return None


def _edges_for(target: Entity, boundaries: list[Entity]) -> list[Primitive]:
    return [prim for entity in boundaries if entity is not target for prim in primitives(entity)]


def _distinct(values: list[float]) -> list[float]:
    found: list[float] = []
    for value in sorted(values):
        if not found or value - found[-1] > PARAMETER_EPSILON:
            found.append(value)
    return found


def trim_entity(target: Entity, click: Point, boundaries: list[Entity]) -> list[Entity] | None:
    """What is left of the target after cutting away the stretch under the click, between the boundaries.

    None means it cannot be trimmed there. An empty list means nothing is left.
    """
    curve = make_curve(target)
    edges = _edges_for(target, boundaries)
    if curve is None or not edges:
        return None
    cuts = curve.cut_params(edges)
    if curve.closed:
        return _trim_closed(curve, curve.param_of(click), _distinct([c % curve.span for c in cuts]))
    cuts = _distinct([c for c in cuts if PARAMETER_EPSILON < c < curve.span - PARAMETER_EPSILON])
    if not cuts:
        return None
    u = curve.param_of(click)
    lower = max((c for c in cuts if c < u), default=0.0)
    upper = min((c for c in cuts if c > u), default=curve.span)
    pieces = []
    if lower > PARAMETER_EPSILON:
        pieces.append(curve.piece(0.0, lower))
    if upper < curve.span - PARAMETER_EPSILON:
        pieces.append(curve.piece(upper, curve.span))
    return [p for p in pieces if p is not None]


def _trim_closed(curve, u: float, cuts: list[float]) -> list[Entity] | None:
    if len(cuts) < 2:
        return None
    lower = max((c for c in cuts if c < u), default=cuts[-1] - curve.span)
    upper = min((c for c in cuts if c > u), default=cuts[0] + curve.span)
    remaining = curve.piece(upper, lower + curve.span)
    return [remaining] if remaining is not None else []


def _beyond(start: Point, end: Point, edges: list[Primitive]) -> Point | None:
    """The first point where the path from start through end, carried on past end, meets an edge."""
    length = math.dist(start, end)
    if length == 0:
        return None
    ux, uy = (end[0] - start[0]) / length, (end[1] - start[1]) / length
    reach = EXTEND_REACH * max(length, 1.0)
    carrier = Line(start, (end[0] + ux * reach, end[1] + uy * reach))
    best, best_distance = None, math.inf
    for edge in edges:
        for point in intersections(carrier, edge):
            ahead = (point[0] - end[0]) * ux + (point[1] - end[1]) * uy
            if DISTANCE_EPSILON * max(length, 1.0) < ahead < best_distance:
                best, best_distance = point, ahead
    return best


def _extend_line(line: Line, click: Point, edges: list[Primitive]) -> Entity | None:
    toward_end = (line_parameter(line, click) or 0.0) >= 0.5
    if toward_end:
        point = _beyond(line.start, line.end, edges)
        return replace(line, end=point) if point else None
    point = _beyond(line.end, line.start, edges)
    return replace(line, start=point) if point else None


def _extend_polyline(polyline: Polyline, click: Point, edges: list[Primitive]) -> Entity | None:
    curve = make_curve(polyline)
    if curve is None or polyline.closed:
        return None
    points = [(x, y) for x, y, _ in polyline.vertices]
    if curve.param_of(click) >= curve.span / 2:
        point = _beyond(points[-2], points[-1], edges)
        points[-1] = point or points[-1]
    else:
        point = _beyond(points[1], points[0], edges)
        points[0] = point or points[0]
    return replace(polyline, vertices=[(x, y, 0.0) for x, y in points]) if point else None


def _extend_arc(arc: Arc, click: Point, edges: list[Primitive]) -> Entity | None:
    curve = ArcCurve(arc)
    toward_end = curve.param_of(click) >= curve.span / 2
    full = Circle(arc.center, arc.radius)
    best = None
    for edge in edges:
        for point in intersections(full, edge):
            angle = math.degrees(math.atan2(point[1] - arc.center[1], point[0] - arc.center[0]))
            gap = (angle - arc.end_angle) % 360.0 if toward_end else (arc.start_angle - angle) % 360.0
            if PARAMETER_EPSILON < gap < 360.0 - curve.span and (best is None or gap < best):
                best = gap
    if best is None:
        return None
    if toward_end:
        return replace(arc, end_angle=(arc.end_angle + best) % 360.0)
    return replace(arc, start_angle=(arc.start_angle - best) % 360.0)


def extend_entity(target: Entity, click: Point, boundaries: list[Entity]) -> Entity | None:
    """The target with the end nearest the click carried on to the first boundary, or None if none is in the way."""
    edges = _edges_for(target, boundaries)
    if not edges:
        return None
    if isinstance(target, Line):
        return _extend_line(target, click, edges)
    if isinstance(target, Arc):
        return _extend_arc(target, click, edges)
    if isinstance(target, Polyline):
        return _extend_polyline(target, click, edges)
    return None



