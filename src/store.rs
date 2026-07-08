use alloc::vec::Vec;

use rust_decimal::Decimal;

/// One rate: `mantissa / 10^scale` currency units per £1. 16 bytes.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub mantissa: u64,
    pub code: [u8; 3],
    pub scale: u8,
}

impl Entry {
    pub fn decimal(&self) -> Decimal {
        Decimal::from_i128_with_scale(self.mantissa as i128, u32::from(self.scale))
    }
}

/// Index row for key-addressed series; the period's entries are
/// `arena[prev_end..end]`, sorted by code.
#[derive(Copy, Clone, Debug)]
pub(crate) struct PeriodIdx {
    pub key: i32,
    pub end: u32,
}

/// Index row for the weekly series: an inclusive day range (days since epoch).
#[derive(Copy, Clone, Debug)]
pub(crate) struct WeekIdx {
    pub start_day: i32,
    pub end_day: i32,
    pub end: u32,
}

/// Bundled data for one key-addressed series (empty when not bundled).
#[derive(Copy, Clone)]
pub(crate) struct StaticSeries {
    pub index: &'static [PeriodIdx],
    pub arena: &'static [Entry],
}

#[cfg(test)]
pub(crate) const EMPTY_SERIES: StaticSeries = StaticSeries {
    index: &[],
    arena: &[],
};

#[derive(Copy, Clone)]
pub(crate) struct StaticWeeks {
    pub index: &'static [WeekIdx],
    pub arena: &'static [Entry],
}

#[cfg(test)]
pub(crate) const EMPTY_WEEKS: StaticWeeks = StaticWeeks {
    index: &[],
    arena: &[],
};

/// A key-addressed series: bundled statics plus an overlay of fetched periods.
/// Overlay wins; a fetched period replaces the whole bundled table.
#[derive(Clone)]
pub(crate) struct Series {
    statics: StaticSeries,
    overlay: Vec<(i32, Vec<Entry>)>, // sorted by key
}

impl Series {
    pub fn new(statics: StaticSeries) -> Series {
        Series {
            statics,
            overlay: Vec::new(),
        }
    }

    /// Inserts or replaces a fetched period (entries must be sorted by code).
    #[cfg(any(test, feature = "http"))]
    pub fn set(&mut self, key: i32, entries: Vec<Entry>) {
        match self.overlay.binary_search_by_key(&key, |(k, _)| *k) {
            Ok(i) => self.overlay[i].1 = entries,
            Err(i) => self.overlay.insert(i, (key, entries)),
        }
    }

    pub fn table(&self, key: i32) -> Option<&[Entry]> {
        if let Ok(i) = self.overlay.binary_search_by_key(&key, |(k, _)| *k) {
            return Some(&self.overlay[i].1);
        }
        let i = self
            .statics
            .index
            .binary_search_by_key(&key, |p| p.key)
            .ok()?;
        Some(self.slice(i))
    }

    fn slice(&self, i: usize) -> &'static [Entry] {
        let start = if i == 0 {
            0
        } else {
            self.statics.index[i - 1].end as usize
        };
        let end = self.statics.index[i].end as usize;
        &self.statics.arena[start..end]
    }

    /// All period keys, ascending, overlay merged in.
    pub fn keys(&self) -> Vec<i32> {
        let mut keys: Vec<i32> = self.statics.index.iter().map(|p| p.key).collect();
        for (k, _) in &self.overlay {
            if let Err(i) = keys.binary_search(k) {
                keys.insert(i, *k);
            }
        }
        keys
    }

    pub fn first_last(&self) -> Option<(i32, i32)> {
        let static_ends = self.statics.index.first().zip(self.statics.index.last());
        let overlay_ends = self.overlay.first().zip(self.overlay.last());
        match (static_ends, overlay_ends) {
            (Some((sf, sl)), Some(((of, _), (ol, _)))) => Some((sf.key.min(*of), sl.key.max(*ol))),
            (Some((sf, sl)), None) => Some((sf.key, sl.key)),
            (None, Some(((of, _), (ol, _)))) => Some((*of, *ol)),
            (None, None) => None,
        }
    }

    /// Whether an overlay period replaces the static one with this key.
    fn shadowed(&self, key: i32) -> bool {
        self.overlay.binary_search_by_key(&key, |(k, _)| *k).is_ok()
    }

    /// Whether `code` appears in any reachable period of this series.
    pub fn knows(&self, code: [u8; 3]) -> bool {
        let in_statics = (0..self.statics.index.len()).any(|i| {
            !self.shadowed(self.statics.index[i].key) && lookup(self.slice(i), code).is_some()
        });
        in_statics || self.overlay.iter().any(|(_, t)| lookup(t, code).is_some())
    }

    /// Every distinct code in reachable periods, ascending.
    pub fn codes(&self) -> Vec<[u8; 3]> {
        let mut codes: Vec<[u8; 3]> = Vec::with_capacity(self.statics.arena.len());
        for i in 0..self.statics.index.len() {
            if !self.shadowed(self.statics.index[i].key) {
                codes.extend(self.slice(i).iter().map(|e| e.code));
            }
        }
        codes.extend(
            self.overlay
                .iter()
                .flat_map(|(_, t)| t.iter().map(|e| e.code)),
        );
        codes.sort_unstable();
        codes.dedup();
        codes
    }
}

