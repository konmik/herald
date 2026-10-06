import os
from pathlib import Path

os.environ["HF_HUB_DISABLE_TELEMETRY"] = "1"

from huggingface_hub import hf_hub_download

ROOT = Path(r"C:\ComfyUI\models")
MODELS = [
    ("Comfy-Org/Wan_2.2_ComfyUI_Repackaged", "diffusion_models", "wan2.2_s2v_14B_fp8_scaled.safetensors"),
    ("Comfy-Org/Wan_2.2_ComfyUI_Repackaged", "audio_encoders", "wav2vec2_large_english_fp16.safetensors"),
    ("Comfy-Org/Wan_2.2_ComfyUI_Repackaged", "vae", "wan_2.1_vae.safetensors"),
    ("Comfy-Org/Wan_2.1_ComfyUI_repackaged", "text_encoders", "umt5_xxl_fp8_e4m3fn_scaled.safetensors"),
]

for repo, folder, name in MODELS:
    destination = ROOT / folder / name
    if destination.is_file():
        print(f"Already installed: {destination}", flush=True)
        continue
    print(f"Installing {name}", flush=True)
    source = Path(hf_hub_download(repo, f"split_files/{folder}/{name}", local_dir=ROOT / ".civilized-download"))
    destination.parent.mkdir(parents=True, exist_ok=True)
    if not destination.exists():
        source.replace(destination)
    print(f"Installed {destination}: {destination.stat().st_size:,} bytes", flush=True)
