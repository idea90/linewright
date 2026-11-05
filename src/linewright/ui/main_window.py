from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path

from PySide6.QtCore import QEvent, QPointF, Qt
from PySide6.QtGui import QAction, QCloseEvent, QKeySequence, QShortcut, QUndoStack
from PySide6.QtWidgets import (
    QApplication,
    QDockWidget,
    QFileDialog,
    QGraphicsScene,
    QLabel,
    QMainWindow,
    QMenu,
    QMessageBox,
    QVBoxLayout,
    QWidget,
)

from linewright.canvas.plot import PlotError, export_pdf, format_scale
from linewright.canvas.sync import SceneSync
from linewright.canvas.view import ZOOM_STEP, CanvasView
from linewright.commands.registry import shortest_alias
from linewright.io.dxf import unit_names
from linewright.io.files import DXF_SUFFIX, FILE_FILTER, FileError, save_drawing
from linewright.model.document import Document
from linewright.snap.resolve import SnapSettings
from linewright.ui.chrome import DocumentTab, Header
from linewright.ui.command_line import CommandLine
from linewright.ui.controller import DrawingController
from linewright.ui.frame_resize import FrameResizer
from linewright.ui.icons import command_icon
from linewright.ui.layer_palette import LayerPalette
from linewright.ui.layers import LayerService
from linewright.ui.loading_overlay import LoadingOverlay
from linewright.ui.navbar import NavigationBar
from linewright.ui.open_job import OpenJob
from linewright.ui.plot_dialog import PlotDialog
from linewright.ui.prompt_hint import PromptHint
from linewright.ui.properties_panels import LayerPanel, PropertiesPanel
from linewright.ui.ribbon import Ribbon
from linewright.ui.snap_toggles import SnapToggles

WINDOW_TITLE = "Linewright"
UNTITLED = "Drawing1"
PDF_SUFFIX = ".pdf"
PDF_FILTER = "PDF (*.pdf)"
COORDS_MIN_WIDTH = 170
STATUS_EDGE_GAP = 8
TOOL_LABELS = {
    "LINE": "Line",
    "PLINE": "Polyline",
    "RECTANGLE": "Rectangle",
    "CIRCLE": "Circle",
    "ARC": "Arc",
    "TEXT": "Text",
    "ERASE": "Erase",
    "MOVE": "Move",
    "COPY": "Copy",
    "ROTATE": "Rotate",
    "SCALE": "Scale",
    "MIRROR": "Mirror",
    "OFFSET": "Offset",
    "TRIM": "Trim",
    "EXTEND": "Extend",
    "FILLET": "Fillet",
    "DIMLINEAR": "Dimension",
    "DIMALIGNED": "Aligned dimension",
    "DIMSCALE": "Dimension scale",
    "DISTANCE": "Distance",
    "AREA": "Area",
}


@contextmanager
def busy_cursor() -> Iterator[None]:
    """Show the wait cursor while a file is read or written, which can take seconds for a large drawing."""
    QApplication.setOverrideCursor(Qt.CursorShape.WaitCursor)
    try:
        yield
    finally:
        QApplication.restoreOverrideCursor()


