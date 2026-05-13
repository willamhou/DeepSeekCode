use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::types::NetworkConfig;
use crate::error::AppResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkDecision {
    Allow,
    Deny,
    Prompt,
}

impl NetworkDecision {
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "deny" | "block" => Self::Deny,
            "prompt" | "ask" => Self::Prompt,
            _ => Self::Allow,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
            Self::Prompt => "prompt",
        }
    }
}

pub fn decide(config: &NetworkConfig, host: &str) -> NetworkDecision {
    let normalized = normalize_host(host);
    if normalized.is_empty() {
        return NetworkDecision::parse(&config.default);
    }
    if config
        .deny
        .iter()
        .any(|entry| host_matches(entry, &normalized))
    {
        return NetworkDecision::Deny;
    }
    if config
        .allow
        .iter()
        .any(|entry| host_matches(entry, &normalized))
    {
        return NetworkDecision::Allow;
    }
    NetworkDecision::parse(&config.default)
}

pub fn audit_decision(config: &NetworkConfig, host: &str, tool: &str, decision: NetworkDecision) {
    if !config.audit {
        return;
    }
    let _ = append_audit_line(&config.audit_path(), host, tool, decision);
}

pub fn append_audit_line(
    path: &Path,
    host: &str,
    tool: &str,
    decision: NetworkDecision,
) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(
        file,
        "{} network {} {} {}",
        unix_timestamp(),
        sanitize_audit_field(host),
        sanitize_audit_field(tool),
        decision.as_str()
    )?;
    Ok(())
}

pub fn normalize_host(host: &str) -> String {
    let trimmed = host
        .trim()
        .trim_matches(|ch| ch == '[' || ch == ']')
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if let Some(rest) = trimmed.strip_prefix("*.") {
        format!(".{rest}")
    } else {
        trimmed
    }
}

fn host_matches(entry: &str, normalized_host: &str) -> bool {
    let entry = normalize_host(entry);
    if let Some(suffix) = entry.strip_prefix('.') {
        if suffix.is_empty() {
            return false;
        }
        return normalized_host.ends_with(&format!(".{suffix}"))
            && normalized_host.len() > suffix.len() + 1;
    }
    entry == normalized_host
}

fn sanitize_audit_field(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_control() || ch.is_whitespace() {
                '_'
            } else {
                ch
            }
        })
        .collect()
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(default: &str, allow: &[&str], deny: &[&str]) -> NetworkConfig {
        NetworkConfig {
            default: default.to_string(),
            allow: allow.iter().map(|value| value.to_string()).collect(),
            deny: deny.iter().map(|value| value.to_string()).collect(),
            ..NetworkConfig::default()
        }
    }

    #[test]
    fn network_policy_uses_default_when_no_rule_matches() {
        assert_eq!(
            decide(&config("deny", &[], &[]), "example.com"),
            NetworkDecision::Deny
        );
        assert_eq!(
            decide(&config("prompt", &[], &[]), "example.com"),
            NetworkDecision::Prompt
        );
    }

    #[test]
    fn network_policy_uses_deny_wins_precedence() {
        let policy = config(
            "allow",
            &[".example.com", "api.example.com"],
            &["bad.example.com"],
        );
        assert_eq!(decide(&policy, "api.example.com"), NetworkDecision::Allow);
        assert_eq!(decide(&policy, "bad.example.com"), NetworkDecision::Deny);
    }

    #[test]
    fn network_policy_subdomain_rule_does_not_match_apex() {
        let policy = config("deny", &[".example.com"], &[]);
        assert_eq!(decide(&policy, "api.example.com"), NetworkDecision::Allow);
        assert_eq!(decide(&policy, "example.com"), NetworkDecision::Deny);
    }

    #[test]
    fn network_audit_appends_plaintext_decision_line() {
        let root = unique_tmp("network-audit");
        let path = root.join("audit.log");

        append_audit_line(
            &path,
            "api.example.com",
            "web_fetch",
            NetworkDecision::Allow,
        )
        .unwrap();
        append_audit_line(
            &path,
            "blocked.example.com",
            "web fetch",
            NetworkDecision::Deny,
        )
        .unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains(" network api.example.com web_fetch allow\n"));
        assert!(content.contains(" network blocked.example.com web_fetch deny\n"));

        let _ = std::fs::remove_dir_all(root);
    }

    fn unique_tmp(label: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("deepseek-{label}-{}-{nanos}", std::process::id()))
    }
}
