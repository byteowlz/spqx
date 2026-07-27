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

/// Seconds of non-speech at the end of a finished utterance.
///
/// A generation that loses coherence stops producing speech but keeps
/// emitting frames, so the tail decays to digital silence or a low noise
/// floor while the run still looks successful. Measuring that tail is how
/// callers detect it. Uses the same block/level basis as
/// [`gate_leading_nonspeech`], with the noise floor rather than true zero as
/// the threshold, because the observed failure leaves low-level junk behind.
pub fn trailing_nonspeech_seconds(samples: &[f32], sample_rate: u32) -> f64 {
    const BLOCK_MS: usize = 5;
    const SPEECH_LEVEL: f32 = 0.02;
    if samples.is_empty() || sample_rate == 0 {
        return 0.0;
    }
    let block = (sample_rate as usize * BLOCK_MS / 1000).max(1);
    let mut quiet_samples = 0usize;
    for chunk in samples.chunks(block).rev() {
        let peak = chunk.iter().fold(0f32, |acc, s| acc.max(s.abs()));
        if peak > SPEECH_LEVEL {
            break;
        }
        quiet_samples += chunk.len();
    }
    quiet_samples as f64 / sample_rate as f64
}

/// Drop a degenerate trailing tail, keeping a short natural release.
///
/// Returns the seconds removed. Only trims when the tail is longer than
/// `keep_s`, so normal utterance endings are untouched.
pub fn trim_trailing_nonspeech(samples: &mut Vec<f32>, sample_rate: u32, keep_s: f64) -> f64 {
    let tail = trailing_nonspeech_seconds(samples, sample_rate);
    if tail <= keep_s {
        return 0.0;
    }
    let removed = tail - keep_s;
    let new_len = samples
        .len()
        .saturating_sub((removed * sample_rate as f64) as usize);
    samples.truncate(new_len);
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(seconds: f64, sample_rate: u32) -> Vec<f32> {
        (0..(seconds * sample_rate as f64) as usize)
            .map(|i| (i as f32 / 20.0).sin() * 0.5)
            .collect()
    }

    #[test]
    fn trailing_silence_is_measured() {
        let sr = 24000;
        let mut samples = tone(1.0, sr);
        samples.extend(std::iter::repeat(0.0).take(sr as usize * 2));
        let tail = trailing_nonspeech_seconds(&samples, sr);
        assert!((tail - 2.0).abs() < 0.05, "expected ~2s tail, got {tail}");
    }

    #[test]
    fn low_noise_floor_counts_as_nonspeech() {
        // The observed failure decays to low-level junk, not true zero.
        let sr = 24000;
        let mut samples = tone(1.0, sr);
        samples.extend((0..sr as usize).map(|i| if i % 7 == 0 { 0.003 } else { -0.002 }));
        let tail = trailing_nonspeech_seconds(&samples, sr);
        assert!((tail - 1.0).abs() < 0.05, "expected ~1s tail, got {tail}");
    }

    #[test]
    fn normal_ending_is_not_trimmed() {
        let sr = 24000;
        let mut samples = tone(1.0, sr);
        samples.extend(std::iter::repeat(0.0).take(sr as usize / 10));
        let before = samples.len();
        let removed = trim_trailing_nonspeech(&mut samples, sr, 0.5);
        assert_eq!(removed, 0.0);
        assert_eq!(samples.len(), before);
    }

    #[test]
    fn degenerate_tail_is_trimmed_to_keep_window() {
        let sr = 24000;
        let mut samples = tone(1.0, sr);
        samples.extend(std::iter::repeat(0.0).take(sr as usize * 10));
        let removed = trim_trailing_nonspeech(&mut samples, sr, 0.5);
        assert!((removed - 9.5).abs() < 0.05, "removed {removed}");
        let tail = trailing_nonspeech_seconds(&samples, sr);
        assert!((tail - 0.5).abs() < 0.05, "tail {tail}");
    }
}