class MainWindow(QMainWindow):
    def __init__(self) -> None:
        super().__init__()
        self.setWindowFlag(Qt.WindowType.FramelessWindowHint)  # the header is the title bar
        self.resize(1280, 800)
        self.scene = QGraphicsScene(self)
        self.view = CanvasView(self.scene)
        self.setCentralWidget(self.view)
        self.sync = SceneSync(self.scene)
        self.view.set_extent_source(self.sync.extent)
        self.undo_stack = QUndoStack(self)

        self.command_line = CommandLine()
        dock = QDockWidget("Command", self)
        dock.setWidget(self.command_line)
        dock.setFeatures(QDockWidget.DockWidgetFeature.NoDockWidgetFeatures)
        dock.setTitleBarWidget(QWidget())
        self.addDockWidget(Qt.DockWidgetArea.BottomDockWidgetArea, dock)

        self.snap_settings = SnapSettings()
        self.controller = DrawingController(
            self.view, self.sync, self.command_line, self.undo_stack, self.snap_settings
        )
        self._build_status_bar()
        self._tool_actions: dict[str, QAction] = {}
        self._build_layer_tools()
        self._build_chrome()
        self.navigation = NavigationBar(self.view)
        self.hint = PromptHint(self.view)
        self.loading = LoadingOverlay(self.view)
        self._opening: OpenJob | None = None
        self._frame_resizer = FrameResizer(self)
        self._connect_signals()
        self._apply_snap_settings()
        self.set_document(Document())
        self._announce_new()
        self.command_line.focus()

    def _build_status_bar(self) -> None:
        self._coords = QLabel("0.0000, 0.0000")
        self._coords.setObjectName("coords")
        self._coords.setMinimumWidth(COORDS_MIN_WIDTH)
        self._layer_info = QLabel()
        self._layer_info.setObjectName("statusInfo")
        self._units_info = QLabel()
        self._units_info.setObjectName("statusInfo")
        for label in (self._coords, self._layer_info, self._units_info):
            self.statusBar().addWidget(label)
        self.statusBar().setSizeGripEnabled(False)
        self.snap_toggles = SnapToggles(self, self.snap_settings)
        edge_gap = QWidget()
        edge_gap.setFixedWidth(STATUS_EDGE_GAP)
        self.statusBar().addPermanentWidget(edge_gap)

    def _build_layer_tools(self) -> None:
        self.layer_service = LayerService(self.sync, self.undo_stack)
        self.layer_palette = LayerPalette(lambda: self.document, self.layer_service, self.command_line.print)
        self.layer_dock = QDockWidget("Layer Properties", self)
        self.layer_dock.setWidget(self.layer_palette)
        self.layer_dock.setFeatures(QDockWidget.DockWidgetFeature.DockWidgetClosable)
        self.addDockWidget(Qt.DockWidgetArea.LeftDockWidgetArea, self.layer_dock)
        self.layer_dock.hide()
        palette_action = self.layer_dock.toggleViewAction()
        palette_action.setIcon(command_icon("LAYERS"))
        palette_action.setText("Manage layers")
        self.layer_panel = LayerPanel(
            lambda: self.document, self.controller, self.layer_service, self.command_line.print, palette_action
        )
        self.properties_panel = PropertiesPanel(lambda: self.document, self.controller)

    def _build_chrome(self) -> None:
        for name in TOOL_LABELS:
            self._tool_action(name)
        new = self._file_action("New", self._new, QKeySequence.StandardKey.New, "NEW")
        open_ = self._file_action("Open...", self._choose_and_open, QKeySequence.StandardKey.Open, "OPEN")
        save = self._file_action("Save", self._save, QKeySequence.StandardKey.Save, "SAVE")
        save_as = self._file_action("Save As...", self._save_as, QKeySequence.StandardKey.SaveAs)
        plot = self._file_action("Export PDF...", self._export_pdf, QKeySequence.StandardKey.Print, "PLOT")
        undo = self._history_action(
            self.undo_stack.createUndoAction(self, "Undo"),
            QKeySequence.StandardKey.Undo,
            "UNDO",
        )
        redo = self._history_action(
            self.undo_stack.createRedoAction(self, "Redo"),
            QKeySequence.StandardKey.Redo,
            "REDO",
        )
        QShortcut(QKeySequence("Ctrl+E"), self, self.view.zoom_extents)

        self._file_actions = [new, open_, save, save_as, plot, undo, redo]
        menu = QMenu(self)
        menu.addActions([new, open_, save, save_as, plot])
        self.doc_tab = DocumentTab()
        chrome = QWidget()
        layout = QVBoxLayout(chrome)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(0)
        self.header = Header(menu, [new, open_, save, plot, None, undo, redo])
        layout.addWidget(self.header)
        self.ribbon = Ribbon(self._tool_actions, [self.layer_panel, self.properties_panel])
        layout.addWidget(self.ribbon)
        layout.addWidget(self.doc_tab)
        self.setMenuWidget(chrome)
        self.setMinimumWidth(chrome.minimumSizeHint().width())  # narrower would overlap the ribbon

    def _tool_action(self, name: str) -> None:
        action = QAction(command_icon(name), TOOL_LABELS[name], self)
        action.setCheckable(True)
        action.setToolTip(f"{TOOL_LABELS[name]} ({shortest_alias(name)})")
        action.triggered.connect(lambda _checked=False, command=name: self.controller.start_command(command))
        self._tool_actions[name] = action

    def _file_action(self, text: str, handler, shortcut: QKeySequence, icon_name: str | None = None) -> QAction:
        action = QAction(text, self)
        if icon_name:
            action.setIcon(command_icon(icon_name))
        action.setShortcut(shortcut)
        action.setToolTip(f"{text.rstrip('.')} ({action.shortcut().toString()})")
        action.triggered.connect(handler)
        self.addAction(action)
        return action

    def _history_action(self, action: QAction, shortcut: QKeySequence, icon_name: str) -> QAction:
        action.setIcon(command_icon(icon_name))
        action.setShortcut(shortcut)
        action.setToolTip(f"{action.text().replace('&', '')} ({action.shortcut().toString()})")
        self.addAction(action)
        return action

    def _connect_signals(self) -> None:
        self.command_line.submitted.connect(self.controller.handle_input)
        self.view.clicked.connect(self.controller.click)
        self.view.released.connect(self.controller.release)
        self.command_line.delete_pressed.connect(self.controller.erase_selection)
        self.view.cursor_moved.connect(self.controller.move_cursor)
        self.controller.cursor_snapped.connect(self._show_coordinates)
        self.controller.command_changed.connect(self._check_running_tool)
        self.controller.prompt_changed.connect(self._show_prompt_hint)
        self.doc_tab.close_requested.connect(self._new)
        self.doc_tab.new_requested.connect(self._new)
        self.snap_toggles.changed.connect(self._apply_snap_settings)
        self.navigation.zoom_extents.connect(self.view.zoom_extents)
        self.navigation.zoom_in.connect(lambda: self.view.zoom_by(ZOOM_STEP))
        self.navigation.zoom_out.connect(lambda: self.view.zoom_by(1 / ZOOM_STEP))
        self.undo_stack.cleanChanged.connect(self._refresh_title)
        self.undo_stack.indexChanged.connect(self._refresh_properties)
        self.controller.selection_changed.connect(self._refresh_properties)
        QShortcut(QKeySequence(Qt.Key.Key_Escape), self, self.controller.cancel)

    def _apply_snap_settings(self) -> None:
        self.view.set_grid_visible(self.snap_settings.show_grid)
        self.controller.reapply_cursor()

    def _check_running_tool(self, running: str | None) -> None:
        for name, action in self._tool_actions.items():
            action.setChecked(name == running)

    def _show_prompt_hint(self, command: str | None, message: str) -> None:
        if command is None:
            self.hint.hide()
        else:
            self.hint.show_prompt(command, TOOL_LABELS[command], message)

    def set_document(self, document: Document) -> None:
        self.document = document
        self.sync.reload(document)
        self.controller.set_document(document)
        self.layer_service.set_document(document)
        self.undo_stack.clear()
        self.undo_stack.setClean()
        self.view.zoom_extents()
        self._refresh_title()
        self._refresh_properties()
        self._report_skipped(document)

    def _refresh_properties(self) -> None:
        self.layer_palette.refresh()
        self.layer_panel.refresh()
        self.properties_panel.refresh()
        self._layer_info.setText(f"Layer {self.document.current_layer}")
        self._units_info.setText(f"Units {unit_names(self.document)[1]}")

    def _refresh_title(self) -> None:
        name = self.document.path.name if self.document.path else UNTITLED
        modified = not self.undo_stack.isClean()
        title = f"{name}{'*' if modified else ''} - {WINDOW_TITLE}"
        self.setWindowTitle(title)
        self.header.set_title(f"{name}{'*' if modified else ''}")
        self.doc_tab.set_title(name, modified)

    def _report_skipped(self, document: Document) -> None:
        if not document.skipped:
            return
        kinds = ", ".join(f"{count} {kind}" for kind, count in sorted(document.skipped.items()))
        self.command_line.print(f"Not shown yet (kept when saving): {kinds}")

    def open_in_background(self, path: Path) -> None:
        """Open a drawing without freezing the window. Everything is held still until it arrives or fails."""
        if self._opening is not None:
            return
        self._opening = OpenJob(path)
        self._opening.opened.connect(self._opened)
        self._opening.failed.connect(self._open_failed)
        self._hold_still(True)
        self.loading.show_opening(path.name)
        self._opening.start()

    def _opened(self, document: Document) -> None:
        self._stop_waiting()
        self.set_document(document)

    def _open_failed(self, message: str) -> None:
        self._stop_waiting()
        self.show_error(message)

    def _stop_waiting(self) -> None:
        self._opening = None
        self.loading.hide()
        self._hold_still(False)

    def _hold_still(self, on: bool) -> None:
        for widget in (self.command_line, self.ribbon):
            widget.setEnabled(not on)
        for action in self._file_actions:
            action.setEnabled(not on)

    def save_path(self, path: Path) -> bool:
        try:
            with busy_cursor():
                save_drawing(self.document, path)
        except FileError as error:
            self.show_error(str(error))
            return False
        self.undo_stack.setClean()
        self._refresh_title()
        return True

    def show_error(self, message: str) -> None:
        QMessageBox.warning(self, WINDOW_TITLE, message)

    def _confirm_discard(self) -> bool:
        if self.undo_stack.isClean():
            return True
        name = self.document.path.name if self.document.path else UNTITLED
        buttons = QMessageBox.StandardButton
        choice = QMessageBox.question(
            self,
            WINDOW_TITLE,
            f"Save changes to {name}?",
            buttons.Save | buttons.Discard | buttons.Cancel,
        )
        if choice == buttons.Save:
            return self._save()
        return choice == buttons.Discard

    def changeEvent(self, event: QEvent) -> None:
        if event.type() == QEvent.Type.WindowStateChange:
            self.header.set_maximized(self.isMaximized())
        super().changeEvent(event)

    def closeEvent(self, event: QCloseEvent) -> None:
        if self._confirm_discard():
            event.accept()
        else:
            event.ignore()

    def _new(self) -> None:
        if self._opening is None and self._confirm_discard():
            self.set_document(Document())
            self._announce_new()

    def _announce_new(self) -> None:
        self.command_line.print(f"{UNTITLED} created. Units: {unit_names(self.document)[0]}.")

    def _choose_and_open(self) -> None:
        if not self._confirm_discard():
            return
        name, _ = QFileDialog.getOpenFileName(self, "Open drawing", "", FILE_FILTER)
        if name:
            self.open_in_background(Path(name))

    def _save(self) -> bool:
        if self.document.path:
            return self.save_path(self.document.path)
        return self._save_as()

    def _save_as(self) -> bool:
        name, _ = QFileDialog.getSaveFileName(self, "Save drawing", "", FILE_FILTER)
        if not name:
            return False
        path = Path(name)
        return self.save_path(path if path.suffix else path.with_suffix(DXF_SUFFIX))

    def _export_pdf(self) -> None:
        dialog = PlotDialog(self, unit_names(self.document)[0])
        if not dialog.exec():
            return
        default = self.document.path.with_suffix(PDF_SUFFIX).name if self.document.path else ""
        name, _ = QFileDialog.getSaveFileName(self, "Export PDF", default, PDF_FILTER)
        if not name:
            return
        path = Path(name)
        try:
            layout = export_pdf(self.document, path, dialog.paper, dialog.landscape, dialog.scale)
        except PlotError as error:
            self.show_error(str(error))
            return
        self.command_line.print(f"Exported {path.name} at {format_scale(layout.scale)}")
        if not layout.fits:
            self.command_line.print("Part of the drawing falls outside the page at this scale.")

    def _show_coordinates(self, point: QPointF) -> None:
        self._coords.setText(f"{point.x():.4f}, {point.y():.4f}")



















