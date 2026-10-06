import json
import hashlib
import subprocess
import tempfile
import unittest
import wave
from unittest.mock import Mock, patch
from pathlib import Path

from PIL import Image

from development_tools import generate_assets
from development_tools.asset_library import clean_assets


class AssetTests(unittest.TestCase):
    def test_cleanup_keeps_portraits_and_videos_but_removes_prompts(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            assets = root / "generated"
            runtime = root / "runtime"
            (assets / "character-portraits").mkdir(parents=True)
            (runtime / "opencode").mkdir(parents=True)
            portrait = assets / "character-portraits" / "herald.png"
            Image.new("RGB", (128, 128)).save(portrait)
            video = runtime / "opencode" / "neutral.mp4"
            video.write_bytes(b"used video")
            graph = {
                "1": {"class_type": "CLIPTextEncode", "inputs": {"text": "Original prompt."}},
                "2": {"class_type": "KSampler", "inputs": {"positive": ["1", 0], "negative": ["1", 0]}},
            }
            for label in ("herald-portrait", "opencode-neutral"):
                (assets / f"{label}-job.json").write_text(json.dumps({"graph": graph}))
            portrait.with_suffix(".md").write_text("Old prompt.")
            video.with_suffix(".md").write_text("Old prompt.")
            (assets / "unused.mp4").write_bytes(b"unused")
            (assets / "recording.wav").write_bytes(b"audio")
            result = clean_assets(assets, runtime)
            self.assertEqual(result, {"portraits": 1, "videos": 1, "removed": 6})
            self.assertEqual(video.read_bytes(), b"used video")
            self.assertFalse(portrait.with_suffix(".md").exists())
            self.assertFalse(video.with_suffix(".md").exists())
            self.assertFalse((assets / "unused.mp4").exists())
            self.assertFalse((assets / "recording.wav").exists())
            self.assertEqual(clean_assets(assets, runtime)["removed"], 0)

    def test_cleanup_does_not_require_saved_prompts(self):
        with tempfile.TemporaryDirectory() as directory:
            assets = Path(directory) / "generated"
            assets.mkdir()
            Image.new("RGB", (128, 128)).save(assets / "portrait.png")
            leftover = assets / "unused.mp4"
            leftover.write_bytes(b"unused")
            clean_assets(assets, Path(directory) / "runtime")
            self.assertFalse(leftover.exists())
            self.assertTrue((Path(directory) / "runtime" / "portraits" / "original.png").exists())

    def test_cleanup_consolidates_portraits_without_overwriting_sources(self):
        with tempfile.TemporaryDirectory() as directory:
            assets = Path(directory) / "generated"
            (assets / "claude").mkdir(parents=True)
            for name, size in (("portrait", 128), ("portrait-source", 512)):
                image = assets / "claude" / f"{name}.png"
                Image.new("RGB", (size, size)).save(image)
                image.with_suffix(".md").write_text(f"Prompt for {name}.")
            clean_assets(assets, Path(directory) / "runtime")
            library = Path(directory) / "runtime" / "portraits"
            with Image.open(library / "claude.png") as image:
                self.assertEqual(image.size, (128, 128))
            with Image.open(library / "claude-source.png") as image:
                self.assertEqual(image.size, (512, 512))
            self.assertEqual(list(assets.rglob("*.md")), [])
            self.assertFalse((assets / "claude").exists())
            self.assertEqual(clean_assets(assets, Path(directory) / "runtime")["removed"], 0)

    def test_video_export_uses_flat_dynamic_combo_inputs(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "recordings").mkdir()
            (root / "input").mkdir()
            (root / "runtime" / "portraits").mkdir(parents=True)
            Image.new("RGB", (128, 128)).save(root / "runtime" / "portraits" / "original-source.png")
            with wave.open(str(root / "recordings" / "neutral.wav"), "wb") as audio:
                audio.setnchannels(1)
                audio.setsampwidth(2)
                audio.setframerate(16000)
                audio.writeframes(bytes(32000))
            output = root / "render.mp4"
            output.touch()
            with patch.object(generate_assets, "ASSETS", root), patch.object(generate_assets, "COMFY", root), patch.object(generate_assets, "RUNTIME_ASSETS", root / "runtime"), patch.object(generate_assets, "run_graph", return_value=[output]) as render:
                generate_assets.video("neutral")
            inputs = render.call_args.args[0]["15"]["inputs"]
            self.assertEqual(inputs["format"], "mp4")
            self.assertEqual(inputs["format.codec"], "h264")
            self.assertEqual(list(root.rglob("*.md")), [])

    def test_preview_video_generates_only_window_sized_frames(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "recordings").mkdir()
            (root / "input").mkdir()
            (root / "monty").mkdir()
            (root / "runtime" / "portraits").mkdir(parents=True)
            Image.new("RGB", (128, 128)).save(root / "runtime" / "portraits" / "monty-source.png")
            with wave.open(str(root / "recordings" / "neutral.wav"), "wb") as audio:
                audio.setnchannels(1)
                audio.setsampwidth(2)
                audio.setframerate(16000)
                audio.writeframes(bytes(32000))
            output = root / "render.mp4"
            output.touch()
            with patch.object(generate_assets, "ASSETS", root), patch.object(generate_assets, "COMFY", root), patch.object(generate_assets, "RUNTIME_ASSETS", root / "runtime"), patch.object(generate_assets, "run_graph", return_value=[output]) as render:
                generate_assets.video("neutral", "monty", size=128, frames=17, steps=8, fps=8)
            graph = render.call_args.args[0]
            self.assertEqual(graph["10"]["inputs"]["width"], 128)
            self.assertEqual(graph["10"]["inputs"]["height"], 128)
            self.assertEqual(graph["10"]["inputs"]["length"], 17)
            self.assertEqual(graph["12"]["inputs"]["steps"], 8)
            self.assertEqual(graph["14"]["inputs"]["fps"], 8)
            self.assertNotIn("audio", graph["14"]["inputs"])

    def test_reconnects_to_an_active_job_without_queueing_a_duplicate(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            graph = {"1": {"class_type": "SaveVideo", "inputs": {}}}
            fingerprint = hashlib.sha256(json.dumps(graph, sort_keys=True).encode()).hexdigest()
            (root / "neutral-job.json").write_text(json.dumps({"prompt_id": "existing", "fingerprint": fingerprint}))
            (root / "output").mkdir()
            output = root / "output" / "output.mp4"
            output.touch()
            responses = [
                {},
                {"queue_running": [[0, "existing"]], "queue_pending": []},
                {"existing": {"status": {"status_str": "success", "completed": True}, "outputs": {"1": {"videos": [{"filename": "output.mp4"}]}}}},
            ]
            with patch.object(generate_assets, "ASSETS", root), patch.object(generate_assets, "COMFY", root), patch.object(generate_assets.requests, "get", side_effect=[Mock(json=Mock(return_value=value)) for value in responses]), patch.object(generate_assets.requests, "post") as submit:
                paths = generate_assets.run_graph(graph, "neutral")
                submit.assert_not_called()
                self.assertEqual(paths, [root / "output" / "output.mp4"])
                self.assertFalse((root / "neutral-job.json").exists())

    def test_proclamation_uses_collection_portrait_and_exports_exactly_four_seconds(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "recordings").mkdir()
            (root / "input").mkdir()
            (root / "runtime" / "portraits").mkdir(parents=True)
            Image.new("RGB", (128, 128)).save(root / "runtime" / "portraits" / "royal-herald-04.png")
            audio_path = root / "recordings" / "proclamation.wav"
            with wave.open(str(audio_path), "wb") as audio:
                audio.setnchannels(1)
                audio.setsampwidth(2)
                audio.setframerate(16000)
                audio.writeframes(bytes(64000))
            output = root / "render.mp4"
            output.touch()
            with patch.object(generate_assets, "ASSETS", root), patch.object(generate_assets, "COMFY", root), patch.object(generate_assets, "RUNTIME_ASSETS", root / "runtime"), patch.object(generate_assets, "run_graph", return_value=[output]) as render:
                generate_assets.video("neutral", "royal-herald-04", size=128, frames=33, output_frames=32, fps=8, audio_path=audio_path, prompt="Proudly shouting and looking upward.", negative_prompt="Changing exposure.")
            graph = render.call_args.args[0]
            self.assertEqual(graph["10"]["inputs"]["length"], 33)
            self.assertEqual(graph["18"]["inputs"]["length"] / graph["14"]["inputs"]["fps"], 4)
            self.assertEqual(graph["8"]["inputs"]["text"], "Proudly shouting and looking upward.")
            self.assertEqual(graph["9"]["inputs"]["text"], "Changing exposure.")
            self.assertTrue((root / "runtime" / "royal-herald-04" / "neutral.mp4").exists())

    def test_publishes_small_video_without_exporting_png_frames(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "monty").mkdir()
            subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=384x384:rate=16:duration=1", "-c:v", "libx264", str(root / "monty" / "neutral.mp4")], check=True)
            with patch.object(generate_assets, "ASSETS", root), patch.object(generate_assets, "RUNTIME_ASSETS", root / "runtime"):
                generate_assets.publish_video("neutral", "monty")
            output = root / "runtime" / "monty" / "neutral.mp4"
            metadata = json.loads(subprocess.check_output(["ffprobe", "-v", "error", "-select_streams", "v:0", "-count_frames", "-show_entries", "stream=width,height,r_frame_rate,nb_read_frames", "-of", "json", str(output)]))["streams"][0]
            self.assertEqual((metadata["width"], metadata["height"]), (128, 128))
            self.assertEqual(metadata["r_frame_rate"], "8/1")
            self.assertEqual(metadata["nb_read_frames"], "8")
            self.assertEqual(list((root / "runtime").rglob("*.png")), [])


if __name__ == "__main__":
    unittest.main()
