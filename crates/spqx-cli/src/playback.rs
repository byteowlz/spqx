//! Live audio playback bridge: a rodio `Source` fed by synthesis chunks over
//! a channel, so audio starts playing as soon as the first chunk arrives
//! (adapted from kokorox's ChannelSource).

use std::sync::mpsc::Receiver;
use std::time::Duration;

use anyhow::{Context, Result};
use rodio::{OutputStream, Sink, Source};

/// Streaming f32-chunk source. Blocks on the channel between chunks; ends when
/// the sender is dropped.
struct ChannelSource {
    rx: Receiver<Vec<f32>>,
    current: std::vec::IntoIter<f32>,
    sample_rate: u32,
}

impl Iterator for ChannelSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if let Some(sample) = self.current.next() {
            return Some(sample);
        }
        match self.rx.recv() {
            Ok(chunk) => {
                self.current = chunk.into_iter();
                self.current.next()
            }
            Err(_) => None,
        }
    }
}

impl Source for ChannelSource {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        1
    }
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

/// Holds the output device/sink alive for the duration of playback.
pub struct Player {
    _stream: OutputStream,
    sink: Sink,
}

impl Player {
    /// Open the default output device and start consuming chunks sent on `rx`.
    pub fn start(rx: Receiver<Vec<f32>>, sample_rate: u32) -> Result<Self> {
        let (stream, handle) =
            OutputStream::try_default().context("opening default audio output device")?;
        let sink = Sink::try_new(&handle).context("creating audio sink")?;
        sink.append(ChannelSource {
            rx,
            current: Vec::new().into_iter(),
            sample_rate,
        });
        Ok(Self {
            _stream: stream,
            sink,
        })
    }

    /// Block until all queued audio has finished playing.
    pub fn wait(&self) {
        self.sink.sleep_until_end();
    }
}
