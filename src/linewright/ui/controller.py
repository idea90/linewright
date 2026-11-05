import math
from dataclasses import replace

from PySide6.QtCore import QObject, QPointF, QRectF, Qt, Signal
from PySide6.QtGui import QGuiApplication, QUndoStack

from linewright.canvas.sync import SceneSync
from linewright.canvas.view import CanvasView, grid_spacing
from linewright.commands.defaults import Defaults
from linewright.commands.prompts import Prompt
from linewright.commands.registry import create, resolve
from linewright.commands.session import SELECT_HINT, CommandSession
from linewright.model.document import Document
from linewright.model.entities import Entity
from linewright.model.geometry import Point
from linewright.selection.picking import pick_entity, select_in_box
from linewright.selection.selection import Selection
from linewright.snap.resolve import SnapResult, SnapScene, SnapSettings, resolve_point
from linewright.ui.command_line import CommandLine
from linewright.ui.undo import AddEntities, ModifyEntities, RemoveEntities

IDLE_PROMPT = "Command:"
IDLE_HINT = "Type a command, e.g. line, circle, move"
UNDO_WORDS = ("U", "UNDO")
REDO_WORDS = ("REDO",)
SELECT_ALL_WORD = "ALL"
SNAP_APERTURE_PIXELS = 12.0
PICK_APERTURE_PIXELS = 8.0
DRAG_THRESHOLD_PIXELS = 4.0
POINT_PROMPTS = ("point", "number", "length", "angle")
SELECTING_PROMPTS = ("select", "pick")


def _square_around(center: Point, radius: float) -> QRectF:
    return QRectF(center[0] - radius, center[1] - radius, 2 * radius, 2 * radius)


