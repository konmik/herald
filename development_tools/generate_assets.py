import argparse
import hashlib
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

if __package__:
    from .asset_library import clean_assets
else:
    from asset_library import clean_assets

ROOT = Path(__file__).resolve().parents[1]
COMFY = Path(r"C:\ComfyUI")
API = "http://127.0.0.1:8188"
ASSETS = ROOT / "generated-assets"
RUNTIME_ASSETS = ROOT / "claude-plugin" / "resources"
MONITOR = False
MEDIEVAL_IDENTITIES = {
    "crier": "a thin elderly town crier with a huge drooping grey moustache, a crooked felt cap, faded rust-red tabard and a deeply solemn expression",
    "herald": "a stern middle-aged female royal herald with short auburn hair, an oversized blue-and-gold heraldic collar and a tiny crooked feathered cap, trying very hard to look dignified",
    "monk": "a round-faced cheerful elderly monk with a bald tonsure, bushy eyebrows, a rough brown hooded robe and an earnest slightly bewildered expression",
    "jester": "a long-faced adult court jester wearing a ridiculous floppy red-and-yellow two-point fool's cap with tiny bells, a ruffled collar and a perfectly deadpan expression",
    "guard": "a weary middle-aged castle guard with a bulbous nose, heavy brown sideburns, a battered oversized iron kettle helmet and a faded green tabard, deeply unimpressed",
    "bishop": "a pompous elderly bishop with a long narrow face, white eyebrows, an absurdly tall battered purple mitre and faded gold vestments, staring gravely as if announcing a very silly decree",
    "executioner": "a burly gentle-looking medieval executioner in a loose black cloth hood with his entire face visible, enormous ginger beard, missing front tooth and faded grey tunic, awkwardly polite",
    "squire": "a lanky freckled young adult squire with large ears, tousled straw-colored hair, an oversized dented steel cap and a faded blue padded tunic, looking earnest and slightly alarmed",
    "scribe": "a elderly female medieval court scribe with round wire spectacles, a long thin face, grey hair under a linen cap and a faded burgundy robe, dryly skeptical with one raised eyebrow",
    "abbess": "a formidable middle-aged abbess with a square face, black veil, oversized white linen wimple and very stern lips, radiating comic disapproval",
    "bailiff": "a stout balding medieval bailiff with a round red nose, thin crooked moustache, a floppy mustard-yellow cap and worn brown leather collar, smugly self-important",
    "plague-doctor": "a medieval plague physician with tired visible human eyes behind round glass lenses and an absurdly short black leather beak mask, battered black wide-brimmed hat and faded olive scarf, practical theatrical costume",
    "alchemist": "a wild-eyed elderly medieval alchemist with unruly white hair, huge bushy white eyebrows, a tiny faded dark-blue pointed cap and patched indigo robe, solemnly convinced of an obviously ridiculous theory",
    "innkeeper": "a cheerful stocky middle-aged female medieval innkeeper with rosy cheeks, messy curly red hair under a crooked linen bonnet, rough russet dress and worn cream neck cloth, warmly amused",
    "trumpeter": "a long-faced middle-aged royal trumpeter with puffed cheeks, a thin curled moustache, an oversized red feathered cap and a faded red-and-gold heraldic tabard, no instrument visible, comically trying to look important",
    "town-crier-01": "an elderly town crier with an enormous white handlebar moustache, deeply wrinkled narrow face, crooked broad-brimmed brown felt hat and faded crimson civic tabard, theatrically solemn",
    "town-crier-02": "a stout middle-aged town crier with a round ruddy face, tiny ginger moustache, an absurdly small black tricorn hat and shabby blue-and-gold civic coat, smugly important",
    "town-crier-03": "a lanky young adult town crier with freckles, sticking-out ears, straw-colored pudding-bowl haircut, floppy burgundy cap and mustard-yellow heraldic collar, nervously earnest",
    "town-crier-04": "a fierce elderly female town crier with a long hooked nose, grey curls beneath a large battered green felt hat, faded red-and-cream civic tabard and a dry disapproving expression",
    "town-crier-05": "a middle-aged town crier with a huge bulbous nose, thick dark eyebrows, black mutton-chop whiskers, crooked rust-colored feathered cap and worn green civic coat, comically bewildered",
    "town-crier-06": "a cheerful elderly town crier with a round face, wispy white chin beard, bushy white eyebrows, broad floppy blue hat and faded orange heraldic collar, warmly amused",
    "town-crier-07": "a stern middle-aged female town crier with a square face, black braided hair, oversized dark-red broad-brimmed hat and blue-and-white civic tabard, rigidly dignified",
    "town-crier-08": "a very thin middle-aged town crier with a long face, drooping black moustache, tiny round spectacles, oversized dark-brown tricorn and faded yellow-and-black heraldic collar, deadpan and exhausted",
    "town-crier-09": "a stocky adult town crier with curly red hair, a short bushy red beard, one crooked eyebrow, a battered olive-green floppy cap and shabby red civic coat with brass buttons, skeptical but good-natured",
    "town-crier-10": "an elderly town crier with a bald domed forehead, long silver sideburns, a crooked oversized purple felt hat perched far back on his head and faded blue-and-gold heraldic collar, painfully self-important",
    "royal-herald-01": "a male royal herald formally proclaiming the king's decrees, a dignified older man with a long face, silver handlebar moustache, dark-blue feathered cap and a blue-and-gold heraldic tabard embroidered with lions, theatrically solemn",
    "royal-herald-02": "a male royal herald delivering his lord's commands, a stout middle-aged man with ruddy cheeks, ginger beard, a red velvet flat cap and red-and-cream heraldic tabard with a gold ceremonial collar, comically self-important",
    "royal-herald-03": "a male royal herald announcing a royal proclamation, a thin young adult man with a narrow face, large ears, black pudding-bowl haircut, small black cap and gold-and-black heraldic tabard with embroidered eagles, nervously dignified",
    "royal-herald-04": "a male royal herald reading the king's orders, an elderly man with a hooked nose, deep wrinkles, white sideburns, a broad burgundy velvet cap and faded purple-and-gold heraldic collar, sternly officious",
    "royal-herald-05": "a male royal herald proclaiming his lord's decree, a middle-aged man with a square jaw, bushy dark eyebrows, enormous drooping black moustache, dark-green feathered cap and green-and-silver coat of arms tabard, deadpan solemnity",
    "royal-herald-06": "a male royal herald announcing the king's commands, a round-faced elderly man with a short white beard, bright blue eyes, an oversized crimson Tudor-style flat cap and crimson-and-gold ceremonial tabard, earnest and slightly bewildered",
    "royal-herald-07": "a male royal herald announcing a nobleman's instructions, a lanky middle-aged man with a long nose, curly auburn hair, a curled auburn moustache, a mustard-yellow feathered cap and blue-and-white heraldic tabard, gravely pompous",
    "royal-herald-08": "a male royal herald formally announcing the king's directions, a bald older man with a domed forehead, heavy grey eyebrows, clean shaven, small dark-purple velvet cap and purple-and-silver ceremonial collar above an embroidered heraldic tabard, dryly unimpressed",
    "royal-herald-09": "a male royal herald proclaiming his lord's orders, a broad-faced middle-aged man with dark curly hair, a neatly pointed brown beard, round wire spectacles, a blue velvet flat cap and orange-and-blue embroidered civic heraldic tabard, painfully precise",
    "royal-herald-10": "a male royal herald delivering a royal decree, a lean older man with a weathered face, a large white walrus moustache, black velvet feathered cap and orange-and-gold heraldic tabard with embroidered lions, stiff-backed ceremonial dignity",
}
CHARACTERS = ("claude", "opencode", "monty", *MEDIEVAL_IDENTITIES)


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
    fingerprint = hashlib.sha256(json.dumps(graph, sort_keys=True).encode()).hexdigest()
    if job_path.exists():
        previous = json.loads(job_path.read_text())
        if previous.get("fingerprint") == fingerprint:
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
        job_path.write_text(json.dumps({"prompt_id": prompt_id, "fingerprint": fingerprint}, indent=2))
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
                job_path.unlink(missing_ok=True)
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
    if character in MEDIEVAL_IDENTITIES:
        prompt = f"An original fictional medieval announcer from a low-budget 1970s British absurdist comedy, Monty Python and the Holy Grail atmosphere: {MEDIEVAL_IDENTITIES[character]}. Realistic photographed actor, faintly ridiculous practical theatrical costume, earthy muted colors, old film aesthetic. Very tight head and shoulders portrait, entire hat visible, face large and centered, perfectly front facing, direct eye contact, eyes open, mouth closed. Fixed soft even studio lighting, plain dark charcoal background, crisp recognizable facial features. No modern objects, no props, no text, no watermark."
    collection = character in MEDIEVAL_IDENTITIES
    destination = ASSETS / "character-portraits"
    destination.mkdir(parents=True, exist_ok=True)
    graph = {
        "1": node("UNETLoader", unet_name="flux1-dev-fp8.safetensors", weight_dtype="default"),
        "2": node("DualCLIPLoader", clip_name1="clip_l.safetensors", clip_name2="t5xxl_fp8_e4m3fn.safetensors", type="flux", device="default"),
        "3": node("VAELoader", vae_name="ae.safetensors"),
        "4": node("CLIPTextEncode", text=prompt, clip=["2", 0]),
        "5": node("ConditioningZeroOut", conditioning=["4", 0]),
        "6": node("EmptySD3LatentImage", width=size, height=size, batch_size=1),
        "7": node("FluxGuidance", conditioning=["4", 0], guidance=3.5),
        "8": node("KSampler", model=["1", 0], positive=["7", 0], negative=["5", 0], latent_image=["6", 0], seed=478324 + list(MEDIEVAL_IDENTITIES).index(character) if character in MEDIEVAL_IDENTITIES else 478323 if character == "monty" else 478322 if character == "claude" else 478321, steps=steps, cfg=1.0, sampler_name="euler", scheduler="simple", denoise=1.0),
        "9": node("VAEDecode", samples=["8", 0], vae=["3", 0]),
        "10": node("SaveImage", images=["9", 0], filename_prefix=f"civilized/{character or 'original'}/portrait"),
    }
    source = run_graph(graph, f"{character}-portrait" if character else "portrait")[0]
    image = Image.open(source)
    if image.size != (size, size):
        raise RuntimeError(f"Unexpected portrait dimensions: {image.size}")
    output = destination / f"{character or 'original'}.png"
    image.resize((128, 128), Image.Resampling.LANCZOS).save(output)
    print(output, flush=True)
    if not collection:
        source_output = destination / f"{character or 'original'}-source.png"
        shutil.copyfile(source, source_output)


