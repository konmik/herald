import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

from PIL import Image, ImageDraw

spec = importlib.util.spec_from_file_location('verify_entrance_lightning', Path(__file__).with_name('verify-entrance-lightning.py'))
verifier = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verifier)


class EntranceLightningTests(unittest.TestCase):
    def capture(self, directory, entrance=True, exit_strike=False, deep=False, hole=False):
        entries = []
        for index, (elapsed, closing) in enumerate([(0.12, None), (1.0, None), (5.1, 5.0)]):
            image = Image.new('RGB', (320, 260), (32, 32, 32))
            draw = ImageDraw.Draw(image)
            draw.rectangle((6, 6, 314, 248), fill=(20, 25, 34))
            if hole:
                draw.rectangle((80, 80, 220, 160), fill=(32, 32, 32))
            if (index == 0 and entrance) or (index == 2 and exit_strike):
                y = 80 if hole else 60 if deep else 8
                draw.line((40, y, 140, y), fill=(0, 223, 255), width=3)
                draw.line((40, y, 140, y), fill=(255, 255, 255), width=1)
            filename = f'frame-{index:06}.png'
            image.save(directory / filename)
            entries.append({'file': filename, 'elapsed': elapsed, 'closingStart': closing})
        (directory / 'timeline.jsonl').write_text('\n'.join(json.dumps(entry) for entry in entries))

    def test_observes_bright_entrance_and_clean_holding_and_exit(self):
        with tempfile.TemporaryDirectory() as scratch:
            directory = Path(scratch)
            self.capture(directory)
            result = verifier.verify(directory)
            self.assertEqual(result['peakEntrance'], {'file': 'frame-000000.png', 'cyan': 202, 'white': 101})
            self.assertEqual(result['maxHoldingExitCyan'], 0)
            self.assertEqual(result['frames'], {'entrance': 1, 'holding': 1, 'exit': 1})

    def test_rejects_missing_strikes_strikes_in_content_and_exit_strikes(self):
        for options, message in [({'entrance': False}, 'No strong cyan'), ({'deep': True}, 'No strong cyan'), ({'hole': True}, 'No strong cyan'), ({'exit_strike': True}, 'persists after entrance')]:
            with self.subTest(options=options), tempfile.TemporaryDirectory() as scratch:
                directory = Path(scratch)
                self.capture(directory, **options)
                with self.assertRaisesRegex(AssertionError, message):
                    verifier.verify(directory)


if __name__ == '__main__':
    unittest.main()
