import argparse
import json
import math
import shutil
import subprocess
import time
import wave
import uuid
from pathlib import Path

import requests
import websocket
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
COMFY = Path(r"C:\ComfyUI")
API = "http://127.0.0.1:8188"
ASSETS = ROOT / "assets"
RUNTIME_ASSETS = ROOT / "claude" / "assets"
MONITOR = False


def run_graph(graph, label):
    client_id = str(uuid.uuid4())
    connection = websocket.create_connection(f"ws://127.0.0.1:8188/ws?clientId={client_id}", timeout=5) if MONITOR else None
    try:
        return execute_graph(graph, label, client_id, connection)
    finally:
        if connection:
            connection.close()


def execute_graph(graph, label, client_id, connection):
    ASSETS.mkdir(parents=True, exist_ok=True)
    job_path = ASSETS / f"{label}-job.json"
    prompt_id = None
    if job_path.exists():
        previous = json.loads(job_path.read_text())
        if previous.get("graph") == graph:
            candidate = previous["prompt_id"]
            history = requests.get(f"{API}/history/{candidate}", timeout=30).json().get(candidate)
            queue = requests.get(f"{API}/queue", timeout=30).json()
            active = any(item[1] == candidate for group in ("queue_running", "queue_pending") for item in queue.get(group, []))
            if active or (history and history["status"]["status_str"] != "error"):
                prompt_id = candidate
                print(f"Resuming {label}: {prompt_id}", flush=True)
    if prompt_id is None:
        response = requests.post(f"{API}/prompt", json={"prompt": graph, "client_id": client_id}, timeout=30)
        response.raise_for_status()
        admitted = response.json()
        if admitted.get("node_errors"):
            raise RuntimeError(admitted["node_errors"])
        prompt_id = admitted["prompt_id"]
        job_path.write_text(json.dumps({"prompt_id": prompt_id, "graph": graph}, indent=2))
        print(f"Queued {label}: {prompt_id}", flush=True)
    deadline = time.monotonic() + 43200
    while time.monotonic() < deadline:
        history = requests.get(f"{API}/history/{prompt_id}", timeout=30).json().get(prompt_id)
        if history:
            if history["status"]["status_str"] == "error":
                raise RuntimeError(history["status"]["messages"])
            if history["status"]["completed"]:
                paths = []
                for output in history["outputs"].values():
                    for group in ("images", "videos", "gifs"):
                        for item in output.get(group, []):
                            path = COMFY / "output" / item.get("subfolder", "") / item["filename"]
                            if not path.is_file():
                                raise FileNotFoundError(path)
                            paths.append(path)
                if not paths:
                    raise RuntimeError(f"No files in completed job: {history}")
                return paths
        if connection:
            try:
                event = connection.recv()
                if isinstance(event, str):
                    event = json.loads(event)
                    data = event.get("data", {})
                    if data.get("prompt_id") == prompt_id:
                        if event.get("type") == "progress":
                            print(f"{label}: step {data['value']}/{data['max']}", flush=True)
                        elif event.get("type") == "executing" and data.get("node"):
                            print(f"{label}: {graph.get(str(data['node']), {}).get('class_type', data['node'])}", flush=True)
            except websocket.WebSocketTimeoutException:
                pass
        else:
            time.sleep(5)
    raise TimeoutError(f"Job still running: {prompt_id}")


def node(kind, **inputs):
    return {"class_type": kind, "inputs": inputs}


