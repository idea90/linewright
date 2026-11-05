import math
from dataclasses import dataclass
from pathlib import Path

from PySide6.QtCore import QMarginsF, QRectF
from PySide6.QtGui import QPageLayout, QPageSize, QPainter, QPdfWriter

from linewright.canvas.layer import NO_DETAIL_LIMIT, EntityLayer, Look
from linewright.io.dxf import mm_per_unit
from linewright.model.document import Document

PLOT_DPI = 600
MM_PER_INCH = 25.4
MARGIN_MM = 10.0
LINE_WEIGHT_MM = 0.25
FIT = "Fit"
SCALE_HELP = "Enter a scale such as 1:50 or 2:1, or Fit."
NOTHING_TO_PLOT = "There is nothing to plot."
PAPER_SIZES = {
    "A4": QPageSize.PageSizeId.A4,
    "A3": QPageSize.PageSizeId.A3,
    "A2": QPageSize.PageSizeId.A2,
    "A1": QPageSize.PageSizeId.A1,
    "A0": QPageSize.PageSizeId.A0,
    "Letter": QPageSize.PageSizeId.Letter,
    "Legal": QPageSize.PageSizeId.Legal,
    "Tabloid": QPageSize.PageSizeId.Tabloid,
}


class PlotError(Exception):
    pass


@dataclass(frozen=True)
class PlotLayout:
    scale: float  # paper length per real length, so 0.02 is 1:50
    source: QRectF  # the part of the drawing that lands on the page, in drawing units
    fits: bool  # whether the whole drawing is on the page


def parse_scale(text: str) -> float | None:
    """Paper per real length from "1:50" or "2:1". None means fit the page. Raises ValueError for anything else."""
    text = text.strip()
    if text.lower() == FIT.lower():
        return None
    paper, colon, real = text.partition(":")
    try:
        scale = float(paper) / float(real)
    except (ValueError, ZeroDivisionError):
        scale = 0.0
    if not colon or not 0 < scale < math.inf:
        raise ValueError(SCALE_HELP)
    return scale


def format_scale(scale: float) -> str:
    return f"{scale:.4g}:1" if scale >= 1 else f"1:{1 / scale:.4g}"


def plot_layout(
    extent: QRectF, page: tuple[float, float], pixels_per_mm: float, mm_per_drawing_unit: float, scale: float | None
) -> PlotLayout:
    """Where the drawing lands on a page of the given size in pixels. Fit picks the largest scale that holds it all."""
    width, height = page
    pixels_per_real_length = pixels_per_mm * mm_per_drawing_unit
    if scale is None:
        fits_each = [room / size for room, size in ((width, extent.width()), (height, extent.height())) if size > 0]
        pixels_per_unit = min(fits_each, default=pixels_per_real_length)
    else:
        pixels_per_unit = pixels_per_real_length * scale
    source = QRectF(0, 0, width / pixels_per_unit, height / pixels_per_unit)
    source.moveCenter(extent.center())
    fits = extent.width() * pixels_per_unit <= width + 1e-6 and extent.height() * pixels_per_unit <= height + 1e-6
    return PlotLayout(pixels_per_unit / pixels_per_real_length, source, fits)


def paint_plot(
    document: Document, painter: QPainter, page: tuple[float, float], pixels_per_mm: float, scale: float | None
) -> PlotLayout:
    """Draw the visible part of a non-empty document on a page. Lines print at a fixed weight, not at drawing scale."""
    layer = EntityLayer(lambda: document, Look(width=LINE_WEIGHT_MM * pixels_per_mm, ink=True))
    layer.reload(document.visible_entities())
    layout = plot_layout(layer.extent(), page, pixels_per_mm, mm_per_unit(document), scale)
    width, height = page
    painter.save()
    painter.setRenderHint(QPainter.RenderHint.Antialiasing)
    painter.setClipRect(QRectF(0, 0, width, height))
    painter.translate(0, height)  # drawings are y-up, pages are y-down
    pixels_per_unit = width / layout.source.width()
    painter.scale(pixels_per_unit, -pixels_per_unit)
    painter.translate(-layout.source.left(), -layout.source.top())
    layer.paint(painter, layout.source, pixels_per_unit, NO_DETAIL_LIMIT)
    painter.restore()
    return layout


def export_pdf(
    document: Document, path: Path, paper: QPageSize.PageSizeId, landscape: bool, scale: float | None
) -> PlotLayout:
    if not document.visible_entities():
        raise PlotError(NOTHING_TO_PLOT)
    orientation = QPageLayout.Orientation.Landscape if landscape else QPageLayout.Orientation.Portrait
    writer = QPdfWriter(str(path))
    writer.setResolution(PLOT_DPI)
    margins = QMarginsF(MARGIN_MM, MARGIN_MM, MARGIN_MM, MARGIN_MM)
    writer.setPageLayout(QPageLayout(QPageSize(paper), orientation, margins, QPageLayout.Unit.Millimeter))
    painter = QPainter(writer)
    if not painter.isActive():
        raise PlotError(f"Could not write {path.name}.")
    try:
        return paint_plot(document, painter, (writer.width(), writer.height()), PLOT_DPI / MM_PER_INCH, scale)
    finally:
        painter.end()
