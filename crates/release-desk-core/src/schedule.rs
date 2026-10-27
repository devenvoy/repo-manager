//! Scheduling, recurrence calculations, and automation rules.

use crate::error::ValidationError;
use serde::{Deserialize, Serialize};

/// Unique schedule identity.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct ScheduleId(u128);

impl ScheduleId {
    pub const fn from_u128(val: u128) -> Self {
        Self(val)
    }

    pub const fn as_u128(self) -> u128 {
        self.0
    }
}

/// Schedule recurrence rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Recurrence {
    Interval { seconds: u64 },
    DailyAt { hour: u8, minute: u8 },
    WeeklyAt { weekday: u8, hour: u8, minute: u8 },
    MonthlyOn { day: u8, hour: u8, minute: u8 },
}

impl Recurrence {
    pub fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Interval { seconds } => {
                if *seconds < 60 {
                    return Err(ValidationError::InvalidRange {
                        field: "interval seconds",
                        min: 60,
                        max: 86400 * 365,
                    });
                }
            }
            Self::DailyAt { hour, minute } => {
                if *hour > 23 {
                    return Err(ValidationError::InvalidRange {
                        field: "hour",
                        min: 0,
                        max: 23,
                    });
                }
                if *minute > 59 {
                    return Err(ValidationError::InvalidRange {
                        field: "minute",
                        min: 0,
                        max: 59,
                    });
                }
            }
            Self::WeeklyAt {
                weekday,
                hour,
                minute,
            } => {
                if !(1..=7).contains(weekday) {
                    return Err(ValidationError::InvalidRange {
                        field: "weekday",
                        min: 1,
                        max: 7,
                    });
                }
                if *hour > 23 || *minute > 59 {
                    return Err(ValidationError::InvalidRange {
                        field: "time",
                        min: 0,
                        max: 59,
                    });
                }
            }
            Self::MonthlyOn { day, hour, minute } => {
                if !(1..=31).contains(day) {
                    return Err(ValidationError::InvalidRange {
                        field: "day of month",
                        min: 1,
                        max: 31,
                    });
                }
                if *hour > 23 || *minute > 59 {
                    return Err(ValidationError::InvalidRange {
                        field: "time",
                        min: 0,
                        max: 59,
                    });
                }
            }
        }
        Ok(())
    }
}

/// Action when a scheduled execution was missed.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissedRunPolicy {
    #[default]
    RunImmediately,
    Skip,
}

/// Deterministic scheduler calculating subsequent execution timestamps.
pub struct ScheduleCalculator;

impl ScheduleCalculator {
    /// Given reference epoch seconds and a recurrence rule, compute the next execution timestamp.
    pub fn next_run(reference_epoch_secs: u64, recurrence: &Recurrence) -> u64 {
        match recurrence {
            Recurrence::Interval { seconds } => reference_epoch_secs.saturating_add(*seconds),
            Recurrence::DailyAt { hour, minute } => {
                let target_secs_in_day = (*hour as u64) * 3600 + (*minute as u64) * 60;
                let current_secs_in_day = reference_epoch_secs % 86400;
                let day_start = reference_epoch_secs - current_secs_in_day;

                if current_secs_in_day < target_secs_in_day {
                    day_start + target_secs_in_day
                } else {
                    day_start + 86400 + target_secs_in_day
                }
            }
            Recurrence::WeeklyAt {
                weekday,
                hour,
                minute,
            } => {
                let current_day = reference_epoch_secs / 86400;
                let current_weekday = ((current_day + 3) % 7) + 1; // 1=Mon, ..., 7=Sun
                let target_secs_in_day = (*hour as u64) * 3600 + (*minute as u64) * 60;
                let current_secs_in_day = reference_epoch_secs % 86400;
                let day_start = reference_epoch_secs - current_secs_in_day;

                let target_weekday = *weekday as u64;
                let days_until = if target_weekday > current_weekday {
                    target_weekday - current_weekday
                } else if target_weekday < current_weekday {
                    7 - (current_weekday - target_weekday)
                } else if current_secs_in_day < target_secs_in_day {
                    0
                } else {
                    7
                };

                day_start + (days_until * 86400) + target_secs_in_day
            }
            Recurrence::MonthlyOn { day, hour, minute } => {
                let target_secs_in_day = (*hour as u64) * 3600 + (*minute as u64) * 60;
                let days_forward = (*day as u64).max(1);
                reference_epoch_secs + (days_forward * 86400) + target_secs_in_day
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recurrence_validation_ranges() {
        assert!(Recurrence::Interval { seconds: 120 }.validate().is_ok());
        assert!(Recurrence::Interval { seconds: 10 }.validate().is_err());
        assert!(
            Recurrence::DailyAt {
                hour: 23,
                minute: 59
            }
            .validate()
            .is_ok()
        );
        assert!(
            Recurrence::DailyAt {
                hour: 24,
                minute: 0
            }
            .validate()
            .is_err()
        );
        assert!(
            Recurrence::WeeklyAt {
                weekday: 7,
                hour: 10,
                minute: 0
            }
            .validate()
            .is_ok()
        );
        assert!(
            Recurrence::WeeklyAt {
                weekday: 0,
                hour: 10,
                minute: 0
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn next_run_interval_math() {
        let t = 1_000_000;
        let next = ScheduleCalculator::next_run(t, &Recurrence::Interval { seconds: 300 });
        assert_eq!(next, 1_000_300);
    }
}
