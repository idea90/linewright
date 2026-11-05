from collections.abc import Callable

from linewright.commands import curves, dimension, draw, edit, measuring
from linewright.commands.defaults import Defaults
from linewright.commands.prompts import Command
from linewright.model.entities import Entity

# Commands that start with no objects selected: they draw or measure.
DRAWING: dict[str, Callable[[], Command]] = {
    "LINE": draw.line,
    "PLINE": draw.polyline,
    "RECTANGLE": draw.rectangle,
    "CIRCLE": curves.circle,
    "ARC": curves.arc,
    "TEXT": draw.text,
    "DISTANCE": measuring.distance,
    "AREA": measuring.area,
}

# These receive the entities that were already selected when they started, and the remembered settings.
EDITING: dict[str, Callable[[list[Entity], Defaults], Command]] = {
    "ERASE": edit.erase,
    "MOVE": edit.move,
    "COPY": edit.copy,
    "ROTATE": edit.rotate,
    "SCALE": edit.scale,
    "MIRROR": edit.mirror,
    "OFFSET": edit.offset,
    "TRIM": edit.trim,
    "EXTEND": edit.extend,
    "FILLET": edit.fillet,
    "DIMLINEAR": dimension.linear,
    "DIMALIGNED": dimension.aligned,
    "DIMSCALE": dimension.set_scale,
}

ALIASES = {
    "L": "LINE",
    "PL": "PLINE",
    "POLYLINE": "PLINE",
    "REC": "RECTANGLE",
    "RECTANG": "RECTANGLE",
    "C": "CIRCLE",
    "A": "ARC",
    "T": "TEXT",
    "E": "ERASE",
    "M": "MOVE",
    "CO": "COPY",
    "CP": "COPY",
    "RO": "ROTATE",
    "SC": "SCALE",
    "MI": "MIRROR",
    "O": "OFFSET",
    "TR": "TRIM",
    "EX": "EXTEND",
    "F": "FILLET",
    "DLI": "DIMLINEAR",
    "DAL": "DIMALIGNED",
    "DS": "DIMSCALE",
    "DI": "DISTANCE",
    "DIST": "DISTANCE",
    "AA": "AREA",
}


def create(name: str, selected: list[Entity], defaults: Defaults) -> Command:
    if name in EDITING:
        return EDITING[name](selected, defaults)
    return DRAWING[name]()


def shortest_alias(name: str) -> str:
    return min((alias for alias, command in ALIASES.items() if command == name), key=len)


def resolve(word: str) -> str | None:
    """Canonical command name for a typed name or alias, or None."""
    word = word.upper()
    word = ALIASES.get(word, word)
    return word if word in DRAWING or word in EDITING else None





