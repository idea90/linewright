import math

from linewright.commands.parse import parse_finite, parse_length, parse_number, parse_point
from linewright.commands.prompts import (
    Answer,
    Command,
    CommandHost,
    Commit,
    Compound,
    Drawn,
    Modify,
    Picked,
    Prompt,
    Remove,
    Report,
    Step,
)
from linewright.model.entities import Entity
from linewright.model.geometry import Point

SELECT_HINT = "Select objects by clicking or dragging, then press Enter."
INVALID_ANSWER = {
    "point": "Enter a point as x,y or @dx,dy or @distance<angle.",
    "number": "Enter a number greater than zero.",
    "length": "Enter a number, zero or greater.",
    "angle": "Enter an angle in degrees.",
    "select": SELECT_HINT,
    "pick": SELECT_HINT,
}


class CommandSession:
    """Runs one command and hands the user's answers to it. Qt-free so it can be tested headless."""

    def __init__(self, command: Command, host: CommandHost) -> None:
        self._command = command
        self._host = host
        self.prompt: Prompt | None = None
        self._advance(None)

    @property
    def active(self) -> bool:
        return self.prompt is not None

    def submit_text(self, text: str) -> str | None:
        """Answer the current prompt with typed text. Returns an error message if it was not valid."""
        prompt = self.prompt
        if prompt is None:
            return None
        text = text.strip()
        if not text:
            self._advance(prompt.default)
            return None
        answer = self._parse(prompt, text)
        if answer is None:
            return INVALID_ANSWER[prompt.kind]
        self._advance(answer)
        return None

    def submit_point(self, point: Point) -> None:
        """Answer with a click: a point, a distance from the base point, or an angle around it."""
        prompt = self.prompt
        if prompt is None or prompt.kind in ("text", "select", "pick"):
            return
        if prompt.kind == "point":
            self._advance(point)
        elif prompt.base is not None and point != prompt.base:
            self._advance(self._measure(prompt, point))

    def submit_objects(self, objects: list[Entity]) -> None:
        """Finish a select prompt with the chosen objects."""
        if self.prompt is not None and self.prompt.kind == "select":
            self._advance(objects)

    def submit_pick(self, entity: Entity, point: Point, shift: bool = False) -> None:
        """Answer a pick prompt with the clicked object and where it was clicked."""
        if self.prompt is not None and self.prompt.kind == "pick":
            self._advance(Picked(entity, point, shift))

    def cancel(self) -> None:
        """Esc. A command that can finish with what it has does so, any other stops."""
        if self.prompt is None:
            return
        self._advance(None)
        if self.prompt is not None:
            self._command.close()
            self.prompt = None

    @staticmethod
    def _measure(prompt: Prompt, point: Point) -> float:
        dx, dy = point[0] - prompt.base[0], point[1] - prompt.base[1]
        if prompt.kind == "angle":
            return math.degrees(math.atan2(dy, dx))
        return math.hypot(dx, dy) * prompt.click_factor

    @staticmethod
    def _parse(prompt: Prompt, text: str) -> Answer:
        if text.upper() in prompt.options:
            return text.upper()
        if prompt.kind == "point":
            return parse_point(text, prompt.base)
        if prompt.kind == "number":
            return parse_number(text)
        if prompt.kind == "length":
            return parse_length(text)
        if prompt.kind == "angle":
            return parse_finite(text)
        if prompt.kind == "text":
            return text
        return None

    def _advance(self, answer: Answer) -> None:
        try:
            step = self._command.send(answer)
            while not isinstance(step, Prompt):
                self._perform(step)
                step = self._command.send(None)
        except StopIteration:
            self.prompt = None
            return
        self.prompt = step

    def _perform(self, step: Step) -> None:
        if isinstance(step, Compound):
            self._host.begin_group()
            for inner in step.steps:
                self._perform(inner)
            self._host.end_group()
        elif isinstance(step, Drawn):
            self._host.add_new(step.entities)
        elif isinstance(step, Commit):
            self._host.add(step.entities)
        elif isinstance(step, Remove):
            self._host.remove(step.entities)
        elif isinstance(step, Modify):
            self._host.modify(step.changes)
        elif isinstance(step, Report):
            self._host.report(step.message)
        else:
            self._host.retract()









