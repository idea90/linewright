from collections.abc import Callable, Generator
from dataclasses import dataclass
from typing import Literal, Protocol

from linewright.model.entities import Entity
from linewright.model.geometry import Point


@dataclass(frozen=True)
class Prompt:
    """A question a command asks.

    "select" collects objects until Enter, "pick" takes one clicked object, "number" only accepts values greater
    than zero and "length" accepts zero too.
    """

    kind: Literal["point", "number", "length", "angle", "text", "select", "pick"]
    message: str
    base: Point | None = None  # relative coordinates and clicked distances or angles are measured from here
    options: tuple[str, ...] = ()  # keywords accepted instead of a value
    default: float | str | None = None  # what Enter answers on a prompt that has one
    click_factor: float = 1.0  # a click on a number prompt answers with distance * click_factor
    preview: Callable[[Point], list[Entity]] | None = None  # rubber band for the cursor position
    or_all: bool = False  # on a select prompt, Enter with nothing selected means every object


@dataclass(frozen=True)
class Picked:
    """An answer to a pick prompt: the object that was clicked, where, and whether Shift was held."""

    entity: Entity
    point: Point
    shift: bool = False


@dataclass(frozen=True)
class Drawn:
    """Draw these brand-new entities. They take the current layer, color and linetype."""

    entities: list[Entity]


@dataclass(frozen=True)
class Commit:
    """Add entities made from existing ones, which keep the layer and look they were given."""

    entities: list[Entity]


@dataclass(frozen=True)
class Retract:
    """Take back the entities this command committed last."""


@dataclass(frozen=True)
class Remove:
    """Delete these entities from the drawing."""

    entities: list[Entity]


@dataclass(frozen=True)
class Modify:
    """Change entities in place: each pair is an existing entity and a copy holding its new values."""

    changes: list[tuple[Entity, Entity]]


@dataclass(frozen=True)
class Compound:
    """Several steps that undo together as one."""

    steps: tuple["Step", ...]


@dataclass(frozen=True)
class Report:
    """Tell the user something on the command line, such as a measurement."""

    message: str


class CommandHost(Protocol):
    """What a running command may do to the drawing. Each call is one undo step, or all of a group."""

    def add(self, entities: list[Entity]) -> None: ...

    def add_new(self, entities: list[Entity]) -> None: ...

    def remove(self, entities: list[Entity]) -> None: ...

    def modify(self, changes: list[tuple[Entity, Entity]]) -> None: ...

    def retract(self) -> None: ...

    def begin_group(self) -> None: ...

    def end_group(self) -> None: ...

    def report(self, message: str) -> None: ...


# A command yields prompts and receives the answer. None means Enter or Esc.
Step = Drawn | Commit | Retract | Remove | Modify | Compound | Report
Answer = Point | float | str | list | Picked | None
Command = Generator[Prompt | Step, Answer, None]









