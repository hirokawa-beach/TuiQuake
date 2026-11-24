import argparse
from itertools import product
from pathlib import Path
from struct import pack
from zipfile import ZipFile


def axis(end):
    return [*range(0, 50, 2), *range(50, 200, 5), *range(200, end + 1, 10)]


def pack_table(archive):
    with ZipFile(archive) as bundle:
        lines = bundle.read("tjma2001").decode("ascii").splitlines()
    grid = list(product(axis(700), axis(2000)))
    if len(lines) != len(grid):
        raise ValueError("Unexpected size for the JMA2001 table")
    result = bytearray()
    previous = (0, 0)
    for (depth, distance), line in zip(grid, lines, strict=True):
        p, p_time, s, s_time, row_depth, row_distance = line.split()
        if (p, s, int(row_depth), int(row_distance)) != ("P", "S", depth, distance):
            raise ValueError(f"Unexpected row: {line}")
        times = tuple(round(float(time) * 1000) for time in (p_time, s_time))
        if distance and any(current < last for current, last in zip(times, previous)):
            raise ValueError(f"Travel time decreased with distance: {line}")
        result.extend(pack("<II", *times))
        previous = times
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Pack the JMA2001 P/S travel-time table in milliseconds")
    parser.add_argument("archive", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    args.output.write_bytes(pack_table(args.archive))
