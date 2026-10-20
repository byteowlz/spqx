"""Capture native dependency source/notices from an owned MLX build tree."""
import argparse
import json
from pathlib import Path
import subprocess
import tarfile


def collect(build, output):
    output.mkdir(parents=True, exist_ok=True)
    records = []
    # FetchContent source trees, never arbitrary model/data/work directories.
    sources = sorted(p for p in build.glob("spqx-core-*/out/build/_deps/*-src") if p.is_dir())
    seen = set()
    for source in sources:
        name = source.name
        if name in seen:
            continue
        seen.add(name)
        archive = output / (name + ".tar.gz")
        revision = subprocess.run(["git", "-C", str(source), "rev-parse", "HEAD"], capture_output=True, text=True) if (source / ".git").exists() else None
        with tarfile.open(archive, "w:gz") as target:
            def safe(info):
                parts = Path(info.name).parts
                if any(part in (".git", "CMakeFiles", "__pycache__", "build") for part in parts):
                    return None
                if info.name.endswith((".gguf", ".safetensors", ".wav")):
                    raise ValueError("unexpected model/audio in native source " + info.name)
                info.uid = info.gid = 0
                info.uname = info.gname = ""
                return info
            target.add(source, arcname=name, filter=safe)
        for notice in source.iterdir():
            if notice.is_file() and notice.name.upper().startswith(("LICENSE", "LICENCE", "COPYING", "NOTICE", "ACKNOWLEDG")):
                (output / (name + "-" + notice.name)).write_bytes(notice.read_bytes())
        records.append({"component": name, "revision": revision.stdout.strip() if revision is not None and revision.returncode == 0 else None,
                        "source_archive": archive.name, "includes_build_patches": True})
    if not any(r["component"] == "mlx-src" for r in records):
        raise ValueError("compiled MLX source tree not found")
    (output / "native-sources.json").write_text(json.dumps(records, indent=2) + "\n")
    return records


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("build", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    print(json.dumps(collect(args.build, args.output)))
