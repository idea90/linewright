from pathlib import Path

import ezdxf
import pytest
from ezdxf.addons import odafc

from linewright.io.dxf import document_from_drawing, drawing_from_document
from linewright.io.files import FileError, oda_available, open_drawing, save_drawing
from linewright.model.document import Document, Layer
from linewright.model.entities import Arc, Circle, Line, Polyline, Text

SAMPLE_ENTITIES = [
    Line((0, 0), (10, 5), layer="WALLS", color=1),
    Circle((3, 4), 2.5),
    Arc((0, 0), 5, 30, 120, layer="WALLS"),
    Polyline([(0, 0, 0.0), (4, 0, 0.5), (4, 3, 0.0)], closed=True, color=3),
    Text((1, 1), "Hello", 2.0, 45.0),
]


def sample_document() -> Document:
    document = Document(entities=list(SAMPLE_ENTITIES))
    document.layers["WALLS"] = Layer("WALLS", 5)
    return document


def test_entities_round_trip_through_a_drawing():
    drawing = drawing_from_document(sample_document())
    reread = document_from_drawing(drawing)
    assert reread.entities == SAMPLE_ENTITIES
    assert reread.layers["WALLS"].color == 5
    assert not reread.skipped


def test_dxf_file_round_trip(tmp_path):
    path = tmp_path / "plan.dxf"
    save_drawing(sample_document(), path)
    assert open_drawing(path).entities == SAMPLE_ENTITIES


def test_unsupported_and_extruded_entities_survive_a_save(tmp_path):
    source = ezdxf.new()
    msp = source.modelspace()
    msp.add_ellipse((0, 0), major_axis=(5, 0), ratio=0.5)
    msp.add_arc((0, 0), 3, 0, 90, dxfattribs={"extrusion": (0, 0, -1)})
    msp.add_line((0, 0), (1, 1))
    original = tmp_path / "in.dxf"
    source.saveas(original)

    document = open_drawing(original)
    assert dict(document.skipped) == {"ELLIPSE": 1, "ARC": 1}
    document.entities.append(Line((5, 5), (6, 6)))
    edited = tmp_path / "out.dxf"
    save_drawing(document, edited)

    kinds = sorted(e.dxftype() for e in ezdxf.readfile(edited).modelspace())
    assert kinds == ["ARC", "ELLIPSE", "LINE", "LINE"]


def test_deleted_entities_are_removed_on_save(tmp_path):
    path = tmp_path / "plan.dxf"
    save_drawing(sample_document(), path)
    document = open_drawing(path)
    document.entities.pop(0)
    save_drawing(document, path)
    assert len(open_drawing(path).entities) == len(SAMPLE_ENTITIES) - 1


def test_saving_twice_does_not_duplicate_entities(tmp_path):
    document = sample_document()
    path = tmp_path / "plan.dxf"
    save_drawing(document, path)
    save_drawing(document, path)
    assert len(ezdxf.readfile(path).modelspace()) == len(SAMPLE_ENTITIES)


def test_failed_save_keeps_the_existing_file(tmp_path):
    path = tmp_path / "plan.dxf"
    save_drawing(sample_document(), path)
    before = path.read_bytes()
    with pytest.raises(FileError):
        save_drawing(sample_document(), tmp_path / "missing-folder" / "plan.dxf")
    assert path.read_bytes() == before


def test_unreadable_dxf_reports_a_file_error(tmp_path):
    path = tmp_path / "broken.dxf"
    path.write_text("this is not a drawing")
    with pytest.raises(FileError):
        open_drawing(path)


def test_unknown_extension_is_rejected(tmp_path):
    with pytest.raises(FileError):
        open_drawing(tmp_path / "plan.pdf")


def test_dwg_without_oda_gives_install_message(monkeypatch):
    monkeypatch.setattr(odafc, "is_installed", lambda: False)
    with pytest.raises(FileError, match="ODA File Converter"):
        open_drawing(Path("plan.dwg"))
    with pytest.raises(FileError, match="ODA File Converter"):
        save_drawing(Document(), Path("plan.dwg"))


@pytest.mark.skipif(not oda_available(), reason="ODA File Converter not installed")
def test_dwg_file_round_trip(tmp_path, monkeypatch):
    monkeypatch.delenv("QT_QPA_PLATFORM", raising=False)  # the converter is a Qt app and crashes offscreen
    path = tmp_path / "plan.dwg"
    save_drawing(sample_document(), path)
    assert open_drawing(path).entities == SAMPLE_ENTITIES
