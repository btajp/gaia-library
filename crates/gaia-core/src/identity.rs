//! クライアント識別。仕様書 §7.1。
use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Human,
    Agent,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::Agent => "agent",
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Role {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "human" => Ok(Self::Human),
            "agent" => Ok(Self::Agent),
            other => Err(format!("unknown role `{other}` (expected human|agent)")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientIdentity {
    pub name: String,
    pub role: Role,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_scope: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_round_trips_lowercase() {
        assert_eq!("agent".parse::<Role>().unwrap(), Role::Agent);
        assert_eq!(serde_json::to_value(Role::Human).unwrap(), "human");
        assert!("admin".parse::<Role>().is_err());
    }

    #[test]
    fn client_identity_omits_missing_default_scope() {
        let c = ClientIdentity {
            name: "x".into(),
            role: Role::Agent,
            default_scope: None,
        };
        assert_eq!(
            serde_json::to_value(&c).unwrap(),
            serde_json::json!({"name": "x", "role": "agent"})
        );
    }
}
