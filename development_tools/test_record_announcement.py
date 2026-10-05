import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


spec = importlib.util.spec_from_file_location("record_announcement", Path(__file__).with_name("record-announcement.py"))
recorder = importlib.util.module_from_spec(spec)
spec.loader.exec_module(recorder)


class RecordingTests(unittest.TestCase):
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
