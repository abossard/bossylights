//! What each open device read about itself, by the kind it was opened for: the Health
//! reply's [`Readings::after_handoff`] counts it. Off the tick: the opening workers note,
//! the replies read.

use crate::Open;
use engine_core::OutputKind;
use engine_core::health::AfterHandoff;
use outputs::Reading;
use std::ops::RangeInclusive;
use std::sync::{Arc, PoisonError};

/// Each open device's [`Reading`], by its kind. A device that reads nothing, or is gone,
/// has none here.
#[derive(Clone, Default)]
#[expect(
    clippy::disallowed_types,
    reason = "the opening workers and the replies lock it, never the tick or an output thread"
)]
pub struct Readings(Arc<std::sync::Mutex<Vec<(OutputKind, Reading)>>>);

impl Readings {
    /// `open`, noting what each device it opens reads about itself.
    pub(crate) fn noting(mut open: Open) -> (Self, Open) {
        let readings = Self::default();
        let noted = readings.clone();
        let noting: Open = Box::new(move |kind| {
            let device = open(kind)?;
            noted.note(kind, device.reading());
            Ok(device)
        });
        (readings, noting)
    }

    fn note(&self, kind: OutputKind, reading: Reading) {
        let mut readings = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        // The newest device of a kind wins; a kind is one adapter, so this stays small.
        readings.retain(|(noted, _)| *noted != kind);
        if reading.is_open() {
            readings.push((kind, reading));
        }
    }

    /// What follows the handoff for an output of `kind` over its `patched` channels
    /// ([`outputs::after_handoff`]), with what the device opened for that kind read.
    pub fn after_handoff(
        &self,
        kind: Option<OutputKind>,
        patched: Option<RangeInclusive<u32>>,
    ) -> AfterHandoff {
        let read = kind.and_then(|kind| {
            let readings = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            readings
                .iter()
                .find(|(noted, _)| *noted == kind)
                .and_then(|(_, reading)| reading.settings())
        });
        outputs::after_handoff(kind, patched, read)
    }
}
