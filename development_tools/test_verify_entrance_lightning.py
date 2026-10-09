import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter

spec = importlib.util.spec_from_file_location('verify_entrance_lightning', Path(__file__).with_name('verify-entrance-lightning.py'))
verifier = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verifier)


class EntranceLightningTests(unittest.TestCase):
    def capture(self, directory, defect=None):
        entries = []
        card = {'x': 600, 'y': 400, 'width': 320, 'height': 260}
        monitor = {'x': 0, 'y': 0, 'width': 1000, 'height': 800}
        phases = [(0.06, 'leader'), (0.14, 'impact'), (0.20, 'propagation'), (0.41, 'propagation'),
                  (0.48, 'decay'), (0.7, 'holding'), (1.0, 'holding'), (1.2, 'holding'), (1.5, 'holding'), (5.1, 'exit'), (5.6, 'exit'), (5.64, 'exit')]
        for index, (elapsed, phase) in enumerate(phases):
            normal = phase in ('holding', 'exit')
            viewport = card.copy() if normal else {'x': 440, 'y': 400, 'width': 480, 'height': 400}
            image = Image.new('RGB', (viewport['width'], viewport['height']), (32, 32, 32))
            local = Image.new('RGB', (320, 260), (32, 32, 32))
            draw = ImageDraw.Draw(local)
            draw.rounded_rectangle((6, 6, 315, 110), radius=20, fill=(20, 25, 34))
            draw.polygon((236, 104, 268, 104, 252, 126), fill=(20, 25, 34))
            draw.rectangle((188, 120, 315, 247), fill=(20, 25, 34))
            silhouette = local.convert('L').point(lambda value: 255 if value != 32 else 0)
            edge = ImageChops.subtract(silhouette.filter(ImageFilter.MaxFilter(3)), silhouette.filter(ImageFilter.MinFilter(3)))
            if phase == 'impact' and defect != 'missing_impact':
                draw.line((188, 162, 188, 179), fill=(247, 251, 255), width=4)
            if phase == 'impact' and defect == 'bubble_first':
                draw.line((40, 6, 280, 6), fill=(247, 251, 255), width=4)
            if phase == 'propagation' and elapsed < 0.28:
                draw.line((188, 140, 188, 200), fill=(247, 251, 255), width=4)
            if (0.38 <= elapsed < 0.56 and defect != 'incomplete') or phase == 'exit':
                local.paste((140, 170, 245), mask=edge.filter(ImageFilter.MaxFilter(7)))
                local.paste((247, 251, 255), mask=edge)
            if 0.38 <= elapsed < 0.56 and defect == 'incomplete':
                draw.line((40, 6, 280, 6), fill=(247, 251, 255), width=4)
            if phase == 'holding' and (elapsed == 1.0 or defect == 'constant_holding') and defect != 'missing_holding':
                ImageDraw.Draw(local).line((40, 6, 100, 6), fill=(247, 251, 255), width=4)
                ImageDraw.Draw(local).line((40, 40, 150, 70), fill=(247, 251, 255), width=3)
            if phase == 'exit':
                if defect != 'missing_interior':
                    ImageDraw.Draw(local).line((40, 40, 280, 80), fill=(247, 251, 255), width=6)
                if elapsed > 5.5 and defect != 'not_swallowed':
                    local.paste((32, 32, 32), (0, 20, 320, 260))
                if elapsed > 5.62 and defect == 'floating_lightning':
                    ImageDraw.Draw(local).line((200, 180, 280, 210), fill=(247, 251, 255), width=4)
            x, y = card['x'] - viewport['x'], card['y'] - viewport['y']
            image.paste(local, (x, y))
            if phase in ('leader', 'impact') and defect != 'missing_leader':
                endpoint = (95, 290) if phase == 'leader' else (347, 170)
                if defect == 'missing_impact' and phase == 'impact':
                    endpoint = (320, 270)
                ImageDraw.Draw(image).line((20, 399, 95, 290, *endpoint), fill=(247, 251, 255), width=3)
            filename = f'frame-{index:06}.png'
            image.save(directory / filename)
            metadata_card = card.copy()
            if defect == 'moved_card' and index == 0:
                metadata_card['x'] += 1
            source = [460, 20 if defect == 'work_area_source' else 799]
            entries.append({'file': filename, 'elapsed': elapsed, 'closingStart': 5.0 if phase == 'exit' else None,
                            'geometry': {'monitor': monitor, 'viewport': viewport, 'card': metadata_card, 'scale': 1.0,
                                         'source': source, 'impact': [787.5, 569.5], 'phase': phase}})
        (directory / 'timeline.jsonl').write_text('\n'.join(json.dumps(entry) for entry in entries))

    def test_proves_full_screen_edge_sequence_and_exact_restoration(self):
        with tempfile.TemporaryDirectory() as scratch:
            directory = Path(scratch)
            self.capture(directory)
            self.assertEqual(verifier.verify(directory), {'result': 'PASS', 'screenEdgeLeader': True, 'videoFirst': True,
                             'fullOutlineCoverage': 1.0, 'fixedCardPlacement': True, 'restoredBounds': True, 'sporadicArcs': True, 'swallowingExit': True, 'frames': 12})

    def test_rejects_uncausal_cropped_incomplete_and_persistent_effects(self):
        for defect, message in [('missing_leader', 'No visible screen-edge leader'), ('missing_impact', 'No bright video-first impact'),
                                ('bubble_first', 'Bubble energized before'), ('incomplete', 'Entire painted outline'),
                                ('missing_holding', 'sporadic'), ('constant_holding', 'sporadic'),
                                ('missing_interior', 'crosses the interiors'), ('not_swallowed', 'swallow'),
                                ('floating_lightning', 'reappeared over erased'),
                                ('work_area_source', 'bottom or right screen edge'),
                                ('moved_card', 'Global card placement')]:
            with self.subTest(defect=defect), tempfile.TemporaryDirectory() as scratch:
                directory = Path(scratch)
                self.capture(directory, defect)
                with self.assertRaisesRegex(AssertionError, message):
                    verifier.verify(directory)

    def test_requires_explicit_geometry_instead_of_guessing_scale_from_width(self):
        with tempfile.TemporaryDirectory() as scratch:
            directory = Path(scratch)
            (directory / 'timeline.jsonl').write_text(json.dumps({'file': 'old.png', 'elapsed': 0.14, 'closingStart': None}))
            with self.assertRaisesRegex(AssertionError, 'Explicit scene geometry'):
                verifier.verify(directory)


if __name__ == '__main__':
    unittest.main()
