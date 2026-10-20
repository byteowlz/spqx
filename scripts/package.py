"""Stage audited native runtime bundles. Build tool only; use `uv run`.

Models, voice references, credentials and issue-store state are never packaged.
Rust crate sources and notices accompany binaries, including MPL components.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import sys
import subprocess
import tarfile
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def copy_file(source, destination):
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, destination, follow_symlinks=True)


def native_sources(source, destination, prefix):
    """Only tracked public source, never build trees, models or local Git state."""
    data = subprocess.check_output(["git", "-C", str(source), "archive", "--format=tar.gz", "--prefix=" + prefix + "/", "HEAD"])
    destination.write_bytes(data)


def rust_materials(stage, target, features):
    cmd = ["cargo", "metadata", "--locked", "--format-version", "1", "--no-default-features",
           "--features", features, "--filter-platform", target]
    metadata = json.loads(subprocess.check_output(cmd, cwd=ROOT))
    packages = {p["id"]: p for p in metadata["packages"]}
    nodes = {n["id"]: n for n in metadata["resolve"]["nodes"]}
    pending = [p["id"] for p in metadata["packages"] if p["name"] in ("spqx-cli", "spqx-core")]
    seen = set()
    while pending:
        item = pending.pop()
        if item in seen:
            continue
        seen.add(item)
        for dep in nodes[item]["deps"]:
            if any(kind["kind"] != "dev" for kind in dep["dep_kinds"]):
                pending.append(dep["pkg"])
    records = []
    for item in sorted(seen):
        package = packages[item]
        if not package["source"]:
            continue
        name = package["name"] + "-" + package["version"]
        if package["name"] in ("mp3lame-encoder", "mp3lame-sys", "audiopus_sys", "tch", "torch-sys"):
            raise ValueError("portable bundle unexpectedly enables legacy native dependency " + name)
        manifest = Path(package["manifest_path"])
        directory = manifest.parent
        # Cargo registry archives are the original full corresponding crate sources.
        registry = directory.parent
        original = registry.parent.parent / "cache" / registry.name / (name + ".crate")
        if not original.is_file():
            raise ValueError("missing original crate archive; run cargo fetch: " + str(original))
        copy_file(original, stage / "sources" / "rust" / original.name)
        notices = []
        for path in directory.iterdir():
            if path.is_file() and path.name.upper().startswith(("LICENSE", "LICENCE", "COPYING", "NOTICE", "UNLICENSE", "AUTHORS")):
                copy_file(path, stage / "licenses" / "rust" / name / path.name)
                notices.append(path.name)
        custom = package.get("license_file")
        if custom:
            path = Path(custom)
            if not path.is_absolute():
                path = directory / path
            copy_file(path, stage / "licenses" / "rust" / name / path.name)
            notices.append(path.name)
        records.append({"name": package["name"], "version": package["version"], "license": package["license"],
                        "source": package["source"], "source_sha256": digest(original), "notices": sorted(set(notices))})
    return records


def unix_launcher(stage, name):
    path = stage / "bin" / name
    path.rename(path.with_name(name + "-real"))
    path.write_text('#!/usr/bin/env bash\nset -euo pipefail\nhere=$(cd -- "$(dirname -- "$(readlink -f -- "${BASH_SOURCE[0]}")")" && pwd)\n'
                    'export LD_LIBRARY_PATH="$here/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"\n'
                    'exec "$here/' + name + '-real" "$@"\n')
    path.chmod(0o755)


def installers(stage, name, windows):
    if windows:
        (stage / "install.cmd").write_bytes((
            '@echo off\r\nsetlocal\r\nset "DEST=%~1"\r\n'
            'if not defined DEST set "DEST=%LOCALAPPDATA%\\Programs\\spqx\\' + name + '"\r\n'
            'if exist "%DEST%" (echo Destination already exists & exit /b 2)\r\n'
            'mkdir "%DEST%" 2>nul\r\nxcopy "%~dp0*" "%DEST%\\" /E /I /Y >nul\r\n'
            'if errorlevel 1 exit /b 1\r\necho Installed: %DEST%\\bin\\spqx.exe\r\n'
            'echo Existing configuration and PATH were not modified.\r\n').encode())
    else:
        script = '#!/usr/bin/env bash\nset -euo pipefail\nbase=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)\n'
        script += 'prefix=${1:-$HOME/.local}\ndest="$prefix/lib/spqx/' + name + '"\n'
        script += '[[ ! -e "$dest" ]] || { echo "Destination already exists" >&2; exit 2; }\n'
        script += 'for n in spqx spqx-tts-worker; do [[ ! -e "$prefix/bin/$n" && ! -L "$prefix/bin/$n" ]] || { echo "Refusing to replace existing $n" >&2; exit 2; }; done\n'
        script += 'mkdir -p "$dest" "$prefix/bin"\ncp -R "$base/." "$dest/"\n'
        script += 'for n in spqx spqx-tts-worker; do ln -s "$dest/bin/$n" "$prefix/bin/$n"; done\n'
        script += 'printf "Installed in %s; configuration and PATH unchanged\\n" "$prefix"\n'
        (stage / "install.sh").write_text(script)
        (stage / "install.sh").chmod(0o755)


def package(args):
    if not args.native_source and not args.material:
        raise ValueError("native corresponding source materials are required")
    revision = args.source_revision or subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    name = "spqx-" + revision[:12] + "-" + args.target + "-" + args.backend
    stage = args.output / name
    if stage.exists():
        raise ValueError("refusing to mix or overwrite a bundle: " + str(stage))
    (stage / "bin").mkdir(parents=True)
    (stage / "sources").mkdir()
    windows = "windows" in args.target
    for binary in ("spqx", "spqx-tts-worker"):
        file = binary + (".exe" if windows else "")
        copy_file(args.binaries / file, stage / "bin" / file)
    for source in ([args.runtime] if args.runtime else []):
        for path in source.iterdir():
            if path.is_file() and (".so" in path.name or path.suffix in (".dll", ".dylib")):
                if path.is_symlink():
                    target = os.readlink(path)
                    if Path(target).name != target:
                        raise ValueError("nonlocal native symlink " + str(path))
                    (stage / "bin/lib").mkdir(exist_ok=True)
                    (stage / "bin/lib" / path.name).symlink_to(target)
                else:
                    copy_file(path, stage / "bin/lib" / path.name)
    for path in args.extra_library:
        copy_file(path, stage / "bin" / ("" if windows else "lib") / path.name)
    for path in args.material:
        copy_file(path, stage / "sources/native" / path.name)
    for path in args.extra_license:
        copy_file(path, stage / "licenses/native" / path.name)
    if args.backend == "mlx":
        if not args.metallib:
            raise ValueError("MLX bundle requires its exact compiled mlx.metallib")
        copy_file(args.metallib, stage / "bin/mlx.metallib")
    else:
        needed = "qwen.dll" if windows else "libqwen.so"
        if not (stage / "bin/lib" / needed).is_file():
            raise ValueError("native engine missing " + needed)
    if "linux" in args.target:
        for binary in ("spqx", "spqx-tts-worker"):
            unix_launcher(stage, binary)
    for file in ("LICENSE", "README.md", "CHANGELOG.md"):
        copy_file(ROOT / file, stage / file)
    copy_file(ROOT / "docs/portable-engines.md", stage / "PORTABLE-ENGINES.md")
    if args.native_source:
        for filename in ("LICENSE", "ACKNOWLEDGMENTS.md"):
            if (args.native_source / filename).is_file():
                copy_file(args.native_source / filename, stage / "licenses/native" / ("engine-" + filename))
        native_sources(args.native_source, stage / "sources/engine.tar.gz", "engine")
        ggml = args.native_source / "ggml"
        if ggml.is_dir():
            copy_file(ggml / "LICENSE", stage / "licenses/native/ggml-LICENSE")
            native_sources(ggml, stage / "sources/ggml.tar.gz", "ggml")
    if args.project_source:
        # Remote build trees need not contain Git credentials/history. Their
        # production source must exactly match the supplied public source tar.
        with tarfile.open(args.project_source) as archive:
            for entry in archive:
                if entry.isfile() and (entry.name in ("Cargo.toml", "Cargo.lock") or entry.name.startswith("crates/")):
                    path = ROOT / entry.name
                    if ".." in Path(entry.name).parts or not path.is_file() or path.read_bytes() != archive.extractfile(entry).read():
                        raise ValueError("compiled source differs from source archive: " + entry.name)
        copy_file(args.project_source, stage / "sources/spqx.tar.gz")
    else:
        project = subprocess.check_output(["git", "archive", "--format=tar.gz", "HEAD", "Cargo.toml", "Cargo.lock", "crates", "scripts", "docs", "LICENSE", "README.md", "CHANGELOG.md", "CONTEXT.md", "justfile", "install.sh", "CLAUDE.md"], cwd=ROOT)
        (stage / "sources/spqx.tar.gz").write_bytes(project)
    records = rust_materials(stage, args.target, args.features)
    (stage / "THIRD-PARTY-NOTICES.md").write_text(
        "# Third-party components\n\nThe application license does not replace component licenses. "
        "Rust notices are under licenses/rust; original complete crate sources are under sources/rust. "
        "Native notices and corresponding source archives are under licenses/native and sources. "
        "Vendor redistributables retain their own terms. System OS libraries and GPU drivers are not included. "
        "No model weights, voice recordings or credentials are included.\n")
    installers(stage, name, windows)
    manifest = {"schema_version": 1, "source_revision": revision, "target": args.target, "backend": args.backend,
                "features": args.features.split(","), "requirements": args.requirement,
                "models_included": False, "recordings_included": False, "rust_dependencies": records}
    manifest["files"] = {p.relative_to(stage).as_posix(): {"symlink": os.readlink(p)} if p.is_symlink()
                         else {"sha256": digest(p), "bytes": p.stat().st_size}
                         for p in sorted(stage.rglob("*")) if p.is_file() or p.is_symlink()}
    (stage / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    archive = write_archive(stage, windows)
    archive.with_name(archive.name + ".sha256").write_text(digest(archive) + "  " + archive.name + "\n")
    print(json.dumps({"bundle": str(archive), "sha256": digest(archive), "manifest": str(stage / "manifest.json")}))
    return stage


def write_archive(stage, windows):
    name = stage.name
    if windows:
        archive = stage.with_suffix(".zip")
        with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED, strict_timestamps=False) as zipped:
            for p in sorted(stage.rglob("*")):
                if p.is_file():
                    zipped.write(p, str(Path(name) / p.relative_to(stage)))
    else:
        archive = stage.with_suffix(".tar.gz")
        with tarfile.open(archive, "w:gz") as tar:
            tar.add(stage, arcname=name, filter=lambda info: _neutral_owner(info))
    return archive


def _neutral_owner(info):
    info.uid = info.gid = 0
    info.uname = info.gname = ""
    return info


def verify(stage):
    stage = stage.resolve()
    manifest = json.loads((stage / "manifest.json").read_text())
    for name, record in manifest["files"].items():
        relative = Path(name)
        if relative.is_absolute() or ".." in relative.parts:
            raise ValueError("unsafe manifest path")
        path = stage / relative
        if not path.resolve().is_relative_to(stage):
            raise ValueError("escaping symlink")
        if "symlink" in record:
            if not path.is_symlink() or os.readlink(path) != record["symlink"]:
                raise ValueError("symlink mismatch: " + name)
        elif not path.is_file() or digest(path) != record["sha256"]:
            raise ValueError("file mismatch: " + name)
    actual = {p.relative_to(stage).as_posix() for p in stage.rglob("*") if p.is_file() or p.is_symlink()}
    if actual != set(manifest["files"]) | {"manifest.json"}:
        raise ValueError("unrecorded or missing bundle files")
    return len(manifest["files"])


def main():
    if len(sys.argv) == 3 and sys.argv[1] == "--verify":
        print(json.dumps({"verified_files": verify(Path(sys.argv[2]))})); return
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", required=True)
    parser.add_argument("--backend", choices=("cpu", "cuda", "mlx"), required=True)
    parser.add_argument("--features", required=True)
    parser.add_argument("--source-revision")
    parser.add_argument("--project-source", type=Path)
    for name in ("binaries", "runtime", "metallib", "native-source", "output"):
        parser.add_argument("--" + name, type=Path, required=name in ("binaries", "output"))
    parser.add_argument("--material", type=Path, action="append", default=[])
    parser.add_argument("--extra-library", type=Path, action="append", default=[])
    parser.add_argument("--extra-license", type=Path, action="append", default=[])
    parser.add_argument("--requirement", action="append", default=[])
    package(parser.parse_args())


if __name__ == "__main__":
    main()
