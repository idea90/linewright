import re

import pytest
from PySide6.QtCore import QRectF
from PySide6.QtGui import QColor, QImage, QPageSize, QPainter
from PySide6.QtWidgets import QDialog

from linewright.canvas.plot import FIT, PlotError, export_pdf, format_scale, paint_plot, parse_scale, plot_layout
from linewright.io.dxf import drawing_from_document
from linewright.model.dimension import dimension_parts
from linewright.model.document import Document, Layer
from linewright.model.entities import Dimension, Line
from linewright.ui.plot_dialog import PlotDialog

PAGE = (2000, 1000)
PIXELS_PER_MM = 10.0
MIDDLE_ROW = 500
CAP = 3  # a flat 0.25 mm line is 2.5 px wide here and its square caps stick out half of that at each end


@pytest.fixture(autouse=True)
def _app(qapp):
    """Text metrics and painting need a running application."""


def render(document, scale):
    image = QImage(*PAGE, QImage.Format.Format_RGB32)
    image.fill(QColor("white"))
    painter = QPainter(image)
    layout = paint_plot(document, painter, PAGE, PIXELS_PER_MM, scale)
    painter.end()
    return image, layout


def dark_run(image, row):
    """The first and last dark pixel in a row of the page, or None."""
    dark = [x for x in range(image.width()) if image.pixelColor(x, row).lightness() < 128]
    return (dark[0], dark[-1]) if dark else None


def meters(document):
    document.source = drawing_from_document(document)
    document.source.header["$INSUNITS"] = 6
    return document


class TestScaleText:
    @pytest.mark.parametrize(("text", "scale"), [("1:50", 0.02), ("2:1", 2.0), (" 1 : 1 ", 1.0), ("1:2.5", 0.4)])
    def test_ratios_give_paper_per_real_length(self, text, scale):
        assert parse_scale(text) == pytest.approx(scale)

    @pytest.mark.parametrize("text", [FIT, "fit", "  FIT "])
    def test_fit_means_no_fixed_scale(self, text):
        assert parse_scale(text) is None

    @pytest.mark.parametrize("text", ["", "50", "0:1", "1:0", "-1:2", "a:b", "1:2:3", "nan:1", "inf:1"])
    def test_anything_else_is_refused(self, text):
        with pytest.raises(ValueError):
            parse_scale(text)

    @pytest.mark.parametrize(("scale", "text"), [(0.02, "1:50"), (2.0, "2:1"), (1.0, "1:1"), (0.4, "1:2.5")])
    def test_scales_read_back_the_way_they_are_typed(self, scale, text):
        assert format_scale(scale) == text


class TestLayout:
    EXTENT = QRectF(0, 0, 100, 50)

    def test_fit_takes_the_tighter_direction_and_centers_the_drawing(self):
        layout = plot_layout(self.EXTENT, PAGE, PIXELS_PER_MM, 1.0, None)
        assert layout.scale == pytest.approx(2.0)  # 100 mm across 200 mm of page
        assert layout.fits
        assert layout.source.center().x() == pytest.approx(50)
        assert layout.source.center().y() == pytest.approx(25)

    def test_a_fixed_scale_sizes_the_page_in_drawing_units(self):
        layout = plot_layout(self.EXTENT, PAGE, PIXELS_PER_MM, 1.0, 0.5)
        assert layout.source.width() == pytest.approx(400)  # 200 mm of page at 1:2
        assert layout.fits

    def test_the_drawing_unit_sets_how_much_paper_a_unit_takes(self):
        layout = plot_layout(self.EXTENT, PAGE, PIXELS_PER_MM, 1000.0, 0.01)
        assert layout.source.width() == pytest.approx(20)  # a meter drawn at 1:100 is 10 mm of paper

    def test_a_drawing_larger_than_the_page_is_reported(self):
        assert not plot_layout(QRectF(0, 0, 500, 50), PAGE, PIXELS_PER_MM, 1.0, 1.0).fits

    def test_a_single_point_does_not_divide_by_zero(self):
        assert plot_layout(QRectF(5, 5, 0, 0), PAGE, PIXELS_PER_MM, 1.0, None).scale == pytest.approx(1.0)


