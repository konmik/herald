import json
import subprocess
import tempfile
import unittest
from contextlib import ExitStack, nullcontext
from pathlib import Path
from unittest.mock import Mock, patch

from PIL import Image

from development_tools import generate_assets, generate_character
from development_tools.asset_library import clean_assets


class CharacterTests(unittest.TestCase):
    def test_names_cannot_escape_the_library(self):
        for name in ("../escape", "UPPER", "a/b", "a\\b", "con", "nul", "a" * 65, "", "-hat"):
            with self.subTest(name=name), self.assertRaises(ValueError):
                generate_character.validate_name(name)
        self.assertEqual(generate_character.validate_name("herald-21"), "herald-21")

    def test_portrait_preserves_native_pixels_and_requested_seed(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source.png"
            Image.new("RGB", (256, 256), "red").save(source)
            with patch.object(generate_assets, "ASSETS", root), patch.object(generate_assets, "RUNTIME_ASSETS", root), patch.object(generate_assets, "run_graph", return_value=[source]) as render:
                portrait, original = generate_assets.portrait("new-character", size=256, prompt_override="A new character.", seed=123, retain_native=True)
            generate_character.verify_portrait(portrait)
            self.assertEqual(original, source)
            self.assertEqual(render.call_args.args[0]["4"]["inputs"]["text"], "A new character.")
            self.assertEqual(render.call_args.args[0]["8"]["inputs"]["seed"], 123)
            self.assertFalse(portrait.with_name("new-character-source.png").exists())

    def test_video_validation_rejects_wrong_frame_rate_and_audio(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            good = root / "good.mp4"
            subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=256x256:rate=16:duration=4", "-c:v", "libx264", str(good)], check=True)
            generate_character.verify_video(good)
            wrong = root / "wrong.mp4"
            subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-i", str(good), "-r", "8", str(wrong)], check=True)
            with self.assertRaises(RuntimeError):
                generate_character.verify_video(wrong)
            metadata = {"streams": [{"codec_type": "video"}, {"codec_type": "audio"}]}
            with patch.object(generate_character.subprocess, "check_output", return_value=json.dumps(metadata)), self.assertRaises(RuntimeError):
                generate_character.verify_video(good)

    def test_audio_is_normalized_to_four_seconds_mono_16khz(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source.wav"
            subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=300:duration=6", str(source)], check=True)
            generate_character.prepare_audio(source, root / "speech.wav")

    def test_video_export_strips_prompt_metadata_without_changing_frames(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source.mp4"
            output = root / "library.mp4"
            subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=256x256:rate=16:duration=4", "-metadata", "comment=private generation graph", "-c:v", "libx264", str(source)], check=True)
            generate_character.prepare_video(source, output)
            generate_character.verify_video(output)
            data = json.loads(subprocess.check_output(["ffprobe", "-v", "error", "-show_format", "-of", "json", str(output)]))
            self.assertNotIn("comment", data["format"].get("tags", {}))

    def run_mock_generation(self, root, resume=False, video_error=False):
        comfy = root / "comfy"
        (comfy / "output").mkdir(parents=True)
        (comfy / "input").mkdir()
        audio = root / "speech.wav"
        audio.touch()
        sources = []

        def portrait(character, **kwargs):
            output = generate_assets.RUNTIME_ASSETS / "portraits" / f"{character}.png"
            output.parent.mkdir(parents=True)
            Image.new("RGB", (256, 256), "red").save(output)
            source = comfy / "output" / "portrait.png"
            source.write_bytes(output.read_bytes())
            sources.append(source)
            return output, source

        def video(emotion, character, **kwargs):
            if video_error:
                raise RuntimeError("Generation failed")
            output = generate_assets.ASSETS / "video.mp4"
            output.write_bytes(b"video")
            source = comfy / "output" / "video.mp4"
            source.write_bytes(b"video")
            sources.append(source)
            return output, source

        with ExitStack() as stack:
            stack.enter_context(patch.object(generate_character, "ROOT", root))
            stack.enter_context(patch.object(generate_character, "temp_root", return_value=root / "scratch"))
            stack.enter_context(patch.object(generate_character, "comfy_server", return_value=nullcontext()))
            stack.enter_context(patch.object(generate_character, "prepare_audio"))
            stack.enter_context(patch.object(generate_character, "prepare_video", side_effect=lambda source, output: output.write_bytes(source.read_bytes())))
            stack.enter_context(patch.object(generate_character, "verify_video"))
            stack.enter_context(patch.object(generate_character, "ensure_idle"))
            cancel = stack.enter_context(patch.object(generate_character, "cancel_owned_jobs"))
            stack.enter_context(patch.object(generate_assets, "release_idle_models"))
            image_call = stack.enter_context(patch.object(generate_assets, "portrait", side_effect=portrait))
            video_call = stack.enter_context(patch.object(generate_assets, "video", side_effect=video))
            previous = generate_assets.ASSETS, generate_assets.COMFY, generate_assets.RUNTIME_ASSETS
            if video_error:
                with self.assertRaisesRegex(RuntimeError, "Generation failed"):
                    generate_character.generate("herald", "A herald.", comfy, audio, resume)
            else:
                generate_character.generate("herald", "A herald.", comfy, audio, resume)
                self.assertEqual(video_call.call_args.kwargs["fps"], 16)
                self.assertEqual(video_call.call_args.kwargs["frames"], 65)
                self.assertEqual(video_call.call_args.kwargs["output_frames"], 64)
                self.assertFalse(video_call.call_args.kwargs["publish"])
            self.assertEqual((generate_assets.ASSETS, generate_assets.COMFY, generate_assets.RUNTIME_ASSETS), previous)
            cancel.assert_called_once()
            self.assertTrue(all(not source.exists() for source in sources))
            self.assertEqual(list((root / "scratch").iterdir()), [])
            return image_call.call_count

    def test_generation_keeps_only_library_portrait_and_video(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.assertEqual(self.run_mock_generation(root), 1)
            resources = root / "native-announcer/resources"
            self.assertTrue((resources / "portraits/herald.png").exists())
            self.assertEqual((resources / "videos/herald.mp4").read_bytes(), b"video")
            self.assertEqual(len(list(resources.rglob("*.*"))), 2)

    def test_failure_keeps_portrait_without_publishing_a_video(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.run_mock_generation(root, video_error=True)
            self.assertTrue((root / "native-announcer/resources/portraits/herald.png").exists())
            self.assertFalse((root / "native-announcer/resources/videos/herald.mp4").exists())

    def test_resume_reuses_saved_portrait(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            portrait = root / "native-announcer/resources/portraits/herald.png"
            portrait.parent.mkdir(parents=True)
            Image.new("RGB", (256, 256), "blue").save(portrait)
            before = portrait.read_bytes()
            self.assertEqual(self.run_mock_generation(root, resume=True), 0)
            self.assertEqual(portrait.read_bytes(), before)

    def test_existing_character_is_never_overwritten(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            video = root / "native-announcer/resources/videos/herald.mp4"
            video.parent.mkdir(parents=True)
            video.write_bytes(b"existing")
            with patch.object(generate_character, "ROOT", root), self.assertRaises(FileExistsError):
                generate_character.generate("herald", "A herald.", root / "comfy", resume=True)
            self.assertEqual(video.read_bytes(), b"existing")

    def test_cleanup_preserves_shared_library_files(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            runtime = root / "runtime"
            portrait = runtime / "portraits/herald.png"
            video = runtime / "videos/herald.mp4"
            portrait.parent.mkdir(parents=True)
            video.parent.mkdir()
            Image.new("RGB", (256, 256)).save(portrait)
            video.write_bytes(b"video")
            result = clean_assets(root / "generated", runtime)
            self.assertEqual(result, {"portraits": 1, "videos": 1, "removed": 0})
            self.assertTrue(portrait.exists())
            self.assertTrue(video.exists())

    def test_busy_queue_is_not_interrupted(self):
        with patch.object(generate_character, "get_json", return_value={"queue_running": [[1, "other"]]}), self.assertRaisesRegex(RuntimeError, "busy"):
            generate_character.ensure_idle()

    def test_cancellation_targets_only_owned_jobs(self):
        with tempfile.TemporaryDirectory() as directory:
            work = Path(directory)
            (work / "own-job.json").write_text(json.dumps({"prompt_id": "own", "fingerprint": "fingerprint"}))
            queues = [{"queue_running": [[1, "own"]], "queue_pending": [[2, "other"]]}, {"queue_running": [[2, "other"]]}]
            with patch.object(generate_character, "get_json", side_effect=queues), patch.object(generate_assets.requests, "post", return_value=Mock()) as post:
                generate_character.cancel_owned_jobs(work)
            post.assert_called_once_with(generate_assets.API + "/interrupt", json={"prompt_id": "own"}, timeout=10)


if __name__ == "__main__":
    unittest.main()
