import argparse
import json
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter


def verify(directory):
    directory = Path(directory)
    entries = [json.loads(line) for line in (directory / 'timeline.jsonl').read_text().splitlines()]
    holding = [entry for entry in entries if entry['elapsed'] >= 0.65 and entry['closingStart'] is None]
    assert holding, 'No holding frame captured'
    with Image.open(directory / holding[0]['file']) as frame:
        size = frame.size
        silhouette = Image.new('L', size)
        silhouette.putdata([0 if pixel == (32, 32, 32) else 255 for pixel in frame.convert('RGB').getdata()])
        ImageDraw.floodfill(silhouette, (0, 0), 128)
        silhouette = silhouette.point(lambda value: 0 if value == 128 else 255)
    scale = size[0] / 320
    edge = ImageChops.subtract(silhouette.filter(ImageFilter.MaxFilter(3)), silhouette.filter(ImageFilter.MinFilter(3)))
    band = edge.filter(ImageFilter.MaxFilter(2 * round(6 * scale) + 1))
    selected = [index for index, value in enumerate(band.getdata()) if value]
    counts = {'entrance': [], 'holding': [], 'exit': []}
    for entry in entries:
        phase = 'exit' if entry['closingStart'] is not None else 'entrance' if entry['elapsed'] < 0.65 else 'holding'
        with Image.open(directory / entry['file']) as frame:
            assert frame.size == size, 'Frame dimensions changed during playback'
            pixels = list(frame.convert('RGB').getdata())
        colors = [pixels[index] for index in selected]
        cyan = sum(red < 90 and green > 110 and blue > 190 for red, green, blue in colors)
        white = sum(min(color) > 200 for color in colors)
        counts[phase].append({'file': entry['file'], 'cyan': cyan, 'white': white})
    assert counts['entrance'] and counts['exit'], 'Entrance or exit frames missing'
    late_cyan = max(frame['cyan'] for phase in ('holding', 'exit') for frame in counts[phase])
    late_white = max(frame['white'] for phase in ('holding', 'exit') for frame in counts[phase])
    peak = max(counts['entrance'], key=lambda frame: frame['cyan'])
    assert late_cyan <= 50 * scale * scale, f'Cyan outline persists after entrance: {late_cyan} pixels'
    assert peak['cyan'] >= late_cyan + 30 * scale * scale, f'No strong cyan entrance strike: {peak}'
    assert max(frame['white'] for frame in counts['entrance']) >= late_white + 4 * scale * scale, 'No white entrance cores near the outline'
    return {'result': 'PASS', 'peakEntrance': peak, 'maxHoldingExitCyan': late_cyan, 'frames': {phase: len(frames) for phase, frames in counts.items()}}


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('frames', type=Path)
    arguments = parser.parse_args()
    print(json.dumps(verify(arguments.frames), indent=2))
