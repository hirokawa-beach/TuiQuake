"""Convert JMA earthquake areas to TuiQuake polygons with shared boundaries."""

import argparse
import json
from pathlib import Path
import shutil
import subprocess
from tempfile import TemporaryDirectory
from zipfile import ZipFile


def convert_regions(archive, mapshaper):
    with TemporaryDirectory() as folder, ZipFile(archive, metadata_encoding="cp932") as data:
        base = Path(folder) / "areas"
        for extension in (".shp", ".shx", ".dbf"):
            (member,) = (name for name in data.namelist() if name.lower().endswith(extension))
            with data.open(member) as source, base.with_suffix(extension).open("wb") as target:
                shutil.copyfileobj(source, target)

        output = Path(folder) / "regions.geojson"
        subprocess.run([
            "node", "--max-old-space-size=4096", str(mapshaper),
            "-i", str(base.with_suffix(".shp")), "encoding=utf8",
            "-filter", "String(code).trim().length > 0",
            "-filter-islands", "min-area=50000m2",
            "-clean", "gap-width=30m",
            "-simplify", "dp", "interval=190m", "keep-shapes",
            "-o", "format=geojson", "precision=0.00001", str(output),
        ], check=True)
        source = json.loads(output.read_text(encoding="utf-8"))

    regions = {}
    for feature in source["features"]:
        code = str(feature["properties"]["code"])
        name = feature["properties"]["name"]
        geometry = feature["geometry"]
        if geometry is None:
            raise ValueError(f"No polygons for earthquake area {code}")
        polygons = geometry["coordinates"]
        if geometry["type"] == "Polygon":
            polygons = [polygons]
        elif geometry["type"] != "MultiPolygon":
            raise ValueError(f"Unexpected geometry for earthquake area {code}")
        if code in regions:
            raise ValueError(f"Duplicate earthquake area code {code}")
        regions[code] = [name, polygons]

    if not regions:
        raise ValueError("The dataset contains no earthquake areas")
    return regions


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path, help="JMA 地震情報／細分区域 ZIP archive")
    parser.add_argument("output", type=Path, help="output regions.json")
    parser.add_argument("--mapshaper", type=Path, default=Path("node_modules/mapshaper/bin/mapshaper"))
    args = parser.parse_args()
    regions = convert_regions(args.archive, args.mapshaper.resolve())
    args.output.write_text(
        json.dumps(regions, ensure_ascii=False, separators=(",", ":"), sort_keys=True) + "\n",
        encoding="utf-8",
        newline="\n",
    )


if __name__ == "__main__":
    main()
