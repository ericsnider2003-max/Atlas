//! Pure capability-admission checks derived from the Umbrella/OpenShell lane.
//!
//! This module is intentionally not a sandbox or a runtime enforcement layer.
//! It gives Atlas one deterministic contract to use before a future provider,
//! MCP server, or external worker is admitted. The caller still owns the
//! durable receipt and the operating-system enforcement.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub binary: String,
    pub method: String,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityRule {
    pub binary: String,
    pub method: String,
    pub target_prefix: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityPolicy {
    pub revision: String,
    pub rules: Vec<CapabilityRule>,
}

impl CapabilityPolicy {
    pub fn new(revision: impl Into<String>, rules: Vec<CapabilityRule>) -> Self {
        Self {
            revision: revision.into(),
            rules,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Approval {
    policy_revision: String,
    accepted: bool,
}

impl Approval {
    pub fn accepted(policy_revision: impl Into<String>) -> Self {
        Self {
            policy_revision: policy_revision.into(),
            accepted: true,
        }
    }

    pub fn rejected(policy_revision: impl Into<String>) -> Self {
        Self {
            policy_revision: policy_revision.into(),
            accepted: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allowed {
        policy_revision: String,
        normalized_target: String,
    },
    Denied {
        reason: String,
    },
}

/// Normalize a path-like capability target without touching the filesystem.
///
/// A caller may use this for a path or a provider endpoint. We deliberately
/// reject traversal instead of trying to resolve it, because resolution would
/// make this pure admission check depend on the host filesystem.
fn normalize_target(raw: &str) -> Result<String, String> {
    if raw.is_empty() || raw.contains('\0') {
        return Err("capability target is empty or contains a NUL".into());
    }
    let raw = raw.replace('\\', "/");
    let rooted = raw.starts_with('/');
    let mut parts = Vec::new();
    for part in raw.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err("capability target escapes its declared root".into());
                }
            }
            part => parts.push(part),
        }
    }
    let joined = parts.join("/");
    if rooted {
        Ok(if joined.is_empty() {
            "/".into()
        } else {
            format!("/{joined}")
        })
    } else if joined.is_empty() {
        Err("capability target has no usable components".into())
    } else {
        Ok(joined)
    }
}

fn covers(prefix: &str, target: &str) -> bool {
    target == prefix
        || target
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Evaluate one request against an already selected policy and owner decision.
/// The returned revision is the receipt boundary a caller must persist.
pub fn evaluate(
    policy: &CapabilityPolicy,
    request: &Request,
    approval: Option<&Approval>,
) -> Decision {
    if policy.revision.is_empty() {
        return Decision::Denied {
            reason: "capability policy has no revision".into(),
        };
    }
    let Some(approval) = approval else {
        return Decision::Denied {
            reason: "capability request has no approval".into(),
        };
    };
    if !approval.accepted {
        return Decision::Denied {
            reason: "capability request was rejected".into(),
        };
    }
    if approval.policy_revision != policy.revision {
        return Decision::Denied {
            reason: "approval does not match the capability policy revision".into(),
        };
    }
    let normalized_target = match normalize_target(&request.target) {
        Ok(target) => target,
        Err(reason) => return Decision::Denied { reason },
    };
    let allowed = policy.rules.iter().any(|rule| {
        rule.binary == request.binary
            && rule.method == request.method
            && normalize_target(&rule.target_prefix)
                .is_ok_and(|prefix| covers(&prefix, &normalized_target))
    });
    if allowed {
        Decision::Allowed {
            policy_revision: policy.revision.clone(),
            normalized_target,
        }
    } else {
        Decision::Denied {
            reason: "no capability rule matches the binary, method, and target".into(),
        }
    }
}

/// Check that every child rule is covered by a parent rule.
pub fn is_subset(child: &CapabilityPolicy, parent: &CapabilityPolicy) -> Result<(), String> {
    for child_rule in &child.rules {
        let child_prefix = normalize_target(&child_rule.target_prefix)?;
        let covered = parent.rules.iter().any(|parent_rule| {
            parent_rule.binary == child_rule.binary
                && parent_rule.method == child_rule.method
                && normalize_target(&parent_rule.target_prefix)
                    .is_ok_and(|parent_prefix| covers(&parent_prefix, &child_prefix))
        });
        if !covered {
            return Err(format!(
                "child capability is wider than its parent for {} {} {}",
                child_rule.binary, child_rule.method, child_prefix
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        evaluate, is_subset, Approval, CapabilityPolicy, CapabilityRule, Decision, Request,
    };

    fn rule(binary: &str, method: &str, target_prefix: &str) -> CapabilityRule {
        CapabilityRule {
            binary: binary.into(),
            method: method.into(),
            target_prefix: target_prefix.into(),
        }
    }

    fn request(binary: &str, method: &str, target: &str) -> Request {
        Request {
            binary: binary.into(),
            method: method.into(),
            target: target.into(),
        }
    }

    #[test]
    fn empty_policy_denies() {
        let policy = CapabilityPolicy::new("rev-1", vec![]);
        assert!(matches!(
            evaluate(&policy, &request("worker", "read", "workspace/a"), None),
            Decision::Denied { .. }
        ));
    }

    #[test]
    fn normalized_path_is_allowed_only_under_the_declared_prefix() {
        let policy =
            CapabilityPolicy::new("rev-1", vec![rule("worker", "read", "C:/workspace/src")]);
        let result = evaluate(
            &policy,
            &request("worker", "read", r"C:\workspace\.\src\main.rs"),
            Some(&Approval::accepted("rev-1")),
        );
        assert!(matches!(result, Decision::Allowed { .. }));
    }

    #[test]
    fn outside_write_is_denied() {
        let policy =
            CapabilityPolicy::new("rev-1", vec![rule("worker", "write", "C:/workspace/src")]);
        let result = evaluate(
            &policy,
            &request("worker", "write", "C:/workspace/secrets.txt"),
            Some(&Approval::accepted("rev-1")),
        );
        assert!(matches!(result, Decision::Denied { .. }));
    }

    #[test]
    fn wrong_binary_or_method_is_denied() {
        let policy = CapabilityPolicy::new("rev-1", vec![rule("worker", "read", "C:/workspace")]);
        assert!(matches!(
            evaluate(
                &policy,
                &request("other", "read", "C:/workspace/a"),
                Some(&Approval::accepted("rev-1")),
            ),
            Decision::Denied { .. }
        ));
        assert!(matches!(
            evaluate(
                &policy,
                &request("worker", "write", "C:/workspace/a"),
                Some(&Approval::accepted("rev-1")),
            ),
            Decision::Denied { .. }
        ));
    }

    #[test]
    fn a_child_policy_cannot_be_wider_than_its_parent() {
        let parent =
            CapabilityPolicy::new("parent", vec![rule("worker", "write", "C:/workspace/src")]);
        let child = CapabilityPolicy::new("child", vec![rule("worker", "write", "C:/workspace")]);
        assert!(is_subset(&child, &parent).is_err());
    }

    #[test]
    fn rejected_approval_stays_denied() {
        let policy = CapabilityPolicy::new("rev-1", vec![rule("worker", "read", "C:/workspace")]);
        assert!(matches!(
            evaluate(
                &policy,
                &request("worker", "read", "C:/workspace/a"),
                Some(&Approval::rejected("rev-1")),
            ),
            Decision::Denied { .. }
        ));
    }

    #[test]
    fn approval_returns_the_policy_revision_as_the_receipt_boundary() {
        let policy = CapabilityPolicy::new(
            "network-rev-7",
            vec![rule("worker", "connect", "api.example")],
        );
        assert_eq!(
            evaluate(
                &policy,
                &request("worker", "connect", "api.example/v1"),
                Some(&Approval::accepted("network-rev-7")),
            ),
            Decision::Allowed {
                policy_revision: "network-rev-7".into(),
                normalized_target: "api.example/v1".into(),
            }
        );
    }
}
