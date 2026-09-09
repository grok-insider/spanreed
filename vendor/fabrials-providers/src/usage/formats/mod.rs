// Adapted from Tokscale 0d621cae343af9b057fb4a38c84d089524ac4378.
// Copyright (c) 2025 Junho Yeo. MIT; see TOKSCALE-LICENSE.
//! Format-specific readers behind Fabrials adapters; no Tokscale runtime, pricing fetcher or CLI.
#![allow(dead_code)]
pub mod bucket_tz;
pub mod cc_mirror;
pub mod paths;
pub mod pricing;
pub mod provider_identity;
pub mod sessions;
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TokenBreakdown {
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_write: i64,
    pub reasoning: i64,
}

impl TokenBreakdown {
    /// Add every token bucket from `other`, saturating each field independently.
    ///
    /// Use this for whole-breakdown aggregation so adding a new bucket cannot
    /// silently leave one hand-written accumulation site incomplete.
    pub fn add_assign_saturating(&mut self, other: &Self) {
        self.input = self.input.saturating_add(other.input);
        self.output = self.output.saturating_add(other.output);
        self.cache_read = self.cache_read.saturating_add(other.cache_read);
        self.cache_write = self.cache_write.saturating_add(other.cache_write);
        self.reasoning = self.reasoning.saturating_add(other.reasoning);
    }

    pub fn total(&self) -> i64 {
        // saturating so clamped (i64::MAX) buckets from a corrupt source can't
        // overflow the sum.
        self.input
            .saturating_add(self.output)
            .saturating_add(self.cache_read)
            .saturating_add(self.cache_write)
            .saturating_add(self.reasoning)
    }
}

impl std::ops::AddAssign<&TokenBreakdown> for TokenBreakdown {
    fn add_assign(&mut self, other: &TokenBreakdown) {
        self.add_assign_saturating(other);
    }
}

pub mod fs_atomic;
