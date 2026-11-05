import os
import shutil
import tempfile
from pathlib import Path

import ezdxf
from ezdxf.addons import odafc

from linewright.io.dxf import document_from_drawing, drawing_from_document
from linewright.model.document import Document

ODA_FOLDER = Path(os.environ.get("ProgramFiles", r"C:\Program Files")) / "ODA"
DXF_SUFFIX = ".dxf"
DWG_SUFFIX = ".dwg"
FILE_FILTER = "Drawings (*.dwg *.dxf);;DWG (*.dwg);;DXF (*.dxf)"
ODA_MISSING = (
    "DWG files need the free ODA File Converter. Install it from opendesign.com, "
    "then try again."
)


class FileError(Exception):
    """A drawing could not be opened or saved. The message is meant for the user."""


def open_drawing(path: Path) -> Document:
    suffix = path.suffix.lower()
    try:
        if suffix == DXF_SUFFIX:
            drawing = ezdxf.readfile(path)
        elif suffix == DWG_SUFFIX:
            _require_oda()
            drawing = odafc.readfile(path)
        else:
            raise FileError(f"Unsupported file type: {path.suffix}")
    except (OSError, ezdxf.DXFError) as error:
        raise FileError(f"Could not open {path.name}: {error}") from error
    document = document_from_drawing(drawing)
    document.path = path
    return document


def save_drawing(document: Document, path: Path) -> None:
    suffix = path.suffix.lower()
    if suffix not in (DXF_SUFFIX, DWG_SUFFIX):
        raise FileError(f"Unsupported file type: {path.suffix}")
    if suffix == DWG_SUFFIX:
        _require_oda()
    drawing = drawing_from_document(document)
    # Stage in a temp folder so a failed export never damages an existing file.
    with tempfile.TemporaryDirectory(prefix="linewright_") as folder:
        staged = Path(folder) / path.name
        try:
            if suffix == DXF_SUFFIX:
                drawing.saveas(staged)
            else:
                odafc.export_dwg(drawing, staged)
            shutil.move(staged, path)
        except (OSError, ezdxf.DXFError) as error:
            raise FileError(f"Could not save {path.name}: {error}") from error
    document.path = path


def oda_available() -> bool:
    # The installer puts the exe in a versioned folder, which ezdxf does not search.
    installed = sorted(ODA_FOLDER.glob("ODAFileConverter*/ODAFileConverter.exe"))
    if installed:
        ezdxf.options.set("odafc-addon", "win_exec_path", str(installed[-1]))
    return odafc.is_installed()


def _require_oda() -> None:
    if not oda_available():
        raise FileError(ODA_MISSING)