def video(emotion, character=None, size=384, frames=None, steps=20, fps=16, cfg=6.0, prompt=None, negative_prompt=None, audio_path=None, output_frames=None, seed=478321):
    destination = ASSETS / character if character else ASSETS
    recordings = ASSETS / "recordings"
    audio_path = Path(audio_path) if audio_path else recordings / f"{emotion}.wav"
    destination.mkdir(parents=True, exist_ok=True)
    with wave.open(str(audio_path)) as audio:
        duration = audio.getnframes() / audio.getframerate()
    length = frames if frames is not None else max(77, 4 * math.ceil(duration * 16 / 4) + 1)
    image_name = f"civilized-{character or 'original'}-portrait.png"
    audio_name = f"civilized-{character or 'original'}-{emotion}.wav"
    reference = destination / "portrait-source.png"
    library_source = ASSETS / "character-portraits" / f"{character or 'original'}-source.png"
    if not reference.exists() and library_source.exists():
        reference = library_source
    if not reference.exists() and character:
        reference = ASSETS / "character-portraits" / f"{character}.png"
    if not reference.exists() and not character:
        reference = ASSETS / "character-portraits" / "original.png"
    shutil.copyfile(reference, COMFY / "input" / image_name)
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
        "8": node("CLIPTextEncode", clip=["2", 0], text=prompt or f"A front-facing royal adviser speaks the supplied speech naturally with {expressions[emotion]}. Small restrained lip movements and a subtle natural blink. Locked camera, perfectly stationary head, no nodding, no body movement. Preserve the reference person's exact identity, helmet, moustache, clothing, framing and plain dark background. Fixed soft studio lighting, constant exposure, constant white balance, unchanged shadows, consistent face brightness throughout the shot. Crisp stable facial features."),
        "9": node("CLIPTextEncode", clip=["2", 0], text=negative_prompt or "flicker, flashing, changing lighting, changing exposure, brightness fluctuations, moving shadows, color shifts, artifacts, noise, distorted face, melting features, blurry mouth, camera movement, head movement, turning, gestures, exaggerated expression, identity change, face deformation, text, watermark"),
        "10": node("WanSoundImageToVideo", positive=["8", 0], negative=["9", 0], vae=["3", 0], width=size, height=size, length=length, batch_size=1, audio_encoder_output=["6", 0], ref_image=["7", 0]),
        "11": node("ModelSamplingSD3", model=["1", 0], shift=8.0),
        "12": node("KSampler", model=["11", 0], positive=["10", 0], negative=["10", 1], latent_image=["10", 2], seed=seed, steps=steps, cfg=cfg, sampler_name="uni_pc", scheduler="simple", denoise=1.0),
        "16": node("LatentCut", samples=["12", 0], dim="t", index=0, amount=1),
        "17": node("LatentConcat", samples1=["16", 0], samples2=["12", 0], dim="t"),
        "13": node("VAEDecode", samples=["17", 0], vae=["3", 0]),
        "18": node("ImageFromBatch", image=["13", 0], batch_index=4, length=output_frames if output_frames is not None else length),
        "14": node("CreateVideo", images=["18", 0], fps=fps),
        "15": node("SaveVideo", video=["14", 0], filename_prefix=f"civilized/{character + '/' if character else ''}{emotion}", **{"format": "mp4", "format.codec": "h264"}),
    }
    source = run_graph(graph, f"{character}-{emotion}" if character else emotion)[0]
    shutil.copyfile(source, destination / f"{emotion}.mp4")
    if character:
        publish_video(emotion, character, size, fps)
    print(destination / f"{emotion}.mp4", flush=True)


