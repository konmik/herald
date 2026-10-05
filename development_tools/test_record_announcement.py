import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


spec = importlib.util.spec_from_file_location("record_announcement", Path(__file__).with_name("record-announcement.py"))
recorder = importlib.util.module_from_spec(spec)
spec.loader.exec_module(recorder)


class RecordingTests(unittest.TestCase):
    def test_interference_uses_the_actual_transition_time_on_the_render_clock(self):
        frames = [{"elapsed": 0.02, "closingStart": None}, {"elapsed": 10.05, "closingStart": 10.01}]
        opening, delay = recorder.interference_timing(frames)
        self.assertEqual(opening, 0.5)
        self.assertAlmostEqual(delay, 10.51)

    def test_missing_closing_interference_is_rejected(self):
        with self.assertRaisesRegex(RuntimeError, "closing interference"):
            recorder.interference_timing([{"elapsed": 0.02}])

    def test_video_includes_blank_padding_instead_of_a_frozen_final_frame(self):
        frames = [{"file": "frame-000000.png", "elapsed": 0.01}, {"file": "frame-000001.png", "elapsed": 10.6}]
        timeline, delay, duration = recorder.video_timeline(frames, 8, 10.65)
        self.assertAlmostEqual(delay, 1.15)
        self.assertAlmostEqual(duration, 11.65)
        self.assertIn("duration 0.510000000", timeline)
        self.assertIn("file 'frame-000001.png'\noption framerate 1000\nduration 0.050000000\nfile 'background.png'", timeline)
        self.assertTrue(timeline.endswith("duration 0.500000000\nfile 'background.png'\noption framerate 1000\n"))

    def test_long_visuals_are_not_truncated_for_short_speech(self):
        frames = [{"file": "frame-000000.png", "elapsed": 0.01}, {"file": "frame-000001.png", "elapsed": 11.01}]
        _, _, duration = recorder.video_timeline(frames, 3, 11.05)
        self.assertEqual(duration, 12.05)

    def test_speech_cannot_be_cut_off_or_extend_the_final_frame(self):
        frames = [{"file": "frame-000000.png", "elapsed": 0.01}]
        with self.assertRaisesRegex(RuntimeError, "narration extends"):
            recorder.video_timeline(frames, 12, 10)

    def test_selects_the_last_message_from_the_requested_session(self):
        with tempfile.TemporaryDirectory() as directory:
            history = Path(directory) / "history.jsonl"
            messages = [
                {"sessionID": "current", "text": "Old.", "character": "first"},
                {"sessionID": "current", "text": "Latest.", "character": "herald", "video": "herald.mp4"},
                {"sessionID": "other", "text": "Another session."},
            ]
            history.write_text("\n".join(json.dumps(message) for message in messages) + "\n{unfinished", encoding="utf-8")
            self.assertEqual(recorder.last_message(history, "current"), messages[1])
            self.assertEqual(recorder.last_message(history), messages[2])

    def test_requires_a_saved_message_instead_of_inventing_one(self):
        with tempfile.TemporaryDirectory() as directory:
            history = Path(directory) / "history.jsonl"
            history.write_text('{"sessionID":"other","text":"Done."}\n', encoding="utf-8")
            with self.assertRaisesRegex(RuntimeError, "No displayed announcement"):
                recorder.last_message(history, "current")


if __name__ == "__main__":
    unittest.main()
