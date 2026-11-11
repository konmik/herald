import array
import importlib.util
import tempfile
import unittest
import wave
from pathlib import Path

spec = importlib.util.spec_from_file_location('verify_announcement_audio', Path(__file__).with_name('verify-announcement-audio.py'))
verifier = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verifier)
opening_gap = verifier.opening_gap


class OpeningGapTests(unittest.TestCase):
    def check(self, blocks, channels=1):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'recording.wav'
            samples = array.array('h')
            for seconds, amplitude in blocks:
                samples.extend([amplitude] * round(seconds * 16000) * channels)
            with wave.open(str(path), 'wb') as output:
                output.setnchannels(channels)
                output.setsampwidth(2)
                output.setframerate(16000)
                output.writeframes(samples.tobytes())
            return opening_gap(path)

    def test_original_gap_fails_the_limit(self):
        gap = self.check([(0.1, 0), (0.65, 2000), (0.7, 0), (0.4, 1500), (0.5, 0)])
        self.assertEqual(gap, 0.7)
        self.assertGreaterEqual(gap, 0.35)

    def test_continuous_speech(self):
        self.assertEqual(self.check([(0.1, 0), (0.65, 2000), (0.4, 1500), (0.5, 0)]), 0)

    def test_quiet_speech_is_not_a_gap(self):
        self.assertEqual(self.check([(0.1, 0), (0.65, 2000), (0.4, 15), (0.5, 0)]), 0)

    def test_stereo_gap(self):
        self.assertEqual(self.check([(0.1, 0), (0.65, 2000), (0.7, 0), (0.4, 1500), (0.5, 0)], channels=2), 0.7)

    def test_missing_speech(self):
        with self.assertRaisesRegex(AssertionError, 'No speech captured after static'):
            self.check([(0.1, 0), (0.65, 2000), (2, 0)])

    def test_short_recording(self):
        with self.assertRaisesRegex(AssertionError, 'Recording ends before the speech handoff can be checked'):
            self.check([(0.1, 0), (0.5, 2000)])

    def test_silent_recording(self):
        with self.assertRaisesRegex(AssertionError, 'No static or speech captured'):
            self.check([(2, 0)])


class ChimeraAudioTests(unittest.TestCase):
    def write(self, path, amplitudes, channels=1):
        samples = array.array('h')
        for amplitude in amplitudes:
            samples.extend(([amplitude] * channels + [-amplitude] * channels) * 240)
        with wave.open(str(path), 'wb') as output:
            output.setnchannels(channels)
            output.setsampwidth(2)
            output.setframerate(48000)
            output.writeframes(samples.tobytes())

    def test_finds_scaled_stereo_speech_after_unrelated_audio(self):
        with tempfile.TemporaryDirectory() as directory:
            reference = Path(directory) / 'speech.wav'
            recorded = Path(directory) / 'loopback.wav'
            envelope = [500 + (index * index * 113 % 8000) for index in range(200)]
            self.write(reference, envelope)
            self.write(recorded, [100] * 50 + [value // 10 for value in envelope] + [0] * 20, 2)
            proof = verifier.verify_chimera_audio(recorded, reference)
            self.assertGreater(proof['correlation'], 0.99)
            self.assertEqual(proof['speechAtSeconds'], 0.5)

    def test_rejects_silence_and_unrelated_constant_output(self):
        with tempfile.TemporaryDirectory() as directory:
            reference = Path(directory) / 'speech.wav'
            recorded = Path(directory) / 'loopback.wav'
            self.write(reference, [500 + (index * index * 113 % 8000) for index in range(200)])
            for amplitude in (0, 1000):
                self.write(recorded, [amplitude] * 300, 2)
                with self.assertRaisesRegex(AssertionError, 'speech was not found'):
                    verifier.verify_chimera_audio(recorded, reference)


if __name__ == '__main__':
    unittest.main()
