#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Prepare a 6-12 second voice-cloning reference using trnscrb timestamps.

Usage:
  prepare_voice_reference.sh [options] AUDIO

Options:
  -o, --output DIR          Output directory (default: prepared-reference)
  --transcript JSON         Reuse an existing trnscrb transcript.json
  --backend NAME            trnscrb backend: whisper or parakeet (default: parakeet)
  --language CODE           Language code or auto (default: auto)
  --target SECONDS          Preferred clip duration (default: 9)
  --min SECONDS             Minimum clip duration (default: 6)
  --max SECONDS             Maximum clip duration (default: 12)
  --padding SECONDS         Audio padding on each side (default: 0.15)
  --play                    Play the prepared WAV after writing it
  --force                   Replace output files
  -h, --help                Show this help

The script transcribes the audio when --transcript is omitted, selects the
contiguous transcript segment window closest to the target duration, and writes
reference.wav, reference.txt, and preparation.json. Requires trnscrb, jq,
ffmpeg, and ffprobe.
EOF
}

output="prepared-reference"
transcript=""
backend="parakeet"
language="auto"
target="9"
minimum="6"
maximum="12"
padding="0.15"
force=0
play=0

while (($#)); do
  case "$1" in
    -o|--output) output=${2:?missing output directory}; shift 2 ;;
    --transcript) transcript=${2:?missing transcript path}; shift 2 ;;
    --backend) backend=${2:?missing backend}; shift 2 ;;
    --language) language=${2:?missing language}; shift 2 ;;
    --target) target=${2:?missing seconds}; shift 2 ;;
    --min) minimum=${2:?missing seconds}; shift 2 ;;
    --max) maximum=${2:?missing seconds}; shift 2 ;;
    --padding) padding=${2:?missing seconds}; shift 2 ;;
    --play) play=1; shift ;;
    --force) force=1; shift ;;
    -h|--help) usage; exit 0 ;;
    --) shift; break ;;
    -*) echo "error: unknown option: $1" >&2; usage >&2; exit 2 ;;
    *) break ;;
  esac
done

if (($# != 1)); then usage >&2; exit 2; fi
audio=$1
[[ -f "$audio" ]] || { echo "error: audio not found: $audio" >&2; exit 1; }
for command in trnscrb jq ffmpeg ffprobe; do
  command -v "$command" >/dev/null || { echo "error: required command not found: $command" >&2; exit 1; }
done
if [[ -e "$output" && $force -ne 1 ]]; then
  echo "error: output exists: $output (pass --force to replace its generated files)" >&2
  exit 1
fi
mkdir -p "$output"

if [[ -z "$transcript" ]]; then
  log=$(mktemp)
  trap 'rm -f "$log"' EXIT

  # trnscrb currently expects the NVIDIA-style parakeet CLI interface:
  # `parakeet-cli transcribe --input ... --json`. Homebrew's parakeet.cpp CLI
  # uses `-f` and cannot emit that JSON shape, despite having the same binary
  # name. Fall back to Whisper instead of failing with "unknown --input".
  if [[ "$backend" == "parakeet" ]]; then
    parakeet_help=$(parakeet-cli --help 2>&1 || true)
    if ! grep -q -- '--input' <<<"$parakeet_help" || ! grep -q -- '--json' <<<"$parakeet_help"; then
      echo "warning: installed parakeet-cli is the incompatible parakeet.cpp CLI; falling back to whisper" >&2
      echo "         (trnscrb requires: parakeet-cli transcribe --input FILE --json)" >&2
      backend="whisper"
    fi
  fi

  echo "Transcribing with trnscrb ($backend)..." >&2
  trnscrb run "$audio" --transcribe-only --backend "$backend" --language "$language" --no-progress 2>&1 | tee "$log" >&2
  transcript=$(awk '/^Transcript:[[:space:]]/ { sub(/^Transcript:[[:space:]]*/, ""); sub(/ \([0-9]+ segments\)$/, ""); print; exit }' "$log")
  [[ -n "$transcript" ]] || { echo "error: could not locate transcript.json in trnscrb output" >&2; exit 1; }
fi
[[ -f "$transcript" ]] || { echo "error: transcript not found: $transcript" >&2; exit 1; }

selection=$(jq -r --argjson min "$minimum" --argjson max "$maximum" --argjson target "$target" '
  [.segments[] | select(.end > .start and (.text | length) > 0)] as $s
  | [range(0; $s|length) as $i
      | range($i + 1; ($s|length) + 1) as $j
      | ($s[$j-1].end - $s[$i].start) as $d
      | select($d >= $min and $d <= $max)
      | {start:$s[$i].start, end:$s[$j-1].end, duration:$d,
         text:([$s[$i:$j][].text] | join(" ")),
         score:(($d-$target)|fabs) + (if ($s[$j-1].text|test("[.!?…][\\\")]]?$")) then 0 else 0.35 end)}]
  | if length == 0 then empty else sort_by(.score) | .[0] | [.start,.end,.duration,.text] | @tsv end
' "$transcript")
[[ -n "$selection" ]] || {
  echo "error: no contiguous transcript window between ${minimum}s and ${maximum}s" >&2
  echo "hint: adjust --min/--max or inspect $transcript" >&2
  exit 1
}
IFS=$'\t' read -r speech_start speech_end speech_duration text <<<"$selection"

audio_duration=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$audio")
clip_start=$(jq -nr --argjson s "$speech_start" --argjson p "$padding" '$s-$p | if . < 0 then 0 else . end')
clip_end=$(jq -nr --argjson e "$speech_end" --argjson p "$padding" --argjson total "$audio_duration" '$e+$p | if . > $total then $total else . end')
clip_duration=$(jq -nr --argjson a "$clip_start" --argjson b "$clip_end" '$b-$a')

printf '%s\n' "$text" >"$output/reference.txt"
ffmpeg -hide_banner -loglevel error -y -ss "$clip_start" -i "$audio" -t "$clip_duration" \
  -ac 1 -ar 24000 -c:a pcm_s16le "$output/reference.wav"
jq -n \
  --arg source "$(cd "$(dirname "$audio")" && pwd)/$(basename "$audio")" \
  --arg transcript "$(cd "$(dirname "$transcript")" && pwd)/$(basename "$transcript")" \
  --argjson speech_start "$speech_start" --argjson speech_end "$speech_end" \
  --argjson clip_start "$clip_start" --argjson clip_end "$clip_end" \
  --argjson target "$target" --arg text "$text" \
  '{source:$source, transcript:$transcript, target_seconds:$target,
    selected_speech:{start:$speech_start,end:$speech_end},
    clip:{start:$clip_start,end:$clip_end,duration:($clip_end-$clip_start)}, text:$text}' \
  >"$output/preparation.json"

echo "Prepared reference: $output/reference.wav (${clip_duration}s)"
echo "Transcript:         $output/reference.txt"
echo "Selection:          ${speech_start}s-${speech_end}s"
echo
echo "Review the WAV, then enroll:"
echo "  spqx voices add <name> --ref-audio '$output/reference.wav' --ref-text-file '$output/reference.txt'"

if ((play)); then
  echo
  echo "Playing $output/reference.wav..."
  if command -v afplay >/dev/null 2>&1; then
    afplay "$output/reference.wav"
  elif command -v ffplay >/dev/null 2>&1; then
    ffplay -autoexit -nodisp -loglevel error "$output/reference.wav"
  else
    echo "warning: neither afplay nor ffplay is available; WAV was still written" >&2
  fi
fi