class TestPaperOutput:
    LINE = [Line((0, 0), (100, 0))]

    @pytest.mark.parametrize(("scale", "millimeters"), [(1.0, 100), (0.5, 50), (0.1, 10)])
    def test_a_line_is_the_right_length_on_paper(self, scale, millimeters):
        image, _ = render(Document(entities=list(self.LINE)), scale)
        first, last = dark_run(image, MIDDLE_ROW)
        assert last - first == pytest.approx(millimeters * PIXELS_PER_MM, abs=2 * CAP)

    def test_the_drawing_is_centered_on_the_page(self):
        image, _ = render(Document(entities=list(self.LINE)), 1.0)
        first, last = dark_run(image, MIDDLE_ROW)
        assert (first + last) / 2 == pytest.approx(PAGE[0] / 2, abs=CAP)

    def test_fit_stretches_the_drawing_to_the_page_width(self):
        image, layout = render(Document(entities=list(self.LINE)), None)
        first, last = dark_run(image, MIDDLE_ROW)
        assert first <= CAP and last >= PAGE[0] - 1 - CAP
        assert layout.scale == pytest.approx(2.0)

    def test_a_meter_drawing_plotted_at_1_to_100_is_10_mm_a_meter(self):
        document = meters(Document(entities=[Line((0, 0), (1, 0))]))
        image, _ = render(document, 0.01)
        first, last = dark_run(image, MIDDLE_ROW)
        assert last - first == pytest.approx(10 * PIXELS_PER_MM, abs=2 * CAP)

    def test_up_in_the_drawing_is_up_on_the_page(self):
        image, _ = render(Document(entities=[Line((0, 0), (100, 0)), Line((0, 0), (0, 50))]), 1.0)
        corner_x, corner_y = 500, 750
        assert image.pixelColor(corner_x, corner_y - 200).lightness() < 128  # on the vertical line, above the corner
        assert image.pixelColor(corner_x, corner_y + 100).lightness() > 200  # nothing below it
        assert dark_run(image, corner_y) == pytest.approx((500, 1500), abs=CAP)

    def test_white_lines_print_black_and_other_colors_stay(self):
        white = Line((0, 0), (100, 0), color=7)
        red = Line((0, 10), (100, 10), color=1)
        image, _ = render(Document(entities=[white, red]), 1.0)
        ink = image.pixelColor(1000, MIDDLE_ROW + 50)  # the drawing is 10 mm tall, centered, so y=0 is 50 px low
        assert (ink.red(), ink.green(), ink.blue()) == (0, 0, 0)
        red_ink = image.pixelColor(1000, MIDDLE_ROW - 50)
        assert red_ink.red() > 200 and red_ink.green() < 60

    def test_hidden_layers_are_not_plotted_or_counted_in_the_extent(self):
        document = Document(entities=[Line((0, 0), (100, 0)), Line((0, 0), (5000, 0), layer="Gone")])
        document.layers["Gone"] = Layer("Gone", visible=False)
        _, layout = render(document, None)
        assert layout.scale == pytest.approx(2.0)

    def test_a_dimension_plots_its_line_and_its_text(self):
        dim = Dimension((0, 0), (100, 0), (50, 12))
        image, layout = render(Document(entities=[dim]), 1.0)
        assert layout.fits

        def page_row(y):
            return round(PAGE[1] - (y - layout.source.y()) * PIXELS_PER_MM)

        parts = dimension_parts(dim)
        assert dark_run(image, page_row(12)) is not None
        text_rows = range(page_row(parts.text_anchor[1] + 2), page_row(parts.text_anchor[1] + 0.5))
        middle = PAGE[0] // 2
        assert any(image.pixelColor(x, y).lightness() < 128 for y in text_rows for x in range(middle - 40, middle + 40))


