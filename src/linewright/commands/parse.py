import math

from linewright.model.geometry import Point


def parse_number(text: str) -> float | None:
    """A finite number greater than zero. The decimal separator is always a dot."""
    try:
        value = float(text)
    except ValueError:
        return None
    return value if math.isfinite(value) and value > 0 else None


def parse_finite(text: str) -> float | None:
    """Any finite number. Angles are read as degrees."""
    try:
        value = float(text)
    except ValueError:
        return None
    return value if math.isfinite(value) else None


def parse_point(text: str, base: Point | None) -> Point | None:
    """Accepts x,y  and  @dx,dy  and  @distance<angle  (relative to base, or the origin without one)."""
    text = text.strip()
    relative = text.startswith("@")
    body = text[1:] if relative else text
    offset = _parse_offset(body)
    if offset is None:
        return None
    origin = base if relative and base else (0.0, 0.0)
    return (origin[0] + offset[0], origin[1] + offset[1])


def _parse_offset(body: str) -> Point | None:
    if "<" in body:
        distance_text, _, angle_text = body.partition("<")
        distance, angle = parse_finite(distance_text), parse_finite(angle_text)
        if distance is None or angle is None:
            return None
        radians = math.radians(angle)
        return (distance * math.cos(radians), distance * math.sin(radians))
    parts = body.split(",")
    if len(parts) != 2:
        return None
    x, y = parse_finite(parts[0]), parse_finite(parts[1])
    return None if x is None or y is None else (x, y)


def parse_length(text: str) -> float | None:
    """A finite number, zero or greater."""
    value = parse_finite(text)
    return value if value is not None and value >= 0 else None