def portrait(character=None, size=512, steps=20):
    prompt = "An original fictional adult male royal adviser, clean shaven, short brown hair, friendly expressive face, wearing a simple dark green Renaissance velvet tunic with a gold collar, realistic photographed actor with a 1990s strategy-game adviser aesthetic, warm muted colors, even studio lighting, tightly framed head and shoulders, perfectly front facing, looking directly into the camera, eyes open, mouth closed, head upright and still, plain dark charcoal background, no props, no text, no watermark. Square composition with the entire head visible and the face large and centered."
    if character:
        identity = "a young adult man with curly dark hair, clean shaven, wearing a charcoal Renaissance tunic with prominent burnt-orange collar and orange shoulder panels" if character == "claude" else "an older man with short silver hair, clean shaven, wearing a charcoal Renaissance tunic with prominent royal-blue collar and blue shoulder panels"
        prompt = f"An original fictional royal adviser, {identity}. Realistic photographed actor in the style of a 1990s strategy-game adviser. Tightly cropped head and shoulders portrait, entire head visible, face fills most of the square image, front facing, eyes open, mouth closed, looking directly into the camera. Even studio lighting, plain dark charcoal background. No props, no text, no watermark."
    if character == "monty":
        prompt = "An original fictional medieval town crier from a low-budget 1970s British absurdist comedy, Monty Python and the Holy Grail atmosphere. A middle-aged man with an elongated face, enormous drooping brown moustache, crooked pudding-bowl haircut, wearing a battered iron kettle helmet slightly too large for his head, rough brown wool tunic and faded blue heraldic tabard. Deadpan solemn expression, faintly ridiculous but dignified. Muddy earthy colors, photographed on grainy old film, practical theatrical costume, no modern objects. Very tight head and shoulders portrait, entire helmet visible, front facing, eyes open, mouth closed, direct eye contact, plain dark charcoal background. No text, no watermark."
    destination = ASSETS / character if character else ASSETS
    destination.mkdir(parents=True, exist_ok=True)
    graph = {
        "1": node("UNETLoader", unet_name="flux1-dev-fp8.safetensors", weight_dtype="default"),
        "2": node("DualCLIPLoader", clip_name1="clip_l.safetensors", clip_name2="t5xxl_fp8_e4m3fn.safetensors", type="flux", device="default"),
        "3": node("VAELoader", vae_name="ae.safetensors"),
        "4": node("CLIPTextEncode", text=prompt, clip=["2", 0]),
        "5": node("ConditioningZeroOut", conditioning=["4", 0]),
        "6": node("EmptySD3LatentImage", width=size, height=size, batch_size=1),
        "7": node("FluxGuidance", conditioning=["4", 0], guidance=3.5),
        "8": node("KSampler", model=["1", 0], positive=["7", 0], negative=["5", 0], latent_image=["6", 0], seed=478323 if character == "monty" else 478322 if character == "claude" else 478321, steps=steps, cfg=1.0, sampler_name="euler", scheduler="simple", denoise=1.0),
        "9": node("VAEDecode", samples=["8", 0], vae=["3", 0]),
        "10": node("SaveImage", images=["9", 0], filename_prefix=f"civilized/{character or 'original'}/portrait"),
    }
    source = run_graph(graph, f"{character}-portrait" if character else "portrait")[0]
    image = Image.open(source)
    if image.size != (size, size):
        raise RuntimeError(f"Unexpected portrait dimensions: {image.size}")
    shutil.copyfile(source, destination / "portrait-source.png")
    image.resize((128, 128), Image.Resampling.LANCZOS).save(destination / "portrait.png")
    if character:
        runtime = RUNTIME_ASSETS / character
        runtime.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(destination / "portrait.png", runtime / "portrait.png")
    print(destination / "portrait-source.png", flush=True)


