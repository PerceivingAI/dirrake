use std::collections::BTreeSet;

pub(crate) const MAX_WARNING_SAMPLES: usize = 100;

#[derive(Debug, Clone, Default)]
pub(crate) struct WarningAccumulator {
    count: u64,
    samples: BTreeSet<String>,
}

impl WarningAccumulator {
    pub(crate) fn record(&mut self, warning: String) {
        self.count = self.count.saturating_add(1);
        self.retain_sample(warning);
    }

    pub(crate) fn merge(&mut self, other: Self) {
        self.count = self.count.saturating_add(other.count);
        for warning in other.samples {
            self.retain_sample(warning);
        }
    }

    pub(crate) fn count(&self) -> u64 {
        self.count
    }

    pub(crate) fn samples(&self) -> Vec<String> {
        self.samples.iter().cloned().collect()
    }

    fn retain_sample(&mut self, warning: String) {
        if self.samples.contains(&warning) {
            return;
        }
        if self.samples.len() < MAX_WARNING_SAMPLES {
            self.samples.insert(warning);
            return;
        }

        let should_replace = self
            .samples
            .last()
            .is_some_and(|largest| warning.as_str() < largest.as_str());
        if should_replace {
            self.samples.pop_last();
            self.samples.insert(warning);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accumulator(values: &[String]) -> WarningAccumulator {
        let mut warnings = WarningAccumulator::default();
        for value in values {
            warnings.record(value.clone());
        }
        warnings
    }

    #[test]
    fn duplicates_count_as_events_but_samples_remain_unique() {
        let mut warnings = WarningAccumulator::default();
        warnings.record("same warning".to_owned());
        warnings.record("same warning".to_owned());
        warnings.record("other warning".to_owned());

        assert_eq!(warnings.count(), 3);
        assert_eq!(
            warnings.samples(),
            vec!["other warning".to_owned(), "same warning".to_owned()]
        );
    }

    #[test]
    fn retains_the_lexicographically_smallest_hundred_samples() {
        let mut warnings = WarningAccumulator::default();
        for index in (0..150).rev() {
            warnings.record(format!("warning-{index:03}"));
        }

        let samples = warnings.samples();
        assert_eq!(warnings.count(), 150);
        assert_eq!(samples.len(), MAX_WARNING_SAMPLES);
        assert_eq!(samples.first().unwrap(), "warning-000");
        assert_eq!(samples.last().unwrap(), "warning-099");
    }

    #[test]
    fn merge_order_does_not_change_count_or_samples() {
        let values: Vec<_> = (0..240)
            .map(|index| format!("warning-{:03}", (index * 73) % 173))
            .collect();
        let expected = accumulator(&values);
        let expected_count = expected.count();
        let expected_samples = expected.samples();

        let chunks: Vec<Vec<String>> = values.chunks(17).map(|chunk| chunk.to_vec()).collect();
        for seed in 0..128_u64 {
            let mut order: Vec<usize> = (0..chunks.len()).collect();
            let mut state = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
            for index in (1..order.len()).rev() {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1);
                let swap_with = (state as usize) % (index + 1);
                order.swap(index, swap_with);
            }

            let mut merged = WarningAccumulator::default();
            for chunk_index in order {
                merged.merge(accumulator(&chunks[chunk_index]));
            }
            assert_eq!(merged.count(), expected_count, "seed={seed}");
            assert_eq!(merged.samples(), expected_samples, "seed={seed}");
        }
    }
}
