//! Request-scoped MLX sampling keys; never changes a global accelerator RNG.
#![cfg_attr(not(spqx_mlx), allow(dead_code))]
use std::cell::Cell;
thread_local! { static STATE: Cell<Option<u64>> = const { Cell::new(None) }; }

pub(crate) struct SeedGuard(Option<u64>);
impl SeedGuard {
    pub(crate) fn new(seed: Option<u64>) -> Self {
        Self(STATE.replace(seed))
    }
}
impl Drop for SeedGuard {
    fn drop(&mut self) {
        STATE.set(self.0);
    }
}

pub(crate) fn next_seed() -> u64 {
    STATE.with(|state| match state.get() {
        Some(seed) => {
            let next = seed.wrapping_add(0x9e3779b97f4a7c15);
            state.set(Some(next));
            // SplitMix64: deterministic independent keys within this request.
            let mut key = next;
            key = (key ^ (key >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
            key = (key ^ (key >> 27)).wrapping_mul(0x94d049bb133111eb);
            key ^ (key >> 31)
        }
        None => std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos() as u64)
            .unwrap_or(42),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn seeded_keys_repeat_and_nested_requests_restore_outer_state() {
        let sequence = || {
            let _guard = SeedGuard::new(Some(42));
            [next_seed(), next_seed(), next_seed()]
        };
        let expected = sequence();
        assert_eq!(sequence(), expected);
        let _outer = SeedGuard::new(Some(42));
        assert_eq!(next_seed(), expected[0]);
        {
            let _inner = SeedGuard::new(Some(9));
            assert_ne!(next_seed(), expected[1]);
        }
        assert_eq!(next_seed(), expected[1]);
        let separate = std::thread::spawn(sequence).join().unwrap();
        assert_eq!(separate, expected);
        assert_eq!(next_seed(), expected[2]);
    }
}
