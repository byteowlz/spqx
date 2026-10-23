#!/usr/bin/env python3
"""Model-free developer-command tests; fake tools never install or run inference."""
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
BASH = shutil.which("bash")
JUST = shutil.which("just")


class DeveloperTools(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="spqx-dev-tools-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.bin = self.root / "fake tools"
        self.bin.mkdir()
        self.env = os.environ.copy()
        for key in ("SPQX_ACCEL", "SPQX_DEVICE", "SPQX_METALLIB", "SPQX_RUNTIME",
                    "CMAKE_CUDA_ARCHITECTURES", "SPQX_CUDA_ARCHITECTURES", "LIBTORCH",
                    "CARGO_HOME", "CARGO_INSTALL_ROOT", "CARGO_TARGET_DIR"):
            self.env.pop(key, None)
        self.env.update(PATH=f"{self.bin}:{os.environ['PATH']}",
                        CARGO_TARGET_DIR=str(self.root / "build outputs"),
                        CARGO_INSTALL_ROOT=str(self.root / "install root"))
        self.tool("uname", 'case "$1" in -s) echo "${TEST_OS:-Linux}";; -m) echo "${TEST_ARCH:-x86_64}";; esac')
        self.tool("nvidia-smi", 'test "${TEST_NVIDIA:-0}" = 1 || exit 1\ncase "$*" in -L) echo "synthetic GPU";; *compute_cap*) printf "8.6\\n8.9\\n";; esac')
        self.tool("cargo", 'printf "%s\\n" "$@" > "$TEST_CARGO_ARGS"\nprintf "%s\\n" "$CARGO_TARGET_DIR" > "$TEST_CARGO_TARGET"\nif [[ "$*" == *install* ]]; then\n mkdir -p "$CARGO_TARGET_DIR/release/build/spqx-core-fixture/out/lib" "$CARGO_INSTALL_ROOT/bin"\n printf fixture > "$CARGO_TARGET_DIR/release/build/spqx-core-fixture/out/lib/mlx.metallib"\nfi')
        self.env.update(TEST_CARGO_ARGS=str(self.root / "cargo.args"),
                        TEST_CARGO_TARGET=str(self.root / "cargo.target"))
        (self.root / "mlx-c").mkdir()
        (self.root / "mlx-c/CMakeLists.txt").touch()

    def tool(self, name, body):
        path = self.bin / name
        path.write_text(f"#!{BASH}\nset -eu\n{body}\n")
        path.chmod(0o755)

    def run_script(self, name, *args, cwd=None):
        return subprocess.run([BASH, str(ROOT / "scripts" / name), *args],
                              cwd=cwd or self.root, env=self.env,
                              text=True, capture_output=True, timeout=20)

    def run_just(self, *args):
        return subprocess.run([JUST, *args], cwd=ROOT, env=self.env,
                              text=True, capture_output=True, timeout=20)

    def resolve(self, *args):
        result = self.run_script("accel-env.sh", *args)
        self.assertEqual(result.returncode, 0, result.stderr)
        return dict(shlex.split(line.removeprefix("export "))[0].split("=", 1)
                    for line in result.stdout.splitlines())

    def test_platform_matrix_and_unavailable_target(self):
        self.assertEqual(self.resolve()["SPQX_ACCEL"], "cpu")
        self.env["TEST_NVIDIA"] = "1"
        cuda = self.resolve()
        self.assertEqual((cuda["SPQX_ACCEL"], cuda["SPQX_DEVICE"]), ("cuda", "cuda:0"))
        self.assertEqual(cuda["SPQX_CUDA_ARCHITECTURES"], "86;89")
        self.env.update(TEST_OS="Darwin", TEST_ARCH="arm64")
        mlx = self.resolve()
        self.assertEqual(mlx["SPQX_CORE_FEATURES"], "mlx,qwentts")
        self.assertEqual(mlx["SPQX_CLI_FEATURES"], "mlx,qwentts,playback")
        self.env["TEST_ARCH"] = "x86_64"
        failure = self.run_script("accel-env.sh", "mlx")
        self.assertNotEqual(failure.returncode, 0)
        self.assertEqual(failure.stdout, "")

    def test_env_default_explicit_override_and_architecture_priority(self):
        self.env.update(SPQX_ACCEL="cpu", TEST_NVIDIA="1", CMAKE_CUDA_ARCHITECTURES="75")
        self.assertEqual(self.resolve()["SPQX_ACCEL"], "cpu")
        self.assertEqual(self.resolve("auto")["SPQX_ACCEL"], "cuda")
        self.assertEqual(self.resolve("cuda")["CMAKE_CUDA_ARCHITECTURES"], "75")
        self.env["SPQX_CUDA_ARCHITECTURES"] = "89"
        self.assertEqual(self.resolve("cuda")["CMAKE_CUDA_ARCHITECTURES"], "89")
        for unsupported in ("unknown", "tch"):
            result = self.run_script("accel-env.sh", unsupported)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(result.stdout, "")

    def test_cross_build_warning_never_changes_requested_backend(self):
        result = self.run_script("accel-env.sh", "cuda")
        self.assertEqual(result.returncode, 0)
        self.assertIn("no NVIDIA device", result.stderr)
        self.assertIn("SPQX_ACCEL=cuda", result.stdout)
        for backend in ("vulkan", "metal"):
            result = self.run_script("accel-env.sh", backend)
            self.assertIn("not a qualified", result.stderr)
            self.assertIn(f"SPQX_ACCEL={backend}", result.stdout)

    def test_metallib_staging_rejects_ambiguity_and_handles_spaces(self):
        dest = self.root / "installed files"
        dest.mkdir()
        first = Path(self.env["CARGO_TARGET_DIR"]) / "release/build/spqx-core-a/out/lib/mlx.metallib"
        first.parent.mkdir(parents=True)
        first.write_bytes(b"matching fixture")
        self.assertEqual(self.run_script("stage-metallib.sh", str(dest)).returncode, 0)
        second = first.parents[3] / "spqx-core-b/out/lib/mlx.metallib"
        second.parent.mkdir(parents=True)
        second.write_bytes(first.read_bytes())
        self.assertEqual(self.run_script("stage-metallib.sh", str(dest)).returncode, 0)
        second.write_bytes(b"different build")
        result = self.run_script("stage-metallib.sh", str(dest))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("incompatible", result.stderr)
        self.assertEqual((dest / "mlx.metallib").read_bytes(), first.read_bytes())
        self.env["SPQX_METALLIB"] = str(second)
        self.assertEqual(self.run_script("stage-metallib.sh", str(dest)).returncode, 0)
        self.assertEqual((dest / "mlx.metallib").read_bytes(), second.read_bytes())
        self.assertNotEqual(self.run_script("stage-metallib.sh", str(dest), "invalid").returncode, 0)

    @unittest.skipUnless(JUST, "just is required for recipe routing tests")
    def test_just_build_honors_env_and_install_retains_metallib(self):
        self.env["SPQX_ACCEL"] = "cpu"
        result = self.run_just("build")
        self.assertEqual(result.returncode, 0, result.stderr)
        args = (self.root / "cargo.args").read_text()
        self.assertIn("qwentts,playback", args)
        self.assertNotIn("mlx", args)
        self.env.update(TEST_OS="Darwin", TEST_ARCH="arm64")
        result = self.run_just("install", "mlx")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((Path(self.env["CARGO_INSTALL_ROOT"]) / "bin/mlx.metallib").read_bytes(), b"fixture")
        self.assertEqual((self.root / "cargo.target").read_text().strip(), self.env["CARGO_TARGET_DIR"])

    @unittest.skipUnless(JUST, "just is required for recipe routing tests")
    def test_bad_runtime_fails_before_install_and_tch_needs_no_ggml(self):
        self.env["SPQX_RUNTIME"] = str(self.root / "missing runtime")
        result = self.run_just("install", "cpu")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.root / "cargo.args").exists())
        self.env["LIBTORCH"] = str(self.root / "synthetic libtorch")
        result = self.run_just("install", "tch")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("No SPQX_RUNTIME", result.stderr)
        self.assertIn("tch", (self.root / "cargo.args").read_text())

    def test_install_root_precedence(self):
        result = self.run_script("install-dir.sh")
        self.assertEqual(result.stdout.strip(), f"{self.env['CARGO_INSTALL_ROOT']}/bin")
        del self.env["CARGO_INSTALL_ROOT"]
        self.env["CARGO_HOME"] = str(self.root / "cargo home")
        self.assertEqual(self.run_script("install-dir.sh").stdout.strip(), f"{self.env['CARGO_HOME']}/bin")


if __name__ == "__main__":
    unittest.main()
