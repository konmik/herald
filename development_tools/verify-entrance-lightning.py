import argparse
import json
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter


def light_mask(image):
    mask = Image.new('L', image.size)
    mask.putdata([255 if blue >= 180 and green >= 130 and red >= 100 and blue >= red and blue >= green else 0
                  for red, green, blue in image.convert('RGB').getdata()])
    return mask


def count(mask):
    return sum(value > 0 for value in mask.getdata())


def card_image(directory, entry):
    geometry = entry['geometry']
    viewport, card = geometry['viewport'], geometry['card']
    with Image.open(directory / entry['file']) as frame:
        assert frame.size == (viewport['width'], viewport['height']), 'Viewport metadata does not match the captured scene'
        x, y = card['x'] - viewport['x'], card['y'] - viewport['y']
        assert x >= 0 and y >= 0 and x + card['width'] <= frame.width and y + card['height'] <= frame.height, 'Card is cropped out of the scene'
        return frame.convert('RGB').crop((x, y, x + card['width'], y + card['height']))


def verify(directory):
    directory = Path(directory)
    entries = [json.loads(line) for line in (directory / 'timeline.jsonl').read_text().splitlines()]
    assert entries and all('geometry' in entry for entry in entries), 'Explicit scene geometry is required'
    holding = [entry for entry in entries if entry['elapsed'] >= 0.65 and entry['closingStart'] is None]
    assert holding, 'No holding frame captured'
    reference = holding[0]['geometry']
    scale = reference['scale']
    card, monitor = reference['card'], reference['monitor']
    assert monitor is not None, 'No provable monitor-edge placement on this platform'
    baseline = card_image(directory, holding[0])
    silhouette = Image.new('L', baseline.size)
    silhouette.putdata([0 if pixel == (32, 32, 32) else 255 for pixel in baseline.getdata()])
    ImageDraw.floodfill(silhouette, (0, 0), 128)
    silhouette = silhouette.point(lambda value: 0 if value == 128 else 255)
    edge = ImageChops.subtract(silhouette.filter(ImageFilter.MaxFilter(3)), silhouette.filter(ImageFilter.MinFilter(3)))
    band = edge.filter(ImageFilter.MaxFilter(2 * round(5 * scale) + 1))
    far = Image.new('L', baseline.size)
    ImageDraw.Draw(far).rectangle((30 * scale, 0, 285 * scale, 15 * scale), fill=255)
    impact = reference['impact']
    near = Image.new('L', baseline.size)
    ix, iy = impact[0] - card['x'], impact[1] - card['y']
    ImageDraw.Draw(near).ellipse((ix - 22 * scale, iy - 22 * scale, ix + 22 * scale, iy + 22 * scale), fill=255)
    baseline_light = light_mask(baseline)
    records = []
    source_seen = False
    for entry in entries:
        geometry = entry['geometry']
        assert geometry['card'] == card and geometry['scale'] == scale, 'Global card placement or scale changed'
        assert geometry['monitor'] == monitor, 'Chosen monitor changed during playback'
        assert geometry['source'] == reference['source'] and geometry['impact'] == impact, 'Planted strike geometry changed'
        elapsed = entry['elapsed']
        phase = 'exit' if entry['closingStart'] is not None else 'leader' if elapsed < 0.12 else 'impact' if elapsed < 0.18 else 'propagation' if elapsed < 0.45 else 'decay' if elapsed < 0.65 else 'holding'
        assert geometry['phase'] == phase, 'Frame timing and phase disagree'
        viewport = geometry['viewport']
        if phase in ('holding', 'exit'):
            assert viewport == card, 'Normal window bounds were not restored'
        else:
            source = geometry['source']
            assert source[1] == monitor['y'], 'Lightning does not start on the actual top screen edge'
            assert monitor['x'] <= source[0] < monitor['x'] + monitor['width'], 'Source is outside the selected monitor'
            assert viewport['y'] == monitor['y'] and viewport['x'] < card['x'], 'Entrance scene is cropped to the card'
            assert viewport['x'] >= monitor['x'] and viewport['x'] + viewport['width'] <= monitor['x'] + monitor['width'], 'Scene exceeds the monitor'
            if phase == 'leader' and elapsed > 0:
                with Image.open(directory / entry['file']) as frame:
                    sx = round(source[0] - viewport['x'])
                    patch = frame.crop((max(0, sx - round(5 * scale)), 0, sx + round(5 * scale) + 1, max(1, round(3 * scale))))
                    source_seen |= count(light_mask(patch)) >= 3
        mask = light_mask(card_image(directory, entry))
        fresh = ImageChops.subtract(mask, baseline_light)
        coverage = count(ImageChops.multiply(edge, fresh.filter(ImageFilter.MaxFilter(2 * round(3 * scale) + 1)))) / max(1, count(edge))
        records.append({'phase': phase, 'elapsed': elapsed, 'outline': count(ImageChops.multiply(band, fresh)),
                        'near': count(ImageChops.multiply(near, fresh)), 'far': count(ImageChops.multiply(far, fresh)), 'coverage': coverage})
    leaders = [record for record in records if record['phase'] == 'leader' and record['elapsed'] > 0]
    impacts = [record for record in records if record['phase'] == 'impact']
    early = [record for record in records if record['phase'] == 'propagation' and record['elapsed'] < 0.28]
    late = [record for record in records if 0.38 <= record['elapsed'] < 0.56 and record['phase'] != 'exit']
    after = [record for record in records if record['phase'] in ('holding', 'exit')]
    assert leaders and source_seen, 'No visible screen-edge leader captured'
    assert impacts and early and late, 'Impact or charge propagation frames missing'
    assert any(record['phase'] == 'exit' for record in records), 'Exit frames missing'
    allowance = 20 * scale * scale
    assert max(record['outline'] for record in leaders) <= allowance, 'Outline energized before video impact'
    assert max(record['near'] for record in impacts) >= 10 * scale * scale, 'No bright video-first impact'
    assert max(record['far'] for record in impacts + early) <= allowance, 'Bubble energized before charge traveled from the video'
    assert max(record['near'] for record in early) >= 20 * scale * scale, 'Charge did not propagate from the video hit'
    assert max(record['far'] for record in late) >= 80 * scale * scale, 'Charge never reached the far bubble edge'
    full = max(record['coverage'] for record in late)
    assert full >= 0.85, f'Entire painted outline did not energize: {full:.1%}'
    assert max(record['outline'] for record in after) <= 50 * scale * scale, 'Lightning persists during holding or exit'
    return {'result': 'PASS', 'screenEdgeLeader': True, 'videoFirst': True, 'fullOutlineCoverage': round(full, 3),
            'fixedCardPlacement': True, 'restoredBounds': True, 'frames': len(entries)}


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('frames', type=Path)
    arguments = parser.parse_args()
    print(json.dumps(verify(arguments.frames), indent=2))
