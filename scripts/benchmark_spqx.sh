#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/benchmark_spqx.sh [OPTIONS] -- TEXT

Benchmark `spqx say` latency and process resource use.

Options:
  --bin PATH              spqx binary to run (default: target/release/spqx)
  --config PATH           config file to pass to spqx
  --voice NAME            voice registry name to pass to spqx
  --ref-audio PATH        ad-hoc reference WAV
  --ref-text TEXT         transcript for --ref-audio
  --ref-text-file PATH    transcript file for --ref-audio
  --out-dir DIR           directory for per-run WAV/metric files (default: target/spqx-bench/<timestamp>)
  --runs N                measured runs (default: 5)
  --warmups N             warmup runs before measuring (default: 1)
  --no-build              do not build target/release/spqx first
  -h, --help              show this help

Examples:
  scripts/benchmark_spqx.sh --voice demo --runs 10 -- "Hello from spqx."
  scripts/benchmark_spqx.sh --ref-audio ref.wav --ref-text-file ref.txt -- "Benchmark text."

Outputs:
  results.jsonl           one spqx --json object per measured run, with run index and WAV path
  time-*.txt              /usr/bin/time output for each measured run when available
  summary.txt             lightweight summary if jq is installed

Notes:
  spqx already reports ttfa_ms, wall_s, audio_s, and rtf via `spqx --json say`.
  This wrapper adds OS process metrics from /usr/bin/time where supported:
  max RSS, CPU percentage, page faults, context switches, etc.
EOF
}

bin="target/release/spqx"
config=""
voice=""
ref_audio=""
ref_text=""
ref_text_file=""
runs=5
warmups=1
build=1
out_dir="target/spqx-bench/$(date +%Y%m%d-%H%M%S)"
text=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --bin) bin="$2"; shift 2 ;;
    --config) config="$2"; shift 2 ;;
    --voice) voice="$2"; shift 2 ;;
    --ref-audio) ref_audio="$2"; shift 2 ;;
    --ref-text) ref_text="$2"; shift 2 ;;
    --ref-text-file) ref_text_file="$2"; shift 2 ;;
    --out-dir) out_dir="$2"; shift 2 ;;
    --runs) runs="$2"; shift 2 ;;
    --warmups) warmups="$2"; shift 2 ;;
    --no-build) build=0; shift ;;
    -h|--help) usage; exit 0 ;;
    --) shift; text="${*:-}"; break ;;
    *) echo "unknown option: $1" >&2; usage >&2; exit 2 ;;
  esac
done

if [[ -z "$text" ]]; then
  echo "missing TEXT after --" >&2
  usage >&2
  exit 2
fi

if [[ "$build" == 1 ]]; then
  cargo build --release -p spqx-cli
fi

mkdir -p "$out_dir"
results="$out_dir/results.jsonl"
: > "$results"

spqx_args=(--json)
[[ -n "$config" ]] && spqx_args+=(--config "$config")
spqx_args+=(say "$text" --no-play)
[[ -n "$voice" ]] && spqx_args+=(--voice "$voice")
[[ -n "$ref_audio" ]] && spqx_args+=(--ref-audio "$ref_audio")
[[ -n "$ref_text" ]] && spqx_args+=(--ref-text "$ref_text")
[[ -n "$ref_text_file" ]] && spqx_args+=(--ref-text-file "$ref_text_file")

run_once() {
  local idx="$1"
  local measured="$2"
  local wav="$out_dir/run-${idx}.wav"
  local stdout_file="$out_dir/stdout-${idx}.json"
  local stderr_file="$out_dir/stderr-${idx}.txt"
  local time_file="$out_dir/time-${idx}.txt"

  if [[ "$measured" == 1 && -x /usr/bin/time ]]; then
    /usr/bin/time -l -o "$time_file" "$bin" "${spqx_args[@]}" --out "$wav" >"$stdout_file" 2>"$stderr_file" \
      || { cat "$stderr_file" >&2; return 1; }
  else
    "$bin" "${spqx_args[@]}" --out "$wav" >"$stdout_file" 2>"$stderr_file" \
      || { cat "$stderr_file" >&2; return 1; }
  fi

  if [[ "$measured" == 1 ]]; then
    if command -v jq >/dev/null 2>&1; then
      jq -c --argjson run "$idx" --arg wav "$wav" '. + {run: $run, wav: $wav}' "$stdout_file" >> "$results"
    else
      cat "$stdout_file" >> "$results"
    fi
  fi
}

for ((i=1; i<=warmups; i++)); do
  echo "warmup $i/$warmups" >&2
  run_once "warmup-${i}" 0
done

for ((i=1; i<=runs; i++)); do
  echo "run $i/$runs" >&2
  run_once "$i" 1
done

if command -v jq >/dev/null 2>&1; then
  jq -rs '
    def avg(k): map(.[k] // 0) | add / length;
    def sorted(k): sort_by(.[k]);
    def p(k; q): sorted(k)[((length - 1) * q | floor)][k];
    {
      runs: length,
      ttfa_ms_avg: avg("ttfa_ms"), ttfa_ms_p50: p("ttfa_ms"; 0.50), ttfa_ms_p95: p("ttfa_ms"; 0.95),
      wall_s_avg: avg("wall_s"), wall_s_p50: p("wall_s"; 0.50), wall_s_p95: p("wall_s"; 0.95),
      rtf_avg: avg("rtf"), rtf_p50: p("rtf"; 0.50), rtf_p95: p("rtf"; 0.95),
      audio_s_avg: avg("audio_s")
    }' "$results" | tee "$out_dir/summary.txt"
else
  echo "Install jq for summary statistics. Raw results: $results" | tee "$out_dir/summary.txt"
fi

if command -v uv >/dev/null 2>&1; then
  scripts/benchmark_report.py "$out_dir" >/dev/null
  echo "html report: $out_dir/report.html" >&2
fi

echo "benchmark output: $out_dir" >&2
