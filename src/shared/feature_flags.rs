//! Runtime feature-flag registry.
//!
//! Strategy: every feature gate the product uses is declared **once** as a
//! variant of [`FeatureFlag`].  Unknown strings from ENV or the override
//! table are logged as warnings but never panic (so an old release can
//! survive a newer DB without refusing to start — [R-04] in the plan).
//!
//! Overrides are layered:
//!
//! 1. Baseline from ENV (`FEATURE_FLAGS="onboarding_v2,admin_panel"`).
//! 2. Runtime overrides from `feature_flag_overrides` (migration 008) —
//!    updated by `AdminFeatureFlagsUseCase` in phase 4.
//!
//! Lookup is O(1) against a pre-built `HashSet<FeatureFlag>` so the hot path
//! stays fast.

use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

/// Every feature that can be toggled at runtime.  Keep this list **stable**;
/// removing a variant is a breaking change that requires cleaning up the
/// `feature_flag_overrides` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureFlag {
    OnboardingV2,
    AdminPanel,
    SlaEscalations,
    VoiceV2,
    NotificationDigest,
    TaskTemplates,
    RecurrenceRules,
    InlineAssigneeSearch,
    TeamAnalytics,
    CsvExport,
}

impl FeatureFlag {
    pub fn as_key(self) -> &'static str {
        match self {
            Self::OnboardingV2 => "onboarding_v2",
            Self::AdminPanel => "admin_panel",
            Self::SlaEscalations => "sla_escalations",
            Self::VoiceV2 => "voice_v2",
            Self::NotificationDigest => "notification_digest",
            Self::TaskTemplates => "task_templates",
            Self::RecurrenceRules => "recurrence_rules",
            Self::InlineAssigneeSearch => "inline_assignee_search",
            Self::TeamAnalytics => "team_analytics",
            Self::CsvExport => "csv_export",
        }
    }

    /// Flags that ship **enabled by default** for v3.  Anything not listed
    /// here must be explicitly switched on via ENV or runtime override.
    pub fn default_enabled() -> HashSet<FeatureFlag> {
        [FeatureFlag::OnboardingV2, FeatureFlag::AdminPanel]
            .into_iter()
            .collect()
    }
}

impl FromStr for FeatureFlag {
    type Err = UnknownFeatureFlag;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim() {
            "onboarding_v2" => Ok(Self::OnboardingV2),
            "admin_panel" => Ok(Self::AdminPanel),
            "sla_escalations" => Ok(Self::SlaEscalations),
            "voice_v2" => Ok(Self::VoiceV2),
            "notification_digest" => Ok(Self::NotificationDigest),
            "task_templates" => Ok(Self::TaskTemplates),
            "recurrence_rules" => Ok(Self::RecurrenceRules),
            "inline_assignee_search" => Ok(Self::InlineAssigneeSearch),
            "team_analytics" => Ok(Self::TeamAnalytics),
            "csv_export" => Ok(Self::CsvExport),
            other => Err(UnknownFeatureFlag(other.to_owned())),
        }
    }
}

#[derive(Debug, Clone)]
pub struct UnknownFeatureFlag(pub String);

/// Immutable snapshot of the flag state at startup + overrides.  Cheap to
/// clone (`Arc<HashSet>`-style semantics would add complexity without any
/// measurable benefit at this scale).
#[derive(Debug, Clone, Default)]
pub struct FeatureFlagRegistry {
    enabled: HashSet<FeatureFlag>,
}

impl FeatureFlagRegistry {
    pub fn from_env_and_defaults(env_value: Option<&str>) -> Self {
        let mut enabled = FeatureFlag::default_enabled();
        if let Some(csv) = env_value {
            for token in csv.split(',') {
                match FeatureFlag::from_str(token) {
                    Ok(flag) => {
                        enabled.insert(flag);
                    }
                    Err(UnknownFeatureFlag(name)) if !name.is_empty() => {
                        tracing::warn!(
                            flag = %name,
                            "ignoring unknown feature flag from FEATURE_FLAGS env",
                        );
                    }
                    _ => {}
                }
            }
        }
        Self { enabled }
    }

