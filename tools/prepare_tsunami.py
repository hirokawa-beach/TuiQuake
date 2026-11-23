"""Extract the forecast-area coastlines from JMA's tsunami GeoJSON."""

import argparse
import json
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("geojson", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()

    source = json.loads(args.geojson.read_bytes())
    coasts = {}
    for feature in source["features"]:
        code = feature["properties"]["code"]
        geometry = feature["geometry"]
        if code in coasts or geometry["type"] != "MultiLineString":
            raise ValueError(f"Unexpected geometry for tsunami district {code}")
        coasts[code] = geometry["coordinates"]

    rows = [
        f"  {json.dumps(code)}: {json.dumps(lines, separators=(',', ':'))}"
        for code, lines in sorted(coasts.items())
    ]
    args.output.write_text("{\n" + ",\n".join(rows) + "\n}\n", encoding="utf-8")


if __name__ == "__main__":
    main()