def video(emotion, character=None, size=384, frames=None, steps=20, fps=16):
    destination = ASSETS / character if character else ASSETS
    recordings = ASSETS / "recordings"
    audio_path = recordings / f"{emotion}.wav"
    with wave.open(str(audio_path)) as audio:
        duration = audio.getnframes() / audio.getframerate()
    length = frames if frames is not None else max(77, 4 * math.ceil(duration * 16 / 4) + 1)
    image_name = f"civilized-{character or 'original'}-portrait.png"
    audio_name = f"civilized-{emotion}.wav"
    shutil.copyfile(destination / "portrait-source.png", COMFY / "input" / image_name)
    shutil.copyfile(audio_path, COMFY / "input" / audio_name)
    expressions = {
        "neutral": "a calm attentive neutral expression",
    }
    graph = {
        "1": node("UNETLoader", unet_name="wan2.2_s2v_14B_fp8_scaled.safetensors", weight_dtype="default"),
        "2": node("CLIPLoader", clip_name="umt5_xxl_fp8_e4m3fn_scaled.safetensors", type="wan", device="cpu"),
        "3": node("VAELoader", vae_name="wan_2.1_vae.safetensors"),
        "4": node("AudioEncoderLoader", audio_encoder_name="wav2vec2_large_english_fp16.safetensors"),
        "5": node("LoadAudio", audio=audio_name),
        "6": node("AudioEncoderEncode", audio_encoder=["4", 0], audio=["5", 0]),
        "7": node("LoadImage", image=image_name),
        "8": node("CLIPTextEncode", clip=["2", 0], text=f"A front-facing royal adviser speaks the supplied speech naturally with {expressions[emotion]}. Clear precise lip movements, natural blinking. Locked camera, perfectly stationary head, no nodding, no body movement. Preserve the reference person's exact identity, clothing, lighting, framing and plain dark background."),
        "9": node("CLIPTextEncode", clip=["2", 0], text="camera movement, head movement, turning, gestures, exaggerated expression, identity change, face deformation, blurry mouth, text, watermark"),
        "10": node("WanSoundImageToVideo", positive=["8", 0], negative=["9", 0], vae=["3", 0], width=size, height=size, length=length, batch_size=1, audio_encoder_output=["6", 0], ref_image=["7", 0]),
        "11": node("ModelSamplingSD3", model=["1", 0], shift=8.0),
        "12": node("KSampler", model=["11", 0], positive=["10", 0], negative=["10", 1], latent_image=["10", 2], seed=478321, steps=steps, cfg=6.0, sampler_name="uni_pc", scheduler="simple", denoise=1.0),
        "16": node("LatentCut", samples=["12", 0], dim="t", index=0, amount=1),
        "17": node("LatentConcat", samples1=["16", 0], samples2=["12", 0], dim="t"),
        "13": node("VAEDecode", samples=["17", 0], vae=["3", 0]),
        "18": node("ImageFromBatch", image=["13", 0], batch_index=4, length=length),
        "14": node("CreateVideo", images=["18", 0], fps=fps),
        "15": node("SaveVideo", video=["14", 0], filename_prefix=f"civilized/{character + '/' if character else ''}{emotion}", **{"format": "mp4", "format.codec": "h264"}),
    }
    source = run_graph(graph, f"{character}-{emotion}" if character else emotion)[0]
    shutil.copyfile(source, destination / f"{emotion}.mp4")
    print(destination / f"{emotion}.mp4", flush=True)


def slice_video(emotion, character=None):
    source = ASSETS / character if character else ASSETS
    destination = (RUNTIME_ASSETS / character if character else source) / emotion
    destination.mkdir(parents=True, exist_ok=True)
    for frame in destination.glob("frame-*.png"):
        frame.unlink()
    subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-i", str(source / f"{emotion}.mp4"), "-vf", "select='not(mod(n,2))',setpts=N/(8*TB),scale=128:128", "-r", "8", str(destination / "frame-%03d.png")], check=True)
    frames = sorted(destination.glob("frame-*.png"))
    if not frames:
        raise RuntimeError("Video contained no frames")
    manifest = {"emotion": emotion, "fps": 8, "frames": len(frames)}
    (destination / "manifest.json").write_text(json.dumps(manifest, indent=2))
    print(json.dumps(manifest), flush=True)


def release_idle_models():
    try:
        queue = requests.get(f"{API}/queue", timeout=10).json()
        if not queue.get("queue_running") and not queue.get("queue_pending"):
            requests.post(f"{API}/free", json={"unload_models": True, "free_memory": True}, timeout=30).raise_for_status()
            print("Released idle generation models", flush=True)
    except requests.RequestException as error:
        print(f"Could not release generation models: {error}", flush=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("action", choices=["portrait", "video", "slice", "animations", "characters", "preview"])
    parser.add_argument("--emotion", choices=["neutral"], default="neutral")
    parser.add_argument("--character", choices=["claude", "opencode", "monty"])
    args = parser.parse_args()
    try:
        if args.action == "preview":
            portrait(args.character, size=128, steps=12)
            video("neutral", args.character, size=128, frames=17, steps=8, fps=8)
        if args.action == "portrait":
            portrait(args.character)
        if args.action == "video":
            video(args.emotion, args.character)
        if args.action == "slice":
            slice_video(args.emotion, args.character)
        if args.action == "animations":
            video("neutral", args.character)
            slice_video("neutral", args.character)
        if args.action == "characters":
            for character in ("opencode", "claude"):
                video("neutral", character)
                slice_video("neutral", character)
    finally:
        release_idle_models()


if __name__ == "__main__":
    MONITOR = True
    main()
