import array
import importlib.util
from pathlib import Path
import tempfile
import unittest
import wave


spec = importlib.util.spec_from_file_location("announcement_audio", Path(__file__).with_name("verify-announcement-audio.py"))
announcement_audio = importlib.util.module_from_spec(spec)
spec.loader.exec_module(announcement_audio)


class AnnouncementAudioTests(unittest.TestCase):
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
