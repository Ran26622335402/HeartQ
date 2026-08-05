//! OpenSquilla-compatible tier identifiers (`c0`–`c3`).

use serde::{Deserialize, Serialize};
use std::fmt;

/// Model capability / cost tier (OpenSquilla `c0`–`c3`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    C0,
    C1,
    C2,
    C3,
}

impl Tier {
    pub const ALL: [Tier; 4] = [Tier::C0, Tier::C1, Tier::C2, Tier::C3];

    pub fn as_str(self) -> &'static str {
        match self {
            Tier::C0 => "c0",
            Tier::C1 => "c1",
            Tier::C2 => "c2",
            Tier::C3 => "c3",
        }
    }

    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "c0" | "t0" | "r0" => Some(Tier::C0),
            "c1" | "t1" | "r1" => Some(Tier::C1),
            "c2" | "t2" | "r2" => Some(Tier::C2),
            "c3" | "t3" | "r3" => Some(Tier::C3),
            _ => None,
        }
    }

    pub fn upgrade(self, steps: u8) -> Self {
        let idx = (self as u8).saturating_add(steps).min(3);
        Self::ALL[idx as usize]
    }

    pub fn max(self, other: Self) -> Self {
        if self >= other { self } else { other }
    }
}

impl fmt::Display for Tier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Default for Tier {
    fn default() -> Self {
        Tier::C1
    }
}

/// ML / heuristic route class before tier mapping (R0–R3 ↔ c0–c3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RouteClass {
    R0 = 0,
    R1 = 1,
    R2 = 2,
    R3 = 3,
}

impl RouteClass {
    pub fn to_tier(self) -> Tier {
        match self {
            RouteClass::R0 => Tier::C0,
            RouteClass::R1 => Tier::C1,
            RouteClass::R2 => Tier::C2,
            RouteClass::R3 => Tier::C3,
        }
    }

    pub fn from_tier(tier: Tier) -> Self {
        match tier {
            Tier::C0 => RouteClass::R0,
            Tier::C1 => RouteClass::R1,
            Tier::C2 => RouteClass::R2,
            Tier::C3 => RouteClass::R3,
        }
    }

    pub fn upgrade(self, steps: u8) -> Self {
        Self::from_tier(self.to_tier().upgrade(steps))
    }
}
