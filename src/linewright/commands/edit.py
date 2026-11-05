import math
from collections.abc import Callable, Generator

from linewright.commands.defaults import Defaults
from linewright.commands.prompts import Answer, Command, Commit, Compound, Modify, Picked, Prompt, Remove, Retract, Step
from linewright.model.entities import Entity, Line
from linewright.model.fillet import FilletError, fillet_lines
from linewright.model.geometry import Point
from linewright.model.offset import offset_entity
from linewright.model.transform import Similarity, reflection, rotation, scaling, transform_entity, translation
from linewright.model.trimming import extend_entity, trim_entity

MAX_PREVIEW_ENTITIES = 200  # a rubber band of thousands of entities would make the cursor lag
UNDO = "U"
YES = "Y"
NO = "N"
RADIUS = "R"
TRIM_PROMPT = "Select object to trim or shift-select to extend or [Undo]:"
EXTEND_PROMPT = "Select object to extend or shift-select to trim or [Undo]:"


def _direction(base: Point, cursor: Point) -> float:
    return math.degrees(math.atan2(cursor[1] - base[1], cursor[0] - base[0]))


def _changes(objects: list[Entity], sim: Similarity) -> list[tuple[Entity, Entity]]:
    return [(entity, transform_entity(entity, sim)) for entity in objects]


def _copies(objects: list[Entity], sim: Similarity) -> list[Entity]:
    return [new for _, new in _changes(objects, sim)]


def _preview(objects: list[Entity], sim_for: Callable[[Point], Similarity]) -> Callable[[Point], list[Entity]]:
    shown = objects[:MAX_PREVIEW_ENTITIES]
    return lambda cursor: _copies(shown, sim_for(cursor))


def _objects_to_edit(selected: list[Entity]) -> Generator[Prompt, Answer, list[Entity]]:
    """The objects to work on: the ones already selected, otherwise whatever the user selects now."""
    if selected:
        return selected
    chosen = yield Prompt("select", "Select objects:")
    return chosen or []


def erase(selected: list[Entity], defaults: Defaults) -> Command:
    objects = yield from _objects_to_edit(selected)
    if objects:
        yield Remove(objects)


def move(selected: list[Entity], defaults: Defaults) -> Command:
    objects = yield from _objects_to_edit(selected)
    if not objects:
        return
    base = yield Prompt("point", "Specify base point:")
    if base is None:
        return
    target = yield Prompt(
        "point",
        "Specify second point:",
        base,
        preview=_preview(objects, lambda cursor: translation(cursor[0] - base[0], cursor[1] - base[1])),
    )
    if target is not None:
        yield Modify(_changes(objects, translation(target[0] - base[0], target[1] - base[1])))


def copy(selected: list[Entity], defaults: Defaults) -> Command:
    objects = yield from _objects_to_edit(selected)
    if not objects:
        return
    base = yield Prompt("point", "Specify base point:")
    if base is None:
        return
    copies_made = 0
    while True:
        target = yield Prompt(
            "point",
            "Specify second point or [Undo]:",
            base,
            (UNDO,),
            preview=_preview(objects, lambda cursor: translation(cursor[0] - base[0], cursor[1] - base[1])),
        )
        if target is None:
            return
        if target == UNDO:
            if copies_made:
                yield Retract()
                copies_made -= 1
            continue
        yield Commit(_copies(objects, translation(target[0] - base[0], target[1] - base[1])))
        copies_made += 1


def rotate(selected: list[Entity], defaults: Defaults) -> Command:
    objects = yield from _objects_to_edit(selected)
    if not objects:
        return
    base = yield Prompt("point", "Specify base point:")
    if base is None:
        return
    angle = yield Prompt(
        "angle",
        "Specify rotation angle:",
        base,
        preview=_preview(objects, lambda cursor: rotation(base, _direction(base, cursor))),
    )
    if angle is not None:
        yield Modify(_changes(objects, rotation(base, angle)))


def scale(selected: list[Entity], defaults: Defaults) -> Command:
    objects = yield from _objects_to_edit(selected)
    if not objects:
        return
    base = yield Prompt("point", "Specify base point:")
    if base is None:
        return
    factor = yield Prompt("number", "Specify scale factor:", base, preview=_scale_preview(objects, base))
    if factor is not None:
        yield Modify(_changes(objects, scaling(base, factor)))


def _scale_preview(objects: list[Entity], base: Point) -> Callable[[Point], list[Entity]]:
    shown = objects[:MAX_PREVIEW_ENTITIES]

    def preview(cursor: Point) -> list[Entity]:
        factor = math.dist(base, cursor)
        return _copies(shown, scaling(base, factor)) if factor > 0 else []

    return preview


def mirror(selected: list[Entity], defaults: Defaults) -> Command:
    objects = yield from _objects_to_edit(selected)
    if not objects:
        return
    first = yield Prompt("point", "Specify first point of mirror line:")
    if first is None:
        return
    second = yield from _second_mirror_point(objects, first)
    if second is None:
        return
    erase_source = yield Prompt(
        "point", "Erase source objects? [Yes/No] <N>:", options=(YES, NO), default=NO
    )
    sim = reflection(first, second)
    if erase_source == YES:
        yield Modify(_changes(objects, sim))
    elif erase_source == NO:
        yield Commit(_copies(objects, sim))


