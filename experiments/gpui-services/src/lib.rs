//! Bounded, nonblocking selected-text handoff. No speech or persistent history.
use std::sync::mpsc::{SyncSender, TrySendError};

pub const MAX_SELECTION_BYTES: usize = 64 * 1024;

#[derive(Debug, PartialEq)]
pub struct Selection(pub String);

#[derive(Debug, PartialEq)]
pub enum Rejection {
    Empty,
    TooLarge,
    InvalidText,
    Busy,
    Closed,
}

pub fn enqueue(sender: &SyncSender<Selection>, bytes: &[u8]) -> Result<(), Rejection> {
    if bytes.len() > MAX_SELECTION_BYTES {
        return Err(Rejection::TooLarge);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| Rejection::InvalidText)?;
    if text.trim().is_empty() {
        return Err(Rejection::Empty);
    }
    if text.contains('\0') {
        return Err(Rejection::InvalidText);
    }
    sender
        .try_send(Selection(text.to_owned()))
        .map_err(|error| match error {
            TrySendError::Full(_) => Rejection::Busy,
            TrySendError::Disconnected(_) => Rejection::Closed,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::sync_channel;

    #[test]
    fn preserves_unicode_and_whitespace_without_history() {
        let (tx, rx) = sync_channel(1);
        enqueue(&tx, "  Grüße — hello\n".as_bytes()).unwrap();
        assert_eq!(rx.recv().unwrap(), Selection("  Grüße — hello\n".into()));
    }

    #[test]
    fn rejects_empty_invalid_and_oversized_inputs() {
        let (tx, rx) = sync_channel(1);
        for (bytes, expected) in [
            (&b" \n"[..], Rejection::Empty),
            (&b"\xff"[..], Rejection::InvalidText),
            (&b"hello\0there"[..], Rejection::InvalidText),
            (
                &vec![b'x'; MAX_SELECTION_BYTES + 1][..],
                Rejection::TooLarge,
            ),
        ] {
            assert_eq!(enqueue(&tx, bytes), Err(expected));
        }
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn busy_and_closed_fail_instead_of_waiting_or_replacing() {
        let (tx, rx) = sync_channel(1);
        enqueue(&tx, b"first").unwrap();
        assert_eq!(enqueue(&tx, b"second"), Err(Rejection::Busy));
        assert_eq!(rx.recv().unwrap(), Selection("first".into()));
        drop(rx);
        assert_eq!(enqueue(&tx, b"third"), Err(Rejection::Closed));
    }

    #[test]
    fn accepts_exact_byte_limit() {
        let (tx, rx) = sync_channel(1);
        enqueue(&tx, &vec![b'x'; MAX_SELECTION_BYTES]).unwrap();
        assert_eq!(rx.recv().unwrap().0.len(), MAX_SELECTION_BYTES);
    }
}
