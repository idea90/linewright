import math

from linewright.model.entities import Circle, Entity, Line, Text
from linewright.model.geometry import Point
from linewright.snap.intersect import intersections
from linewright.snap.primitives import Primitive, arc_point, distance_to, on_arc, primitives

TEXT_WIDTH_PER_HEIGHT = 0.6  # rough average glyph advance; the real width depends on the font


def _text_corners(text: Text) -> list[Point]:
    width = TEXT_WIDTH_PER_HEIGHT * text.height * max(len(text.text), 1)
    cos, sin = math.cos(math.radians(text.rotation)), math.sin(math.radians(text.rotation))
    x, y = text.position
    local = [(0, 0), (width, 0), (width, text.height), (0, text.height)]
    return [(x + u * cos - v * sin, y + u * sin + v * cos) for u, v in local]


def outline(entity: Entity) -> list[Primitive]:
    """What the entity looks like to the mouse. Text is treated as the box around it."""
    if isinstance(entity, Text):
        corners = _text_corners(entity)
        return [Line(corners[i], corners[(i + 1) % 4]) for i in range(4)]
    return primitives(entity)


def entity_distance(entity: Entity, point: Point) -> float:
    shapes = outline(entity)
    if not shapes:
        return math.inf
    if isinstance(entity, Text) and _inside(_text_corners(entity), point):
        return 0.0
    return min(distance_to(shape, point) for shape in shapes)


def _inside(corners: list[Point], point: Point) -> bool:
    """Whether the point is inside a convex polygon given in order."""
    signs = []
    for index, start in enumerate(corners):
        end = corners[(index + 1) % len(corners)]
        signs.append((end[0] - start[0]) * (point[1] - start[1]) - (end[1] - start[1]) * (point[0] - start[0]))
    return all(s >= 0 for s in signs) or all(s <= 0 for s in signs)


def pick_entity(entities: list[Entity], point: Point, aperture: float) -> Entity | None:
    """The entity nearest the point within the aperture. Later (upper) entities win ties."""
    best, best_distance = None, aperture
    for entity in entities:
        distance = entity_distance(entity, point)
        if distance <= best_distance:
            best, best_distance = entity, distance
    return best


def _extent_points(shape: Primitive) -> list[Point]:
    """Points that bound the shape: its ends, plus the extreme points of a round shape."""
    if isinstance(shape, Line):
        return [shape.start, shape.end]
    quadrants = [(shape.center[0] + dx * shape.radius, shape.center[1] + dy * shape.radius)
                 for dx, dy in ((1, 0), (0, 1), (-1, 0), (0, -1))]
    if isinstance(shape, Circle):
        return quadrants
    ends = [arc_point(shape, shape.start_angle), arc_point(shape, shape.end_angle)]
    return ends + [p for p in quadrants if on_arc(shape, p)]


def _in_rect(point: Point, low: Point, high: Point) -> bool:
    return low[0] <= point[0] <= high[0] and low[1] <= point[1] <= high[1]


def _rect_edges(low: Point, high: Point) -> list[Line]:
    corners = [low, (high[0], low[1]), high, (low[0], high[1])]
    return [Line(corners[i], corners[(i + 1) % 4]) for i in range(4)]


def _touches_rect(shape: Primitive, low: Point, high: Point) -> bool:
    if any(_in_rect(p, low, high) for p in _extent_points(shape)):
        return True
    return any(intersections(shape, edge) for edge in _rect_edges(low, high))


def select_in_box(entities: list[Entity], first: Point, second: Point) -> list[Entity]:
    """Window (dragged left to right): entities entirely inside. Crossing (right to left): inside or touching."""
    low = (min(first[0], second[0]), min(first[1], second[1]))
    high = (max(first[0], second[0]), max(first[1], second[1]))
    crossing = second[0] < first[0]
    found = []
    for entity in entities:
        shapes = outline(entity)
        if not shapes:
            continue
        if crossing:
            hit = any(_touches_rect(shape, low, high) for shape in shapes)
        else:
            hit = all(_in_rect(p, low, high) for shape in shapes for p in _extent_points(shape))
        if hit:
            found.append(entity)
    return found