/// The weekly series is dead (2014–2016): statics only, addressed by day ranges.
#[derive(Copy, Clone)]
pub(crate) struct Weeks {
    statics: StaticWeeks,
}

impl Weeks {
    pub fn new(statics: StaticWeeks) -> Weeks {
        Weeks { statics }
    }

    /// The week whose inclusive day range contains `day`.
    pub fn containing(&self, day: i32) -> Option<(WeekIdx, &'static [Entry])> {
        let idx = self.statics.index;
        let i = idx.partition_point(|w| w.start_day <= day).checked_sub(1)?;
        let week = idx[i];
        (day <= week.end_day).then(|| {
            let start = if i == 0 { 0 } else { idx[i - 1].end as usize };
            (week, &self.statics.arena[start..week.end as usize])
        })
    }

    pub fn index(&self) -> &'static [WeekIdx] {
        self.statics.index
    }

    pub fn arena(&self) -> &'static [Entry] {
        self.statics.arena
    }

    pub fn knows(&self, code: [u8; 3]) -> bool {
        self.statics.arena.iter().any(|e| e.code == code)
    }
}

/// Binary search a code within one period's sorted table.
pub(crate) fn lookup(table: &[Entry], code: [u8; 3]) -> Option<Entry> {
    table
        .binary_search_by_key(&code, |e| e.code)
        .ok()
        .map(|i| table[i])
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use alloc::vec;

    fn entry(code: &[u8; 3], mantissa: u64) -> Entry {
        Entry {
            mantissa,
            code: *code,
            scale: 4,
        }
    }

    #[test]
    fn entry_decimal_conversion() {
        let e = Entry {
            mantissa: 13541,
            code: *b"USD",
            scale: 4,
        };
        assert_eq!(e.decimal().to_string(), "1.3541");
        let max = Entry {
            mantissa: 64136965388,
            code: *b"VES",
            scale: 4,
        };
        assert_eq!(max.decimal().to_string(), "6413696.5388");
    }

    #[test]
    fn overlay_replaces_and_extends_statics() {
        let mut series = Series::new(EMPTY_SERIES);
        assert!(series.table(1).is_none());
        assert_eq!(series.first_last(), None);

        series.set(2, vec![entry(b"EUR", 11547), entry(b"USD", 13541)]);
        series.set(1, vec![entry(b"USD", 13000)]);
        assert_eq!(series.keys(), vec![1, 2]);
        assert_eq!(series.first_last(), Some((1, 2)));
        assert_eq!(
            lookup(series.table(2).unwrap(), *b"USD").unwrap().mantissa,
            13541
        );

        // Whole-period replacement.
        series.set(2, vec![entry(b"USD", 14000)]);
        assert_eq!(series.table(2).unwrap().len(), 1);
        assert!(series.knows(*b"USD"));
        assert!(!series.knows(*b"EUR"));
        assert_eq!(series.codes(), vec![*b"USD"]);
    }

    #[test]
    fn overlay_shadows_static_codes() {
        static ARENA: [Entry; 2] = [
            Entry {
                mantissa: 13541,
                code: *b"USD",
                scale: 4,
            },
            Entry {
                mantissa: 100,
                code: *b"XYZ",
                scale: 2,
            },
        ];
        static INDEX: [PeriodIdx; 1] = [PeriodIdx { key: 1, end: 2 }];
        let mut series = Series::new(StaticSeries {
            index: &INDEX,
            arena: &ARENA,
        });
        assert!(series.knows(*b"XYZ"));

        // The replacement table drops XYZ; it must vanish from the series.
        series.set(1, vec![entry(b"USD", 14000)]);
        assert!(!series.knows(*b"XYZ"));
        assert_eq!(series.codes(), vec![*b"USD"]);
    }

    #[test]
    fn weeks_containment_boundaries() {
        // Two adjacent weeks over a static arena.
        static ARENA: [Entry; 2] = [
            Entry {
                mantissa: 35418,
                code: *b"TRY",
                scale: 4,
            },
            Entry {
                mantissa: 109760,
                code: *b"ARS",
                scale: 4,
            },
        ];
        static INDEX: [WeekIdx; 2] = [
            WeekIdx {
                start_day: 100,
                end_day: 106,
                end: 1,
            },
            WeekIdx {
                start_day: 107,
                end_day: 113,
                end: 2,
            },
        ];
        let weeks = Weeks::new(StaticWeeks {
            index: &INDEX,
            arena: &ARENA,
        });

        assert!(weeks.containing(99).is_none());
        assert_eq!(weeks.containing(100).unwrap().1[0].code, *b"TRY");
        assert_eq!(weeks.containing(106).unwrap().1[0].code, *b"TRY");
        assert_eq!(weeks.containing(107).unwrap().1[0].code, *b"ARS");
        assert_eq!(weeks.containing(113).unwrap().1[0].code, *b"ARS");
        assert!(weeks.containing(114).is_none());
        assert!(weeks.knows(*b"ARS"));
        assert!(!weeks.knows(*b"USD"));
    }
}
