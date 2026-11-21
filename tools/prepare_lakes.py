"""Convert MLIT W09 lake outlines into TuiQuake's lake polygons."""

import argparse
import json
from pathlib import Path
import subprocess
from tempfile import TemporaryDirectory
from zipfile import ZipFile


MIN_LAKE_AREA_M2 = 10_000_000


def convert_lakes(archive, mapshaper):
    with TemporaryDirectory() as folder, ZipFile(archive) as data:
        base = Path(folder) / "W09-05-g_Lake"
        for extension in (".shp", ".shx", ".dbf"):
            data.extract(base.with_suffix(extension).name, folder)

        output = Path(folder) / "lakes.geojson"
        subprocess.run([
            "node", str(mapshaper),
            "-i", str(base.with_suffix(".shp")), "encoding=shift_jis",
            "-filter", f"this.area >= {MIN_LAKE_AREA_M2}",
            "-simplify", "dp", "interval=60m", "keep-shapes",
            "-o", "format=geojson", "precision=0.00001", str(output),
        ], check=True)
        source = json.loads(output.read_text(encoding="utf-8"))

    lakes = []
    for feature in source["features"]:
        geometry = feature["geometry"]
        if geometry is None:
            raise ValueError("A lake outline was lost during simplification")
        if geometry["type"] == "Polygon":
            lakes.append(geometry["coordinates"])
        elif geometry["type"] == "MultiPolygon":
            lakes.extend(geometry["coordinates"])
        else:
            raise ValueError(f"Unexpected lake geometry: {geometry['type']}")
    if not lakes:
        raise ValueError("The dataset contains no lake outlines")
    print(f"Converted {len(source['features'])} lake features into {len(lakes)} polygons.")
    return lakes


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path, help="MLIT W09-05_GML.zip archive")
    parser.add_argument("output", type=Path, help="output lakes.json")
    parser.add_argument("--mapshaper", type=Path, default=Path("node_modules/mapshaper/bin/mapshaper"))
    args = parser.parse_args()
    lakes = convert_lakes(args.archive, args.mapshaper.resolve())
    args.output.write_text(json.dumps(lakes, separators=(",", ":")) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
