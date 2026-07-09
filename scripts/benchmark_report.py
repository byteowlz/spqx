#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# ///
"""Generate a self-contained HTML report for scripts/benchmark_spqx.sh output."""

from __future__ import annotations

import argparse
import html
import json
import re
import statistics
from pathlib import Path
from typing import Any


def read_jsonl(path: Path) -> list[dict[str, Any]]:
    """Read compact JSONL, or the older pretty-printed concatenated JSON format."""
    text = path.read_text()
    rows: list[dict[str, Any]] = []

    # Fast path: proper one-object-per-line JSONL.
    try:
        for line in text.splitlines():
            line = line.strip()
            if line:
                rows.append(json.loads(line))
        return rows
    except json.JSONDecodeError:
        rows.clear()

    # Compatibility path for results accidentally written as pretty JSON objects
    # back-to-back. json.JSONDecoder.raw_decode can walk that stream safely.
    decoder = json.JSONDecoder()
    index = 0
    while index < len(text):
        while index < len(text) and text[index].isspace():
            index += 1
        if index >= len(text):
            break
        obj, index = decoder.raw_decode(text, index)
        if isinstance(obj, dict):
            rows.append(obj)
    return rows


def pct(values: list[float], q: float) -> float:
    if not values:
        return 0.0
    values = sorted(values)
    return values[int((len(values) - 1) * q)]


def avg(values: list[float]) -> float:
    return statistics.fmean(values) if values else 0.0


def parse_time_file(path: Path) -> dict[str, str]:
    if not path.exists():
        return {}
    metrics: dict[str, str] = {}
    patterns = {
        "max_rss_bytes": r"(\d+)\s+maximum resident set size",
        "user_s": r"([0-9.]+)\s+user",
        "system_s": r"([0-9.]+)\s+sys",
        "cpu_pct": r"(\d+)%\s+cpu",
        "page_faults": r"(\d+)\s+page faults",
        "voluntary_ctx_switches": r"(\d+)\s+voluntary context switches",
        "involuntary_ctx_switches": r"(\d+)\s+involuntary context switches",
    }
    text = path.read_text(errors="replace")
    for key, pattern in patterns.items():
        match = re.search(pattern, text)
        if match:
            metrics[key] = match.group(1)
    return metrics


def sparkline(values: list[float]) -> str:
    if not values:
        return ""
    bars = "▁▂▃▄▅▆▇█"
    lo = min(values)
    hi = max(values)
    if hi == lo:
        return bars[3] * len(values)
    return "".join(bars[round((v - lo) / (hi - lo) * (len(bars) - 1))] for v in values)


def fmt(value: float, suffix: str = "", digits: int = 2) -> str:
    return f"{value:.{digits}f}{suffix}"


def card(label: str, value: str, sub: str) -> str:
    return f"""
      <section class="card">
        <div class="label">{html.escape(label)}</div>
        <div class="value">{html.escape(value)}</div>
        <div class="sub">{html.escape(sub)}</div>
      </section>
    """


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("bench_dir", type=Path, help="Directory created by benchmark_spqx.sh")
    parser.add_argument("--out", type=Path, help="HTML output path (default: BENCH_DIR/report.html)")
    args = parser.parse_args()

    bench_dir = args.bench_dir
    rows = read_jsonl(bench_dir / "results.jsonl")
    if not rows:
        raise SystemExit(f"no rows found in {bench_dir / 'results.jsonl'}")

    for row in rows:
        run = row.get("run")
        if run is not None:
            row["time"] = parse_time_file(bench_dir / f"time-{run}.txt")

    ttfa = [float(r.get("ttfa_ms", 0)) for r in rows]
    wall = [float(r.get("wall_s", 0)) for r in rows]
    rtf = [float(r.get("rtf", 0)) for r in rows]
    audio = [float(r.get("audio_s", 0)) for r in rows]
    rss = [int(r.get("time", {}).get("max_rss_bytes", 0)) / (1024 * 1024) for r in rows if r.get("time", {}).get("max_rss_bytes")]

    table_rows = []
    for r in rows:
        t = r.get("time", {})
        wav = r.get("wav") or ""
        wav_link = f'<a href="{html.escape(Path(wav).name)}">wav</a>' if wav else ""
        table_rows.append(
            "<tr>"
            f"<td>{html.escape(str(r.get('run', '')))}</td>"
            f"<td>{float(r.get('ttfa_ms', 0)):.0f}</td>"
            f"<td>{float(r.get('wall_s', 0)):.2f}</td>"
            f"<td>{float(r.get('audio_s', 0)):.2f}</td>"
            f"<td>{float(r.get('rtf', 0)):.2f}</td>"
            f"<td>{html.escape(t.get('cpu_pct', ''))}</td>"
            f"<td>{int(t.get('max_rss_bytes', 0)) / (1024 * 1024):.0f}</td>"
            f"<td>{wav_link}</td>"
            "</tr>"
        )

    title = "spqx benchmark report"
    html_doc = f"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>