class TestPdf:
    DOCUMENT = Document(entities=[Line((0, 0), (100, 0)), Line((0, 0), (0, 50))])

    def test_the_pdf_has_the_chosen_paper_size(self, tmp_path, qapp):
        path = tmp_path / "plan.pdf"
        export_pdf(self.DOCUMENT, path, QPageSize.PageSizeId.A4, True, None)
        data = path.read_bytes()
        assert data.startswith(b"%PDF")
        width, height = map(float, re.search(rb"/MediaBox \[\s*0 0 ([\d.]+) ([\d.]+)", data).groups())
        assert (width, height) == pytest.approx((841.89, 595.28), abs=1.0)

    def test_portrait_swaps_the_page_around(self, tmp_path, qapp):
        path = tmp_path / "plan.pdf"
        export_pdf(self.DOCUMENT, path, QPageSize.PageSizeId.Letter, False, 0.02)
        width, height = map(float, re.search(rb"/MediaBox \[\s*0 0 ([\d.]+) ([\d.]+)", path.read_bytes()).groups())
        assert (width, height) == pytest.approx((612, 792), abs=1.0)

    def test_the_result_says_what_scale_was_used(self, tmp_path, qapp):
        layout = export_pdf(self.DOCUMENT, tmp_path / "plan.pdf", QPageSize.PageSizeId.A3, True, 0.02)
        assert layout.scale == 0.02
        assert layout.fits

    def test_an_empty_drawing_is_refused_and_no_file_is_made(self, tmp_path, qapp):
        path = tmp_path / "plan.pdf"
        with pytest.raises(PlotError):
            export_pdf(Document(), path, QPageSize.PageSizeId.A4, True, None)
        assert not path.exists()

    def test_a_path_that_cannot_be_written_is_reported(self, tmp_path, qapp):
        with pytest.raises(PlotError):
            export_pdf(self.DOCUMENT, tmp_path, QPageSize.PageSizeId.A4, True, None)


class TestDialog:
    def test_a_good_scale_closes_the_dialog_and_is_read_back(self, qtbot):
        dialog = PlotDialog(None, "millimeters")
        qtbot.addWidget(dialog)
        dialog._scale.setCurrentText("1:50")
        dialog.accept()
        assert dialog.result() == QDialog.DialogCode.Accepted
        assert dialog.scale == pytest.approx(0.02)
        assert dialog.landscape
        assert dialog.paper == QPageSize.PageSizeId.A4

    def test_a_bad_scale_keeps_the_dialog_open_and_says_why(self, qtbot):
        dialog = PlotDialog(None, "millimeters")
        qtbot.addWidget(dialog)
        dialog._scale.setCurrentText("big")
        dialog.accept()
        assert dialog.result() != QDialog.DialogCode.Accepted
        assert dialog._error.text()

    def test_fit_is_the_starting_choice(self, qtbot):
        dialog = PlotDialog(None, "millimeters")
        qtbot.addWidget(dialog)
        dialog.accept()
        assert dialog.scale is None


class TestExportFromTheWindow:
    def test_export_asks_then_writes_and_reports_the_scale(self, window, tmp_path, monkeypatch):
        window.set_document(Document(entities=[Line((0, 0), (100, 0))]))
        target = tmp_path / "out.pdf"
        monkeypatch.setattr(PlotDialog, "exec", lambda self: self.accept() or 1)
        monkeypatch.setattr("linewright.ui.main_window.QFileDialog.getSaveFileName", lambda *args: (str(target), ""))
        window._export_pdf()
        assert target.read_bytes().startswith(b"%PDF")
        assert "Exported out.pdf at" in window.command_line._history.toPlainText()

    def test_an_empty_drawing_shows_an_error_and_writes_nothing(self, window, tmp_path, monkeypatch):
        shown = []
        target = tmp_path / "out.pdf"
        monkeypatch.setattr(window, "show_error", shown.append)
        monkeypatch.setattr(PlotDialog, "exec", lambda self: self.accept() or 1)
        monkeypatch.setattr("linewright.ui.main_window.QFileDialog.getSaveFileName", lambda *args: (str(target), ""))
        window._export_pdf()
        assert len(shown) == 1
        assert not target.exists()