def publish_video(emotion, character, size=384, fps=16):
    source = ASSETS / character / f"{emotion}.mp4"
    destination = RUNTIME_ASSETS / character
    destination.mkdir(parents=True, exist_ok=True)
    output = destination / f"{emotion}.mp4"
    if size == 128 and fps == 8:
        shutil.copyfile(source, output)
    else:
        subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-i", str(source), "-an", "-vf", "select='not(mod(n,2))',setpts=N/(8*TB),scale=128:128", "-r", "8", "-c:v", "libx264", "-crf", "18", "-pix_fmt", "yuv420p", "-movflags", "+faststart", str(output)], check=True)
    print(output, flush=True)


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
    parser.add_argument("action", choices=["portrait", "video", "publish", "animations", "characters", "preview", "clean"])
    parser.add_argument("--emotion", choices=["neutral"], default="neutral")
    parser.add_argument("--character", choices=CHARACTERS)
    args = parser.parse_args()
    if args.action == "clean":
        try:
            queue = requests.get(f"{API}/queue", timeout=5).json()
        except requests.ConnectionError:
            queue = {}
        if queue.get("queue_running") or queue.get("queue_pending"):
            parser.error("Wait for ComfyUI generation to finish before cleaning assets")
        print(json.dumps(clean_assets(ASSETS, RUNTIME_ASSETS)))
        return
    try:
        if args.action == "preview":
            portrait(args.character, size=128, steps=12)
            video("neutral", args.character, size=128, frames=17, steps=8, fps=8)
        if args.action == "portrait":
            portrait(args.character)
        if args.action == "video":
            video(args.emotion, args.character)
        if args.action == "publish":
            if not args.character:
                parser.error("publish requires --character")
            publish_video(args.emotion, args.character)
        if args.action == "animations":
            video("neutral", args.character)
        if args.action == "characters":
            for character in ("opencode", "claude"):
                video("neutral", character)
    finally:
        release_idle_models()


if __name__ == "__main__":
    MONITOR = True
    main()
