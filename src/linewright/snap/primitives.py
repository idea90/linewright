import math

from linewright.model.dimension import dimension_parts
from linewright.model.entities import Arc, Circle, Dimension, Entity, Line, Polyline
from linewright.model.geometry import Point, bulge_arc

Primitive = Line | Arc | Circle

ANGLE_TOLERANCE = 1e-7  # degrees
PARAMETER_TOLERANCE = 1e-9


def primitives(entity: Entity) -> list[Primitive]:
    """The lines, arcs and circles an entity is made of. Polylines split into their segments."""
    if isinstance(entity, (Line, Arc, Circle)):
        return [entity]
    if isinstance(entity, Polyline):
        return _polyline_segments(entity)
    if isinstance(entity, Dimension):
        parts = dimension_parts(entity)
        return [Line(*segment) for segment in (*parts.extension_lines, parts.dimension_line)]
    return []


def _polyline_segments(polyline: Polyline) -> list[Primitive]:
    vertices = polyline.vertices
    count = len(vertices) if polyline.closed else len(vertices) - 1
    segments: list[Primitive] = []
    for index in range(count):
        x, y, bulge = vertices[index]
        next_x, next_y, _ = vertices[(index + 1) % len(vertices)]
        if bulge:
            segments.append(_bulge_segment((x, y), (next_x, next_y), bulge))
        else:
            segments.append(Line((x, y), (next_x, next_y)))
    return segments


def _bulge_segment(start: Point, end: Point, bulge: float) -> Arc:
    arc = bulge_arc(start, end, bulge)
    first = math.degrees(arc.start_angle)
    last = first + math.degrees(arc.sweep)
    if arc.sweep < 0:
        first, last = last, first
    return Arc(arc.center, arc.radius, first % 360.0, last % 360.0)


def arc_sweep(arc: Arc) -> float:
    return (arc.end_angle - arc.start_angle) % 360.0 or 360.0


def arc_point(arc: Arc, angle: float) -> Point:
    radians = math.radians(angle)
    return (arc.center[0] + arc.radius * math.cos(radians), arc.center[1] + arc.radius * math.sin(radians))


def on_arc(arc: Arc, point: Point) -> bool:
    angle = math.degrees(math.atan2(point[1] - arc.center[1], point[0] - arc.center[0]))
    offset = (angle - arc.start_angle) % 360.0
    return offset <= arc_sweep(arc) + ANGLE_TOLERANCE or offset >= 360.0 - ANGLE_TOLERANCE


def line_parameter(line: Line, point: Point) -> float | None:
    """Where the point's projection falls along the line: 0 at the start, 1 at the end."""
    dx, dy = line.end[0] - line.start[0], line.end[1] - line.start[1]
    length_squared = dx * dx + dy * dy
    if length_squared == 0:
        return None
    return ((point[0] - line.start[0]) * dx + (point[1] - line.start[1]) * dy) / length_squared


def on_primitive(primitive: Primitive, point: Point) -> bool:
    """Whether a point already known to lie on the primitive's full line or circle is on the primitive itself."""
    if isinstance(primitive, Line):
        parameter = line_parameter(primitive, point)
        return parameter is not None and -PARAMETER_TOLERANCE <= parameter <= 1 + PARAMETER_TOLERANCE
    if isinstance(primitive, Arc):
        return on_arc(primitive, point)
    return True


def distance_to(primitive: Primitive, point: Point) -> float:
    if isinstance(primitive, Line):
        parameter = line_parameter(primitive, point)
        if parameter is None:
            return math.dist(primitive.start, point)
        clamped = min(max(parameter, 0.0), 1.0)
        nearest = (
            primitive.start[0] + clamped * (primitive.end[0] - primitive.start[0]),
            primitive.start[1] + clamped * (primitive.end[1] - primitive.start[1]),
        )
        return math.dist(nearest, point)
    from_center = math.dist(primitive.center, point)
    if isinstance(primitive, Circle) or on_arc(primitive, point):
        return abs(from_center - primitive.radius)
    start = arc_point(primitive, primitive.start_angle)
    end = arc_point(primitive, primitive.end_angle)
    return min(math.dist(start, point), math.dist(end, point))
