from pathlib import Path


def clean_assets(assets, runtime_assets):
    assets = Path(assets)
    runtime_assets = Path(runtime_assets)
    files = [path for root in (assets, runtime_assets) for path in root.rglob("*") if path.is_file()]
    resources = [path for path in files if path.suffix == ".png" or path.name == "neutral.mp4" or (path.parent == runtime_assets / "videos" and path.suffix == ".mp4")]
    targets = {}
    for resource in resources:
        target = resource
        if resource.suffix == ".png" and resource.parent != runtime_assets / "portraits":
            if resource.parent == assets / "character-portraits":
                name = resource.name
            else:
                character = "original" if resource.parent == assets else resource.parent.name
                suffix = "-source" if resource.stem == "portrait-source" else "-native" if resource.stem == "portrait-native" else "" if resource.stem == "portrait" else "-" + resource.stem
                name = f"{character}{suffix}.png"
            target = runtime_assets / "portraits" / name
            if target.exists():
                raise FileExistsError(f"Cannot move {resource}: {target} already exists")
        if target in targets.values():
            raise FileExistsError(f"Two portraits would move to {target}")
        targets[resource] = target
    keep = set(targets.values()) | {runtime_assets / "characters.json"}
    moved = {resource for resource, target in targets.items() if resource != target}
    remove = [path for path in files if path not in keep and path not in moved]
    for resource in moved:
        targets[resource].parent.mkdir(parents=True, exist_ok=True)
        resource.rename(targets[resource])
    for path in remove:
        path.unlink()
    directories = sorted(
        [path for root in (assets, runtime_assets) for path in root.rglob("*") if path.is_dir()],
        key=lambda path: len(path.parts), reverse=True,
    )
    for directory in directories:
        if not any(directory.iterdir()):
            directory.rmdir()
    portraits = sum(path.suffix == ".png" for path in resources)
    return {"portraits": portraits, "videos": len(resources) - portraits, "removed": len(remove)}
