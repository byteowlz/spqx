//! Audio output cleanup shared by the worker and the CLI.

/// Gate leading non-speech in the first synthesized chunk.
///
/// The model can emit an isolated click between the ICL prompt boundary and
/// the actual speech onset (present in the Python MLX worker too). This finds
/// the sustained speech onset and zeroes only isolated high bursts that sit
/// before it in real silence — never the wholesale leading region, and never
/// anything longer than ~50ms, so unvoiced plosives and softly-spoken leading
/// words survive. Apply to the FIRST emitted chunk of an utterance only.
pub fn gate_leading_nonspeech(samples: &mut [f32], sample_rate: u32) {
    const BLOCK_MS: usize = 5;
    const SPEECH_LEVEL: f32 = 0.02;
    const CLICK_LEVEL: f32 = 0.03;
    const QUIET_LEVEL: f32 = 0.004;
    let block = (sample_rate as usize * BLOCK_MS / 1000).max(1);
    let peaks: Vec<f32> = samples
        .chunks(block)
        .map(|chunk| chunk.iter().fold(0f32, |acc, s| acc.max(s.abs())))
        .collect();

    // Sustained speech = dense loudness, not just a run: at least 40ms of
    // loud blocks within the 120ms window after the candidate. Model-emitted
    // onset clicks reach 20ms and defeat any run-length test, but no real
    // word is 20ms followed by silence. A candidate too close to the buffer
    // end is assumed to be speech continuing into the next chunk.
    let mut onset_search = 0usize;
    let mut onset_block = peaks.len();
    while onset_search < peaks.len() {
        if peaks[onset_search] <= SPEECH_LEVEL {
            onset_search += 1;
            continue;
        }
        let window_end = (onset_search + 24).min(peaks.len());
        let loud = peaks[onset_search..window_end]
            .iter()
            .filter(|p| **p > SPEECH_LEVEL)
            .count();
        if window_end - onset_search < 8 || loud >= 8 {
            onset_block = onset_search;
            break;
        }
        let run_end = (onset_search..peaks.len())
            .find(|&j| peaks[j] <= SPEECH_LEVEL)
            .unwrap_or(peaks.len());
        onset_search = run_end;
    }

    // Walk back over the quiet lead-in adjacent to the onset: unvoiced
    // plosives and breath ("T" in "Tune") sit below SPEECH_LEVEL but above
    // silence, and blanket-zeroing up to the onset audibly ate them. Only
    // what lies before this true start — separated by real silence — is
    // click territory.
    let mut true_start = onset_block;
    while true_start > 0 && peaks[true_start - 1] > QUIET_LEVEL {
        true_start -= 1;
    }

    // Zero only isolated high bursts (plus quiet shoulders) in the leading
    // silence — never the region wholesale, and never anything longer than
    // 50ms: a click's connected above-silence region is short, while a
    // softly spoken word that failed the onset density test spans 100ms+.
    // Without the duration cap this pass erased whole leading words.
    let mut index = 0usize;
    while index < true_start {
        if peaks[index] > CLICK_LEVEL {
            let mut start_block = index;
            while start_block > 0 && peaks[start_block - 1] > QUIET_LEVEL {
                start_block -= 1;
            }
            let mut end_block = index + 1;
            while end_block < true_start && peaks[end_block] > QUIET_LEVEL {
                end_block += 1;
            }
            if end_block - start_block <= 10 {
                let start = start_block * block;
                let end = (end_block * block).min(samples.len());
                for sample in samples[start..end].iter_mut() {
                    *sample = 0.0;
                }
            }
            index = end_block;
        } else {
            index += 1;
        }
    }

    // Short fade at the very start guards the residual DC step.
    let fade = (sample_rate as usize * 3 / 1000).min(samples.len());
    for (i, sample) in samples[..fade].iter_mut().enumerate() {
        *sample *= i as f32 / fade.max(1) as f32;
    }
}