def _second_mirror_point(objects: list[Entity], first: Point) -> Generator[Prompt, Answer, Point | None]:
    message = "Specify second point of mirror line:"
    while True:
        second = yield Prompt("point", message, first, preview=_mirror_preview(objects, first))
        if second is None or second != first:
            return second
        message = "The mirror line needs two different points. Specify second point of mirror line:"


def _mirror_preview(objects: list[Entity], first: Point) -> Callable[[Point], list[Entity]]:
    shown = objects[:MAX_PREVIEW_ENTITIES]
    return lambda cursor: _copies(shown, reflection(first, cursor)) if cursor != first else []


def offset(selected: list[Entity], defaults: Defaults) -> Command:
    distance = yield Prompt("number", "Specify offset distance:")
    if distance is None:
        return
    message = "Select object to offset or <exit>:"
    while True:
        picked = yield Prompt("pick", message)
        if picked is None:
            return
        entity = picked.entity
        side = yield Prompt(
            "point",
            "Specify point on side to offset:",
            preview=lambda cursor: [o for o in [offset_entity(entity, distance, cursor)] if o is not None],
        )
        if side is None:
            return
        result = offset_entity(entity, distance, side)
        if result is None:
            message = "That object cannot be offset by that distance. Select object to offset or <exit>:"
        else:
            yield Commit([result])
            message = "Select object to offset or <exit>:"



def _boundary_objects(selected: list[Entity], message: str) -> Generator[Prompt, Answer, list[Entity]]:
    """The objects to cut or extend against: the ones already selected, otherwise chosen now (Enter takes all)."""
    if selected:
        return selected
    chosen = yield Prompt("select", message, or_all=True)
    return chosen or []


def _trim_step(picked: Picked, boundaries: list[Entity]) -> tuple[Step, list[Entity]] | None:
    """The step that trims the picked object, and the boundaries after it (the object's pieces replace it)."""
    target = picked.entity
    pieces = trim_entity(target, picked.point, boundaries)
    if pieces is None:
        return None
    step = Compound((Remove([target]), Commit(pieces))) if pieces else Remove([target])
    if not any(entity is target for entity in boundaries):
        return step, boundaries
    return step, [entity for entity in boundaries if entity is not target] + pieces


def _extend_step(picked: Picked, boundaries: list[Entity]) -> tuple[Step, list[Entity]] | None:
    longer = extend_entity(picked.entity, picked.point, boundaries)
    return None if longer is None else (Modify([(picked.entity, longer)]), boundaries)


def _trim_or_extend(boundaries: list[Entity], extend_by_default: bool) -> Command:
    """Pick objects one after another. Holding Shift does the opposite of the command."""
    prompt = EXTEND_PROMPT if extend_by_default else TRIM_PROMPT
    message = prompt
    earlier: list[list[Entity]] = []
    while True:
        picked = yield Prompt("pick", message, options=(UNDO,))
        if picked is None:
            return
        if picked == UNDO:
            if earlier:
                yield Retract()
                boundaries = earlier.pop()
            continue
        extending = extend_by_default != picked.shift
        outcome = (_extend_step if extending else _trim_step)(picked, boundaries)
        if outcome is None:
            reason = "No boundary edge is in the way." if extending else "Nothing to trim against there."
            message = f"{reason} {prompt}"
            continue
        step, after = outcome
        earlier.append(boundaries)
        boundaries = after
        message = prompt
        yield step


def trim(selected: list[Entity], defaults: Defaults) -> Command:
    boundaries = yield from _boundary_objects(selected, "Select cutting edges or <select all>:")
    if boundaries:
        yield from _trim_or_extend(boundaries, extend_by_default=False)


def extend(selected: list[Entity], defaults: Defaults) -> Command:
    boundaries = yield from _boundary_objects(selected, "Select boundary edges or <select all>:")
    if boundaries:
        yield from _trim_or_extend(boundaries, extend_by_default=True)


def _fillet_step(first: Picked, second: Picked, radius: float) -> Step:
    if not isinstance(first.entity, Line) or not isinstance(second.entity, Line):
        raise FilletError("Fillet works on lines only for now.")
    if first.entity is second.entity:
        raise FilletError("Select two different lines.")
    result = fillet_lines(first.entity, first.point, second.entity, second.point, radius)
    modify = Modify([(first.entity, result.first), (second.entity, result.second)])
    return modify if result.arc is None else Compound((modify, Commit([result.arc])))


def fillet(selected: list[Entity], defaults: Defaults) -> Command:
    message = f"Select first line (radius {defaults.fillet_radius:g}) or [Radius]:"
    while True:
        first = yield Prompt("pick", message, options=(RADIUS,))
        if first is None:
            return
        if first == RADIUS:
            current = defaults.fillet_radius
            radius = yield Prompt("length", f"Specify fillet radius <{current:g}>:", default=current)
            defaults.fillet_radius = current if radius is None else radius
            message = f"Select first line (radius {defaults.fillet_radius:g}) or [Radius]:"
            continue
        second = yield Prompt("pick", "Select second line:")
        if second is None:
            return
        try:
            step = _fillet_step(first, second, defaults.fillet_radius)
        except FilletError as error:
            message = f"{error} Select first line or [Radius]:"
            continue
        yield step
        return