:root {{ color-scheme: dark; --bg:#0b1020; --panel:#121a2f; --panel2:#17233e; --text:#edf2ff; --muted:#9fb0d0; --accent:#7dd3fc; --good:#86efac; }}
* {{ box-sizing:border-box; }}
body {{ margin:0; font-family: ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; background: radial-gradient(circle at 10% 0%, #1e3a5f 0, transparent 32rem), var(--bg); color:var(--text); }}
main {{ max-width:1120px; margin:0 auto; padding:48px 24px; }}
h1 {{ font-size:44px; line-height:1; margin:0 0 12px; letter-spacing:-0.04em; }}
.lede {{ color:var(--muted); font-size:18px; margin:0 0 28px; }}
.grid {{ display:grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap:14px; margin:24px 0; }}
.card {{ background:linear-gradient(180deg, var(--panel2), var(--panel)); border:1px solid rgba(255,255,255,.08); border-radius:18px; padding:18px; box-shadow:0 20px 60px rgba(0,0,0,.25); }}
.label {{ color:var(--muted); font-size:13px; text-transform:uppercase; letter-spacing:.08em; }}
.value {{ font-size:34px; font-weight:750; letter-spacing:-.04em; margin-top:8px; }}
.sub {{ color:var(--muted); font-size:13px; margin-top:6px; }}
.panel {{ background:rgba(18,26,47,.82); border:1px solid rgba(255,255,255,.08); border-radius:22px; padding:22px; margin-top:18px; }}
.spark {{ font-family: ui-monospace, SFMono-Regular, Menlo, monospace; color:var(--accent); font-size:46px; letter-spacing:2px; white-space:nowrap; overflow:hidden; }}
table {{ width:100%; border-collapse:collapse; margin-top:12px; }}
th, td {{ text-align:right; padding:10px 8px; border-bottom:1px solid rgba(255,255,255,.08); }}
th:first-child, td:first-child {{ text-align:left; }}
th {{ color:var(--muted); font-size:12px; text-transform:uppercase; letter-spacing:.08em; }}
a {{ color:var(--accent); }}
code {{ background:rgba(255,255,255,.08); padding:2px 6px; border-radius:6px; }}
@media (max-width:850px) {{ .grid {{ grid-template-columns:1fr 1fr; }} h1 {{ font-size:34px; }} }}
</style>
</head>
<body>
<main>
  <h1>spqx benchmark report</h1>
  <p class="lede">{len(rows)} measured runs from <code>{html.escape(str(bench_dir))}</code>. Lower TTFA and RTF are better; RTF below 1.0 means faster than real time.</p>
  <div class="grid">
    {card('TTFA avg', fmt(avg(ttfa), ' ms', 0), f"p50 {pct(ttfa, .5):.0f} ms · p95 {pct(ttfa, .95):.0f} ms")}
    {card('Wall avg', fmt(avg(wall), ' s'), f"p50 {pct(wall, .5):.2f} s · p95 {pct(wall, .95):.2f} s")}
    {card('RTF avg', fmt(avg(rtf)), f"p50 {pct(rtf, .5):.2f} · p95 {pct(rtf, .95):.2f}")}
    {card('Max RSS avg', fmt(avg(rss), ' MB', 0) if rss else 'n/a', 'from /usr/bin/time -l')}
  </div>
  <section class="panel">
    <h2>Latency shape</h2>
    <div class="spark" title="TTFA per run">{html.escape(sparkline(ttfa))}</div>
    <p class="lede">TTFA per run: min {min(ttfa):.0f} ms, max {max(ttfa):.0f} ms. Audio avg {avg(audio):.2f} s.</p>
  </section>
  <section class="panel">
    <h2>Runs</h2>
    <table>
      <thead><tr><th>Run</th><th>TTFA ms</th><th>Wall s</th><th>Audio s</th><th>RTF</th><th>CPU %</th><th>RSS MB</th><th>Audio</th></tr></thead>
      <tbody>{''.join(table_rows)}</tbody>
    </table>
  </section>
</main>
</body>
</html>
"""

    out = args.out or bench_dir / "report.html"
    out.write_text(html_doc)
    print(out)


if __name__ == "__main__":
    main()