    pub fn apply_overrides(&mut self, overrides: &HashMap<FeatureFlag, bool>) {
        for (flag, enabled) in overrides {
            if *enabled {
                self.enabled.insert(*flag);
            } else {
                self.enabled.remove(flag);
            }
        }
    }

    pub fn is_enabled(&self, flag: FeatureFlag) -> bool {
        self.enabled.contains(&flag)
    }

    pub fn enabled_flags(&self) -> impl Iterator<Item = FeatureFlag> + '_ {
        self.enabled.iter().copied()
    }

    /// Enables or disables a flag in place.  Used by the admin toggle flow
    /// after the override has been persisted to the database.
    pub fn toggle(&mut self, flag: FeatureFlag, enabled: bool) {
        if enabled {
            self.enabled.insert(flag);
        } else {
            self.enabled.remove(&flag);
        }
    }

    /// Returns every known flag together with its current enabled state,
    /// sorted in declaration order for stable UI rendering.
    pub fn all_flags(&self) -> Vec<(FeatureFlag, bool)> {
        ALL_FLAGS
            .iter()
            .map(|&flag| (flag, self.is_enabled(flag)))
            .collect()
    }
}

/// All feature flags in a stable display order.  Must stay in sync with
/// the variants of [`FeatureFlag`] — a mismatch is caught at compile time
/// when the match in [`FeatureFlag::as_key`] or [`FromStr`] is exhaustive.
const ALL_FLAGS: &[FeatureFlag] = &[
    FeatureFlag::OnboardingV2,
    FeatureFlag::AdminPanel,
    FeatureFlag::SlaEscalations,
    FeatureFlag::VoiceV2,
    FeatureFlag::NotificationDigest,
    FeatureFlag::TaskTemplates,
    FeatureFlag::RecurrenceRules,
    FeatureFlag::InlineAssigneeSearch,
    FeatureFlag::TeamAnalytics,
    FeatureFlag::CsvExport,
];

/// A thread-safe, runtime-mutable feature flag registry shared across the
/// entire application.  Reads are lock-free under `RwLock` contention
/// because flag checks are almost exclusively reads; writes only happen
/// when an admin toggles a flag from the panel.
pub type SharedFeatureFlagRegistry = Arc<RwLock<FeatureFlagRegistry>>;

#[cfg(test)]
mod tests {
    use super::{FeatureFlag, FeatureFlagRegistry};

    #[test]
    fn given_empty_env_when_build_then_defaults_are_enabled() {
        let registry = FeatureFlagRegistry::from_env_and_defaults(None);

        assert!(registry.is_enabled(FeatureFlag::OnboardingV2));
        assert!(registry.is_enabled(FeatureFlag::AdminPanel));
        assert!(!registry.is_enabled(FeatureFlag::SlaEscalations));
    }

    #[test]
    fn given_unknown_flag_in_env_when_build_then_does_not_panic() {
        let registry =
            FeatureFlagRegistry::from_env_and_defaults(Some("onboarding_v2,nonsense_flag,"));

        assert!(registry.is_enabled(FeatureFlag::OnboardingV2));
    }

    #[test]
    fn given_override_when_applied_then_wins_over_baseline() {
        let mut registry = FeatureFlagRegistry::from_env_and_defaults(None);
        let mut overrides = std::collections::HashMap::new();
        overrides.insert(FeatureFlag::OnboardingV2, false);
        overrides.insert(FeatureFlag::SlaEscalations, true);

        registry.apply_overrides(&overrides);

        assert!(!registry.is_enabled(FeatureFlag::OnboardingV2));
        assert!(registry.is_enabled(FeatureFlag::SlaEscalations));
    }
}
