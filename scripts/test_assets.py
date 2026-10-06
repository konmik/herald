import json
import subprocess
import tempfile
import unittest
import wave
from unittest.mock import Mock, patch
from pathlib import Path

from PIL import Image

from scripts import generate_assets


class AssetTests(unittest.TestCase):
    def test_video_export_uses_flat_dynamic_combo_inputs(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "recordings").mkdir()
            (root / "input").mkdir()
            Image.new("RGB", (128, 128)).save(root / "portrait-source.png")
            with wave.open(str(root / "recordings" / "neutral.wav"), "wb") as audio:
                audio.setnchannels(1)
                audio.setsampwidth(2)
                audio.setframerate(16000)
                audio.writeframes(bytes(32000))
            output = root / "render.mp4"
            output.touch()
            with patch.object(generate_assets, "ASSETS", root), patch.object(generate_assets, "COMFY", root), patch.object(generate_assets, "run_graph", return_value=[output]) as render:
                generate_assets.video("neutral")
            inputs = render.call_args.args[0]["15"]["inputs"]
            self.assertEqual(inputs["format"], "mp4")
            self.assertEqual(inputs["format.codec"], "h264")

    def test_preview_video_generates_only_window_sized_frames(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "recordings").mkdir()
            (root / "input").mkdir()
            (root / "monty").mkdir()
            Image.new("RGB", (128, 128)).save(root / "monty" / "portrait-source.png")
            with wave.open(str(root / "recordings" / "neutral.wav"), "wb") as audio:
                audio.setnchannels(1)
                audio.setsampwidth(2)
                audio.setframerate(16000)
                audio.writeframes(bytes(32000))
            output = root / "render.mp4"
            output.touch()
            with patch.object(generate_assets, "ASSETS", root), patch.object(generate_assets, "COMFY", root), patch.object(generate_assets, "run_graph", return_value=[output]) as render:
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
            (root / "neutral-job.json").write_text(json.dumps({"prompt_id": "existing", "graph": graph}))
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

    def test_slices_video_into_expression_frames_without_mouth_overlays(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=384x384:rate=16:duration=1", "-c:v", "libx264", str(root / "neutral.mp4")], check=True)
            previous = generate_assets.ASSETS
            generate_assets.ASSETS = root
            try:
                generate_assets.slice_video("neutral")
            finally:
                generate_assets.ASSETS = previous
            self.assertEqual(len(list((root / "neutral").glob("frame-*.png"))), 8)
            self.assertEqual(len(list((root / "neutral").glob("mouth-*.png"))), 0)
            self.assertEqual(json.loads((root / "neutral" / "manifest.json").read_text()), {"emotion": "neutral", "fps": 8, "frames": 8})
            for path in (root / "neutral").glob("frame-*.png"):
                with Image.open(path) as image:
                    self.assertEqual(image.size, (128, 128))


if __name__ == "__main__":
    unittest.main()
