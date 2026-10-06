#!/usr/bin/env python3
"""Check two-half DirectSound streaming deadlines in wire's API log.

Uses mixer frames, rather than Present counts or process exit status. This is
for a fixed-rate buffer whose playback rate equals the 44100 Hz output clock.
"""
import argparse
import json
import re
import statistics


def stats(fills):
    if not fills:
        return {"fills": 0}
    seconds = sorted(item["fill_seconds"] for item in fills)
    return {
        "fills": len(fills),
        "median_seconds": statistics.median(seconds),
        "p95_seconds": seconds[int(0.95 * (len(seconds) - 1))],
        "max_seconds": seconds[-1],
        "over_one_half": sum(item["over_one_half"] for item in fills),
        "deadline_misses": sum(item["deadline_miss"] for item in fills),
        "active_half_at_lock": sum(item["active_at_lock"] for item in fills),
        "same_half_again": sum(item["same_half_again"] for item in fills),
    }


def analyze(path, handle, buffer_bytes, block_align):
    ring = buffer_bytes // block_align
    half_bytes = buffer_bytes // 2
    half = ring // 2
    pattern = re.compile(
        r"IDirectSoundBuffer\.(Lock|Unlock|Play|Stop|SetCurrentPosition|SetFrequency)"
        r"\((0x[0-9a-f]+),?\s*(.*?)\) = 0x0 ;.*?frames=(\d+)"
    )
    playing = False
    origin = 0
    position = 0
    pending = None
    previous_half = None
    fills = []
    unsupported_frequency = False
    first_play = None
    last_frame = 0
    with open(path, encoding="utf-8", errors="replace") as log:
        for line in log:
            match = pattern.search(line)
            if not match or int(match[2], 16) != handle:
                continue
            method = match[1]
            args = [int(n.strip(), 16) for n in match[3].split(",") if n.strip()]
            frame = int(match[4])
            last_frame = frame
            if method == "SetFrequency":
                unsupported_frequency |= args[0] not in (0, 44100)
            elif method == "SetCurrentPosition":
                position = args[0] // block_align
                if playing:
                    origin = frame - position
                previous_half = None
            elif method == "Play":
                if not playing:
                    origin = frame - position
                    previous_half = None
                playing = True
                if first_play is None:
                    first_play = frame
            elif method == "Stop":
                if playing:
                    position = (frame - origin) % ring
                playing = False
                pending = None
                previous_half = None
            elif method == "Lock":
                pending = None
                if playing and args[0] in (0, half_bytes) and args[1] == half_bytes:
                    pending = (frame, args[0] // block_align)
            elif method == "Unlock" and pending is not None:
                start, offset = pending
                phase = (start - origin) % ring
                active = offset <= phase < offset + half
                remaining = 0 if active else (offset - phase) % ring
                duration = frame - start
                fills.append({
                    "start_seconds": (start - first_play) / 44100,
                    "fill_seconds": duration / 44100,
                    "half": int(offset != 0),
                    "over_one_half": duration > half,
                    "deadline_miss": duration > remaining,
                    "active_at_lock": active,
                    "same_half_again": previous_half == offset,
                })
                previous_half = offset
                pending = None
    if unsupported_frequency:
        raise ValueError("buffer rate changed: this diagnostic requires 44100 Hz playback")
    return {
        "log": str(path),
        "buffer": hex(handle),
        "half_seconds": half / 44100,
        "observed_seconds_since_play": (last_frame - first_play) / 44100 if first_play is not None else 0,
        "all": stats(fills),
        "first_ten_seconds": stats([f for f in fills if f["start_seconds"] < 10]),
        "last_thirty_seconds": stats([
            f for f in fills if f["start_seconds"] >= (last_frame - first_play) / 44100 - 30
        ]) if first_play is not None else stats([]),
        "records": fills,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("log")
    parser.add_argument("--buffer", required=True, type=lambda n: int(n, 0))
    parser.add_argument("--buffer-bytes", required=True, type=int)
    parser.add_argument("--block-align", type=int, default=4)
    args = parser.parse_args()
    if args.block_align <= 0 or args.buffer_bytes <= 0 or args.buffer_bytes % (2 * args.block_align):
        parser.error("buffer size must contain two equal whole-frame halves")
    try:
        result = analyze(args.log, args.buffer, args.buffer_bytes, args.block_align)
    except ValueError as error:
        parser.error(str(error))
    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