class DrawingController(QObject):
    """Connects typed input, clicks and the cursor to the running command, and its results to the document.

    It is also the host a running command acts on: every change goes through the undo stack.
    """

    cursor_snapped = Signal(QPointF)
    command_changed = Signal(object)  # the running command's name, or None when idle
    prompt_changed = Signal(object, str)  # the running command's name and its current question, or None and ""
    selection_changed = Signal()

    def __init__(
        self,
        view: CanvasView,
        sync: SceneSync,
        command_line: CommandLine,
        undo_stack: QUndoStack,
        snap_settings: SnapSettings,
    ) -> None:
        super().__init__()
        self._view = view
        self._sync = sync
        self._command_line = command_line
        self._undo_stack = undo_stack
        self._snap_settings = snap_settings
        self._document = Document()
        self._defaults = Defaults()
        self._selection = Selection()
        self._session: CommandSession | None = None
        self._last_command: str | None = None
        self._raw_cursor: Point | None = None
        self._cursor: Point | None = None
        self._press: Point | None = None
        undo_stack.indexChanged.connect(self._forget_vanished_selection)

    def set_document(self, document: Document) -> None:
        self._stop_session()
        self._selection.clear()
        self._document = document
        self._refresh()

    def start_command(self, name: str) -> None:
        self._stop_session()
        self._command_line.print(f"Command: {name}")
        self._last_command = name
        self._session = CommandSession(create(name, list(self._selection), self._defaults), self)
        self._refresh()
        self._command_line.focus()

    def handle_input(self, text: str) -> None:
        if self._session is not None:
            self._answer_with_text(text)
            return
        word = text.strip().upper()
        if word in UNDO_WORDS:
            self._undo_stack.undo()
        elif word in REDO_WORDS:
            self._undo_stack.redo()
        elif word == SELECT_ALL_WORD:
            self._select_all()
        else:
            self._start_typed_command(word)

    def selected(self) -> list[Entity]:
        return list(self._selection)

    def change_selected(self, **fields) -> None:
        """Give every selected object new values for the named properties, as one undo step."""
        changes = [(entity, replace(entity, **fields)) for entity in self._selection]
        changes = [(old, new) for old, new in changes if old != new]
        if changes:
            self.modify(changes)

    def erase_selection(self) -> None:
        if self._session is None and len(self._selection):
            self.start_command("ERASE")

    def cancel(self) -> None:
        """Esc: stop the running command, or when idle clear the selection."""
        if self._session is not None:
            self._session.cancel()
        else:
            self._selection.clear()
            self._publish_selection()
        self._refresh()

    def click(self, point: QPointF) -> None:
        prompt = self._prompt()
        if prompt is not None and prompt.kind in POINT_PROMPTS:
            snapped = self._snap((point.x(), point.y())).point
            self._command_line.print(f"{prompt.message} {snapped[0]:.4f},{snapped[1]:.4f}")
            self._session.submit_point(snapped)
            self._refresh()
        elif prompt is None or prompt.kind in SELECTING_PROMPTS:
            self._press = (point.x(), point.y())

    def release(self, point: QPointF) -> None:
        if self._press is None:
            return
        press, self._press = self._press, None
        self._view.set_selection_box(None)
        release = (point.x(), point.y())
        if math.dist(press, release) * self._view.zoom_scale < DRAG_THRESHOLD_PIXELS:
            self._pick_at(press)
        else:
            self._pick_in_box(press, release)
        self._refresh()

    def move_cursor(self, point: QPointF) -> None:
        self._raw_cursor = (point.x(), point.y())
        if self._press is not None:
            self._view.set_selection_box((QPointF(*self._press), point))
        self.reapply_cursor()

    def reapply_cursor(self) -> None:
        """Snap the last cursor position again, after the prompt, drawing or snap settings changed."""
        if self._raw_cursor is None:
            self._sync.clear_preview()
            return
        result = self._snap(self._raw_cursor)
        self._cursor = result.point
        marker = QPointF(*result.point) if result.kind else None
        self._view.set_snap_marker(marker, result.kind)
        self.cursor_snapped.emit(QPointF(*result.point))
        self._update_preview()

    # CommandHost: what a running command may do to the drawing.

    def add(self, entities: list[Entity]) -> None:
        self._undo_stack.push(AddEntities(self._document, self._sync, entities))

    def add_new(self, entities: list[Entity]) -> None:
        document = self._document
        for entity in entities:
            entity.layer = document.current_layer
            entity.color = document.current_color
            entity.linetype = document.current_linetype
        self.add(entities)

    def remove(self, entities: list[Entity]) -> None:
        self._undo_stack.push(RemoveEntities(self._document, self._sync, entities))

    def modify(self, changes: list[tuple[Entity, Entity]]) -> None:
        self._undo_stack.push(ModifyEntities(self._sync, changes))

    def retract(self) -> None:
        self._undo_stack.undo()

    def report(self, message: str) -> None:
        self._command_line.print(message)

    def begin_group(self) -> None:
        self._undo_stack.beginMacro("Edit")

    def end_group(self) -> None:
        self._undo_stack.endMacro()

    def _prompt(self) -> Prompt | None:
        return self._session.prompt if self._session else None

    def _stop_session(self) -> None:
        if self._session is not None:
            self._session.cancel()
            self._session = None

    def _snap(self, cursor: Point) -> SnapResult:
        prompt = self._prompt()
        if prompt is None or prompt.kind not in POINT_PROMPTS:
            return SnapResult(cursor, None)
        scale = self._view.zoom_scale
        aperture = SNAP_APERTURE_PIXELS / scale
        near = self._sync.entities_in(_square_around(cursor, aperture))
        return resolve_point(cursor, prompt.base, self._snap_settings, SnapScene(aperture, grid_spacing(scale), near))

    def _pick_at(self, point: Point) -> None:
        aperture = PICK_APERTURE_PIXELS / self._view.zoom_scale
        entity = pick_entity(self._editable_in(_square_around(point, aperture)), point, aperture)
        prompt = self._prompt()
        if prompt is not None and prompt.kind == "pick":
            if entity is not None:
                self._session.submit_pick(entity, point, self._shift_held())
            return
        self._change_selection([entity] if entity is not None else [])

    def _editable_in(self, area: QRectF) -> list[Entity]:
        return [entity for entity in self._sync.entities_in(area) if self._document.is_editable(entity)]

    def _pick_in_box(self, first: Point, second: Point) -> None:
        prompt = self._prompt()
        if prompt is None or prompt.kind == "select":
            box = QRectF(QPointF(*first), QPointF(*second)).normalized()
            self._change_selection(select_in_box(self._editable_in(box), first, second))

    def _change_selection(self, found: list[Entity]) -> None:
        """Apply a click or box to the selection. Shift toggles. An empty pick clears it, but not mid-command."""
        in_command = self._prompt() is not None
        if self._shift_held():
            toggled = [e for e in found if e in self._selection]
            self._selection.add([e for e in found if e not in toggled])
            self._selection.remove(toggled)
        elif in_command:
            self._selection.add(found)
        elif found:
            self._selection.replace(found)
        else:
            self._selection.clear()
        if found and in_command:
            self._command_line.print(f"{len(found)} found, {len(self._selection)} total")
        self._publish_selection()

    @staticmethod
    def _shift_held() -> bool:
        return bool(QGuiApplication.keyboardModifiers() & Qt.KeyboardModifier.ShiftModifier)

    def _select_all(self) -> None:
        everything = self._document.editable_entities()
        self._selection.add(everything)
        self._publish_selection()
        self._command_line.print(f"{len(everything)} found")

    def _start_typed_command(self, word: str) -> None:
        name = resolve(word) if word else self._last_command
        if name is None:
            if word:
                self._command_line.print(f"Unknown command: {word}")
            return
        self.start_command(name)

    def _answer_with_text(self, text: str) -> None:
        prompt = self._session.prompt
        self._command_line.print(f"{prompt.message} {text}")
        if prompt.kind == "select":
            self._answer_select_prompt(prompt, text.strip().upper())
        else:
            error = self._session.submit_text(text)
            if error:
                self._command_line.print(error)
        self._refresh()

    def _answer_select_prompt(self, prompt: Prompt, word: str) -> None:
        if word == SELECT_ALL_WORD:
            self._select_all()
        elif word:
            self._command_line.print(SELECT_HINT)
        elif len(self._selection) == 0 and prompt.or_all:
            self._session.submit_objects(self._document.visible_entities())
        elif len(self._selection) == 0:
            self._session.cancel()
        else:
            self._session.submit_objects(list(self._selection))

    def _forget_vanished_selection(self) -> None:
        self._selection.keep_only(self._document.editable_entities())
        self._publish_selection()

    def _publish_selection(self) -> None:
        self._sync.set_selected(self._selection)
        self.selection_changed.emit()

    def _refresh(self) -> None:
        if self._session is not None and not self._session.active:
            self._session = None
            self._selection.clear()
            self._publish_selection()
        prompt = self._prompt()
        self._command_line.set_prompt(prompt.message if prompt else IDLE_PROMPT)
        self._command_line.set_hint("" if prompt else IDLE_HINT)
        self._view.set_pickbox(prompt is None or prompt.kind in SELECTING_PROMPTS)
        self.command_changed.emit(self._last_command if self._session else None)
        self.prompt_changed.emit(self._last_command if prompt else None, prompt.message if prompt else "")
        self.reapply_cursor()

    def _update_preview(self) -> None:
        prompt = self._prompt()
        if prompt is None or prompt.preview is None or self._cursor is None:
            self._sync.clear_preview()
            return
        self._sync.set_preview(prompt.preview(self._cursor))























