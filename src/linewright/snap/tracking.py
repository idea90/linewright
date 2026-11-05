import math

from linewright.model.geometry import Point

POLAR_TOLERANCE = 5.0  # degrees either side of an increment that the cursor is pulled onto it


def snap_to_grid(point: Point, step: float) -> Point:
    return (round(point[0] / step) * step, round(point[1] / step) * step)


def constrain_ortho(base: Point, point: Point) -> Point:
    """Keep whichever of horizontal or vertical the cursor is closer to."""
    if abs(point[0] - base[0]) >= abs(point[1] - base[1]):
        return (point[0], base[1])
    return (base[0], point[1])


def constrain_polar(base: Point, point: Point, increment: float) -> Point:
    """Pull the cursor onto the nearest multiple of the increment angle when it is within tolerance."""
    dx, dy = point[0] - base[0], point[1] - base[1]
    length = math.hypot(dx, dy)
    if length == 0:
        return point
    angle = math.degrees(math.atan2(dy, dx))
    target = round(angle / increment) * increment
    error = abs(angle - target)
    if min(error, 360.0 - error) > POLAR_TOLERANCE:
        return point
    reach = length * math.cos(math.radians(angle - target))
    radians = math.radians(target)
    return (base[0] + reach * math.cos(radians), base[1] + reach * math.sin(radians))
