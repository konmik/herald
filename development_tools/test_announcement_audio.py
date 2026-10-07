import array
from datetime import datetime, timezone
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
import wave


spec = importlib.util.spec_from_file_location("announcement_audio", Path(__file__).with_name("verify-announcement-audio.py"))
announcement_audio = importlib.util.module_from_spec(spec)
spec.loader.exec_module(announcement_audio)


class AnnouncementAudioTests(unittest.TestCase):
    def write_recording(self, path, samples):
        with wave.open(str(path), "wb") as recording:
            recording.setnchannels(1)
            recording.setsampwidth(2)
            recording.setframerate(16000)
            recording.writeframes(array.array("h", samples).tobytes())

    def recording_gap(self, silent_frames):
        samples = array.array("h", [1000] * 10400 + [0] * silent_frames + [1000] * 16000)
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "loopback.wav"
            with wave.open(str(path), "wb") as recording:
                recording.setnchannels(1)
                recording.setsampwidth(2)
                recording.setframerate(16000)
                recording.writeframes(samples.tobytes())
            return announcement_audio.opening_gap(path)

    def test_detects_two_second_silence_between_static_and_speech(self):
        self.assertAlmostEqual(self.recording_gap(32000), 2.0)

    def test_detects_slider_delays_between_static_and_speech(self):
        for seconds in [1, 5, 10]:
            with self.subTest(seconds=seconds):
                self.assertAlmostEqual(self.recording_gap(seconds * 16000), seconds)

    def test_detects_uninterrupted_static_to_speech(self):
        self.assertEqual(self.recording_gap(0), 0.0)

    def test_rejects_recordings_without_audio(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "silent.wav"
            with wave.open(str(path), "wb") as recording:
                recording.setnchannels(1)
                recording.setsampwidth(2)
                recording.setframerate(16000)
                recording.writeframes(bytes(64000))
            with self.assertRaisesRegex(AssertionError, "No static or speech captured"):
                announcement_audio.opening_gap(path)

    def announcement_order(self, before_seconds, after_seconds, shown_at):
        with tempfile.TemporaryDirectory() as directory:
            evidence = Path(directory)
            path = evidence / "loopback.wav"
            self.write_recording(path, [0] * (before_seconds * 16000) + [1000] * 10400 + [0] * (after_seconds * 16000) + [1000] * 16000)
            (evidence / "speech-api-requests.jsonl").write_text(json.dumps({"at": 1000000, "method": "POST"}) + "\n")
            (evidence / "history.jsonl").write_text(json.dumps({"shownAt": datetime.fromtimestamp(shown_at, timezone.utc).isoformat()}) + "\n")
            return announcement_audio.verify_announcement_order(path, evidence, 3, 1000)

    def test_accepts_delay_then_notification_and_static_then_speech(self):
        proof = self.announcement_order(3, 0, 1003)
        self.assertEqual(proof, {"delaySeconds": 3, "requestAt": 1000, "shownAt": 1003, "staticAt": 1003, "staticToSpeechGap": 0.0})

    def test_rejects_notification_before_delay(self):
        with self.assertRaisesRegex(AssertionError, "Notification appeared before"):
            self.announcement_order(3, 0, 1000)

    def test_rejects_static_before_delay(self):
        with self.assertRaisesRegex(AssertionError, "Static played before"):
            self.announcement_order(0, 3, 1003)

    def test_rejects_an_extra_delay_after_static(self):
        with self.assertRaisesRegex(AssertionError, "Static-to-speech silence"):
            self.announcement_order(3, 3, 1003)

    def preview_order(self, wrong_example=False):
        with tempfile.TemporaryDirectory() as directory:
            evidence = Path(directory)
            path = evidence / "loopback.wav"
            samples = []
            events = []
            for control in [210, 112, 210, 112]:
                requested_at = 1000 + len(samples) / 16000
                if control == 112 and wrong_example:
                    samples.extend([1000] * 10400 + [0] * 48000)
                else:
                    samples.extend([0] * 48000)
                    if control == 112:
                        samples.extend([1000] * 10400)
                samples.extend([1000] * 8000 + [0] * 2400)
                finished_at = 1000 + len(samples) / 16000
                events.append({"control": control, "requestAt": round(requested_at * 1000), "finishedAt": round(finished_at * 1000)})
            self.write_recording(path, samples)
            (evidence / "preview-events.jsonl").write_text("\n".join(json.dumps(event) for event in events) + "\n")
            return announcement_audio.verify_preview_order(path, evidence, 3, 1000)

    def test_accepts_both_settings_preview_orders(self):
        proof = self.preview_order()
        self.assertEqual(proof["delaySeconds"], 3)
        self.assertEqual([preview["control"] for preview in proof["previews"]], [210, 112, 210, 112])
        self.assertEqual([preview["staticToSpeechGap"] for preview in proof["previews"]], [None, 0.0, None, 0.0])

    def test_rejects_settings_example_static_before_delay(self):
        with self.assertRaisesRegex(AssertionError, "Preview control 112 played before"):
            self.preview_order(wrong_example=True)
