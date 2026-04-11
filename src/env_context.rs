/// Environment Context Tracking — detect when commands run in the wrong cloud/k8s context.
///
/// This module tracks changes to shell environment contexts such as:
/// - `KUBECONFIG` / active kubectl context (via `kubectl config current-context`)
/// - `AWS_PROFILE` / AWS region
/// - `TF_WORKSPACE` (Terraform workspace)
/// - `DOCKER_CONTEXT`
///
/// After each command we snapshot the current context. When a destructive
/// command is about to run we check whether the context changed recently,
/// and if so we emit a `ContextWarning` that the safety layer can surface to
/// the user before execution.
///
/// # Design
///
/// - All I/O (reading kubectl current-context) is done eagerly at snapshot time
///   so the warning path itself is O(1).
/// - The history is a bounded ring of `(Instant, EnvSnapshot)` pairs.  We keep
///   at most `MAX_HISTORY` entries (default 64) so memory stays bounded even in
///   very long sessions.
/// - "Production" detection is config-driven: the user lists context name
///   substrings in `.shako.toml` under `[safety] production_contexts`.
use std::time::{Duration, Instant};

/// Maximum number of context snapshots to keep.
const MAX_HISTORY: usize = 64;

/// Default window: if context switched within this many seconds of a
/// potentially destructive command, we warn.
#[allow(dead_code)]
pub const DEFAULT_WARN_WINDOW_SECS: u64 = 300; // 5 minutes

// ─── Snapshot ────────────────────────────────────────────────────────────────

/// A point-in-time capture of all tracked environment contexts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvSnapshot {
    /// Active kubectl context name, or `None` if kubectl is absent / errors.
    pub kube_context: Option<String>,
    /// `AWS_PROFILE` env var value, or `None` if not set.
    pub aws_profile: Option<String>,
    /// `TF_WORKSPACE` env var value, or `None` if not set.
    pub tf_workspace: Option<String>,
    /// `DOCKER_CONTEXT` env var value, or `None` if not set.
    pub docker_context: Option<String>,
}

impl EnvSnapshot {
    /// Capture the current environment context.
    ///
    /// Reads env vars synchronously.  The kubectl current-context is resolved
    /// by reading the active kubeconfig file rather than spawning kubectl, to
    /// keep this path fast and free of side effects.
    pub fn capture() -> Self {
        let kube_context = resolve_kube_context();
        let aws_profile = std::env::var("AWS_PROFILE").ok();
        let tf_workspace = std::env::var("TF_WORKSPACE").ok();
        let docker_context = std::env::var("DOCKER_CONTEXT").ok();

        Self {
            kube_context,
            aws_profile,
            tf_workspace,
            docker_context,
        }
    }

    /// Returns a human-readable label for the most "significant" context.
    /// Priority: kube > aws > tf > docker > None.
    pub fn label(&self) -> Option<String> {
        if let Some(ref ctx) = self.kube_context {
            return Some(format!("kubectl:{ctx}"));
        }
        if let Some(ref profile) = self.aws_profile {
            return Some(format!("aws:{profile}"));
        }
        if let Some(ref ws) = self.tf_workspace {
            return Some(format!("tf:{ws}"));
        }
        if let Some(ref ctx) = self.docker_context {
            return Some(format!("docker:{ctx}"));
        }
        None
    }
}

/// Read the active kubectl context from the kubeconfig file without spawning kubectl.
///
/// Parses `KUBECONFIG` (or `~/.kube/config`) for `current-context`.  Falls
/// back to `None` on any error — we never block shell startup.
fn resolve_kube_context() -> Option<String> {
    let kubeconfig = std::env::var("KUBECONFIG")
        .ok()
        .and_then(|k| {
            // KUBECONFIG can be colon-separated; use the first file.
            k.split(':').next().map(str::to_string)
        })
        .map(std::path::PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".kube").join("config")))?;

    let contents = std::fs::read_to_string(kubeconfig).ok()?;

    // Fast YAML scan — we only need `current-context: <name>`.
    for line in contents.lines() {
        let trimmed = line.trim();
        if let Some(ctx) = trimmed.strip_prefix("current-context:") {
            let ctx = ctx.trim().trim_matches('"').trim_matches('\'');
            if !ctx.is_empty() {
                return Some(ctx.to_string());
            }
        }
    }
    None
}

// ─── Switch event ────────────────────────────────────────────────────────────

/// A recorded context-switch event: the moment the environment changed and
/// the snapshot *after* the change.
#[derive(Debug, Clone)]
pub struct ContextSwitch {
    pub when: Instant,
    pub from: EnvSnapshot,
    pub to: EnvSnapshot,
}

// ─── ContextTracker ──────────────────────────────────────────────────────────

/// Tracks context changes across shell commands.
///
/// Call `post_command()` after every command to update the snapshot history.
/// Call `check_command()` before executing to get a potential warning.
pub struct ContextTracker {
    /// All recorded switch events, oldest-first, bounded by `MAX_HISTORY`.
    history: Vec<ContextSwitch>,
    /// The most recent snapshot (updated after each command).
    current: EnvSnapshot,
    /// How long after a context switch we keep warning.
    warn_window: Duration,
    /// Context name substrings considered "production" (from config).
    production_patterns: Vec<String>,
}

impl ContextTracker {
    /// Create a new tracker, capturing the initial environment state.
    pub fn new(warn_window_secs: u64, production_patterns: Vec<String>) -> Self {
        Self {
            history: Vec::new(),
            current: EnvSnapshot::capture(),
            warn_window: Duration::from_secs(warn_window_secs),
            production_patterns,
        }
    }

    /// Record the environment state after a command ran.
    ///
    /// If any context changed, a `ContextSwitch` is appended to history.
    pub fn post_command(&mut self) {
        let fresh = EnvSnapshot::capture();
        if fresh != self.current {
            let switch = ContextSwitch {
                when: Instant::now(),
                from: self.current.clone(),
                to: fresh.clone(),
            };
            if self.history.len() >= MAX_HISTORY {
                self.history.remove(0);
            }
            self.history.push(switch);
            self.current = fresh;
        }
    }

    /// Check whether a command is about to run in a recently-switched context
    /// that looks dangerous.
    ///
    /// Returns `Some(ContextWarning)` if the command should be flagged, or
    /// `None` if everything looks fine.
    pub fn check_command<'a>(
        &'a self,
        command: &str,
        production_patterns: &[String],
    ) -> Option<ContextWarning<'a>> {
        let effective_patterns = if !production_patterns.is_empty() {
            production_patterns
        } else {
            &self.production_patterns
        };

        is_context_mismatch_risk(command, &self.history, self.warn_window, effective_patterns)
    }

    /// True if the current context matches a production pattern.
    pub fn is_currently_production(&self, patterns: &[String]) -> bool {
        is_production_context(&self.current, patterns)
    }

    /// Return the most recent context switch, if any.
    #[allow(dead_code)]
    pub fn last_switch(&self) -> Option<&ContextSwitch> {
        self.history.last()
    }

    /// Access the current snapshot.
    #[allow(dead_code)]
    pub fn current(&self) -> &EnvSnapshot {
        &self.current
    }
}

// ─── Warning ─────────────────────────────────────────────────────────────────

/// A warning emitted when a command is about to run in a recently-changed context.
#[derive(Debug)]
pub struct ContextWarning<'a> {
    /// How long ago the context switched.
    pub switched_ago: Duration,
    /// How long the session was in the previous context before switching.
    #[allow(dead_code)]
    pub prior_duration: Option<Duration>,
    /// The switch that triggered the warning.
    pub switch: &'a ContextSwitch,
    /// The specific concern (kubectl, aws, terraform, …).
    pub kind: ContextWarnKind,
}

/// What kind of context mismatch is being flagged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextWarnKind {
    Kubernetes,
    AwsProfile,
    TerraformWorkspace,
    DockerContext,
}

impl ContextWarnKind {
    pub fn label(&self) -> &str {
        match self {
            Self::Kubernetes => "kubectl",
            Self::AwsProfile => "aws",
            Self::TerraformWorkspace => "terraform",
            Self::DockerContext => "docker",
        }
    }
}

// ─── Detection logic ─────────────────────────────────────────────────────────

/// Core mismatch-risk detector.  Returns a warning if:
/// 1. There is a recent context switch within `warn_window`.
/// 2. The current context (post-switch) matches a production pattern.
/// 3. The command is one of the known destructive verbs for that context type.
fn is_context_mismatch_risk<'a>(
    command: &str,
    history: &'a [ContextSwitch],
    warn_window: Duration,
    production_patterns: &[String],
) -> Option<ContextWarning<'a>> {
    let now = Instant::now();

    // Walk history newest-first so we find the most recent relevant switch.
    for (idx, switch) in history.iter().enumerate().rev() {
        let elapsed = now.duration_since(switch.when);
        if elapsed > warn_window {
            break; // History is ordered; older entries won't qualify.
        }

        // Determine the prior session duration (time spent before this switch).
        let prior_duration = if idx > 0 {
            Some(switch.when.duration_since(history[idx - 1].when))
        } else {
            None // Can't tell from history alone — omit.
        };

        // Check each context dimension.
        if let Some(kind) = dangerous_kubectl(command, switch, production_patterns) {
            return Some(ContextWarning {
                switched_ago: elapsed,
                prior_duration,
                switch,
                kind,
            });
        }
        if let Some(kind) = dangerous_aws(command, switch, production_patterns) {
            return Some(ContextWarning {
                switched_ago: elapsed,
                prior_duration,
                switch,
                kind,
            });
        }
        if let Some(kind) = dangerous_terraform(command, switch, production_patterns) {
            return Some(ContextWarning {
                switched_ago: elapsed,
                prior_duration,
                switch,
                kind,
            });
        }
        if let Some(kind) = dangerous_docker(command, switch, production_patterns) {
            return Some(ContextWarning {
                switched_ago: elapsed,
                prior_duration,
                switch,
                kind,
            });
        }
    }
    None
}

/// True when a context name (kube, aws, tf, docker) matches any production pattern.
fn is_production_context(snap: &EnvSnapshot, patterns: &[String]) -> bool {
    if patterns.is_empty() {
        // No patterns configured — use sensible built-in defaults.
        let defaults = ["prod", "production", "live", "prd"];
        let matches = |ctx: &str| defaults.iter().any(|p| ctx.to_lowercase().contains(p));
        snap.kube_context.as_deref().is_some_and(matches)
            || snap.aws_profile.as_deref().is_some_and(matches)
            || snap.tf_workspace.as_deref().is_some_and(matches)
            || snap.docker_context.as_deref().is_some_and(matches)
    } else {
        let matches = |ctx: &str| {
            patterns
                .iter()
                .any(|p| ctx.to_lowercase().contains(&p.to_lowercase()))
        };
        snap.kube_context.as_deref().is_some_and(matches)
            || snap.aws_profile.as_deref().is_some_and(matches)
            || snap.tf_workspace.as_deref().is_some_and(matches)
            || snap.docker_context.as_deref().is_some_and(matches)
    }
}

/// Detect dangerous kubectl commands after a kube context switch to prod.
fn dangerous_kubectl(
    command: &str,
    switch: &ContextSwitch,
    production_patterns: &[String],
) -> Option<ContextWarnKind> {
    // Only warn if the new context is production.
    let new_snap = &switch.to;
    if !context_is_production(new_snap.kube_context.as_deref(), production_patterns) {
        return None;
    }
    // Old context must be different.
    if switch.from.kube_context == new_snap.kube_context {
        return None;
    }

    let lower = command.to_lowercase();
    let tokens: Vec<&str> = lower.split_whitespace().collect();
    if tokens.first() != Some(&"kubectl") {
        return None;
    }

    // Flag destructive kubectl verbs.
    const DESTRUCTIVE_VERBS: &[&str] = &[
        "delete",
        "scale",
        "rollout",
        "drain",
        "cordon",
        "taint",
        "patch",
        "apply",
        "replace",
        "exec",
        "port-forward",
    ];
    if tokens.len() >= 2 && DESTRUCTIVE_VERBS.contains(&tokens[1]) {
        return Some(ContextWarnKind::Kubernetes);
    }
    None
}

/// Detect dangerous AWS CLI commands after an AWS profile switch to prod.
fn dangerous_aws(
    command: &str,
    switch: &ContextSwitch,
    production_patterns: &[String],
) -> Option<ContextWarnKind> {
    let new_snap = &switch.to;
    if !context_is_production(new_snap.aws_profile.as_deref(), production_patterns) {
        return None;
    }
    if switch.from.aws_profile == new_snap.aws_profile {
        return None;
    }

    let lower = command.to_lowercase();
    let tokens: Vec<&str> = lower.split_whitespace().collect();
    if tokens.first() != Some(&"aws") {
        return None;
    }

    // Flag delete/terminate/remove operations.
    // AWS CLI subcommands often include hyphens (e.g. "terminate-instances"),
    // so we check whether any non-flag token *contains* a destructive verb.
    const DESTRUCTIVE_VERBS: &[&str] = &[
        "delete",
        "terminate",
        "remove",
        "destroy",
        "deregister",
        "detach",
        "stop",
        "disable",
        "revoke",
    ];
    if tokens
        .iter()
        .skip(1)
        .filter(|t| !t.starts_with('-'))
        .any(|t| DESTRUCTIVE_VERBS.iter().any(|v| t.contains(v)))
    {
        return Some(ContextWarnKind::AwsProfile);
    }
    None
}

/// Detect `terraform apply/destroy` after a workspace switch to prod.
fn dangerous_terraform(
    command: &str,
    switch: &ContextSwitch,
    production_patterns: &[String],
) -> Option<ContextWarnKind> {
    let new_snap = &switch.to;
    if !context_is_production(new_snap.tf_workspace.as_deref(), production_patterns) {
        return None;
    }
    if switch.from.tf_workspace == new_snap.tf_workspace {
        return None;
    }

    let lower = command.to_lowercase();
    let tokens: Vec<&str> = lower.split_whitespace().collect();
    let cmd = tokens.first().copied().unwrap_or("");
    if cmd != "terraform" && cmd != "tofu" {
        return None;
    }

    const DESTRUCTIVE: &[&str] = &["apply", "destroy", "force-unlock", "taint", "untaint"];
    if tokens.len() >= 2 && DESTRUCTIVE.contains(&tokens[1]) {
        return Some(ContextWarnKind::TerraformWorkspace);
    }
    None
}

/// Detect dangerous docker commands after a context switch to prod.
fn dangerous_docker(
    command: &str,
    switch: &ContextSwitch,
    production_patterns: &[String],
) -> Option<ContextWarnKind> {
    let new_snap = &switch.to;
    if !context_is_production(new_snap.docker_context.as_deref(), production_patterns) {
        return None;
    }
    if switch.from.docker_context == new_snap.docker_context {
        return None;
    }

    let lower = command.to_lowercase();
    let tokens: Vec<&str> = lower.split_whitespace().collect();
    if tokens.first() != Some(&"docker") {
        return None;
    }

    const DESTRUCTIVE: &[&str] = &["rm", "rmi", "stop", "kill", "prune", "down", "swarm"];
    if tokens.len() >= 2 && DESTRUCTIVE.contains(&tokens[1]) {
        return Some(ContextWarnKind::DockerContext);
    }
    None
}

/// True when `ctx_name` matches any of the production patterns.
///
/// Falls back to built-in defaults (`prod`, `production`, `live`, `prd`) when
/// the pattern list is empty.
fn context_is_production(ctx_name: Option<&str>, patterns: &[String]) -> bool {
    let name = match ctx_name {
        Some(n) => n,
        None => return false,
    };
    let lower = name.to_lowercase();

    if patterns.is_empty() {
        return ["prod", "production", "live", "prd"]
            .iter()
            .any(|p| lower.contains(p));
    }

    patterns.iter().any(|p| lower.contains(&p.to_lowercase()))
}

// ─── Formatting helper ────────────────────────────────────────────────────────

/// Format a `Duration` as a human-readable string like "14s", "3m", "2h".
pub fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_switch(from_kube: Option<&str>, to_kube: Option<&str>) -> ContextSwitch {
        let from = EnvSnapshot {
            kube_context: from_kube.map(str::to_string),
            aws_profile: None,
            tf_workspace: None,
            docker_context: None,
        };
        let to = EnvSnapshot {
            kube_context: to_kube.map(str::to_string),
            aws_profile: None,
            tf_workspace: None,
            docker_context: None,
        };
        ContextSwitch {
            when: Instant::now(),
            from,
            to,
        }
    }

    fn make_aws_switch(from: Option<&str>, to: Option<&str>) -> ContextSwitch {
        ContextSwitch {
            when: Instant::now(),
            from: EnvSnapshot {
                kube_context: None,
                aws_profile: from.map(str::to_string),
                tf_workspace: None,
                docker_context: None,
            },
            to: EnvSnapshot {
                kube_context: None,
                aws_profile: to.map(str::to_string),
                tf_workspace: None,
                docker_context: None,
            },
        }
    }

    fn make_tf_switch(from: Option<&str>, to: Option<&str>) -> ContextSwitch {
        ContextSwitch {
            when: Instant::now(),
            from: EnvSnapshot {
                kube_context: None,
                aws_profile: None,
                tf_workspace: from.map(str::to_string),
                docker_context: None,
            },
            to: EnvSnapshot {
                kube_context: None,
                aws_profile: None,
                tf_workspace: to.map(str::to_string),
                docker_context: None,
            },
        }
    }

    #[test]
    fn kubectl_delete_in_prod_after_switch_warns() {
        let switch = make_switch(Some("staging"), Some("prod"));
        let history = vec![switch];
        let warn_window = Duration::from_secs(300);
        let patterns = vec!["prod".to_string()];
        let result = is_context_mismatch_risk(
            "kubectl delete deployment api-gateway",
            &history,
            warn_window,
            &patterns,
        );
        assert!(result.is_some());
        assert_eq!(result.unwrap().kind, ContextWarnKind::Kubernetes);
    }

    #[test]
    fn kubectl_get_does_not_warn() {
        let switch = make_switch(Some("staging"), Some("prod"));
        let history = vec![switch];
        let warn_window = Duration::from_secs(300);
        let patterns = vec!["prod".to_string()];
        let result = is_context_mismatch_risk("kubectl get pods", &history, warn_window, &patterns);
        assert!(result.is_none());
    }

    #[test]
    fn no_context_switch_no_warn() {
        // Switch from prod to prod (no change in kube context) — should not warn.
        let switch = make_switch(Some("prod"), Some("prod"));
        let history = vec![switch];
        let warn_window = Duration::from_secs(300);
        let patterns = vec!["prod".to_string()];
        let result =
            is_context_mismatch_risk("kubectl delete pod foo", &history, warn_window, &patterns);
        assert!(result.is_none());
    }

    #[test]
    fn staging_target_no_warn() {
        let switch = make_switch(Some("dev"), Some("staging"));
        let history = vec![switch];
        let warn_window = Duration::from_secs(300);
        let patterns = vec!["prod".to_string()];
        let result =
            is_context_mismatch_risk("kubectl delete pod foo", &history, warn_window, &patterns);
        assert!(result.is_none());
    }

    #[test]
    fn aws_delete_in_prod_warns() {
        let switch = make_aws_switch(Some("staging"), Some("production"));
        let history = vec![switch];
        let warn_window = Duration::from_secs(300);
        let patterns = vec!["production".to_string()];
        let result = is_context_mismatch_risk(
            "aws ec2 terminate-instances --instance-ids i-abc",
            &history,
            warn_window,
            &patterns,
        );
        assert!(result.is_some());
        assert_eq!(result.unwrap().kind, ContextWarnKind::AwsProfile);
    }

    #[test]
    fn terraform_apply_in_prod_warns() {
        let switch = make_tf_switch(Some("dev"), Some("prod"));
        let history = vec![switch];
        let warn_window = Duration::from_secs(300);
        let patterns = vec!["prod".to_string()];
        let result = is_context_mismatch_risk("terraform apply", &history, warn_window, &patterns);
        assert!(result.is_some());
        assert_eq!(result.unwrap().kind, ContextWarnKind::TerraformWorkspace);
    }

    #[test]
    fn format_duration_seconds() {
        assert_eq!(format_duration(Duration::from_secs(14)), "14s");
    }

    #[test]
    fn format_duration_minutes() {
        assert_eq!(format_duration(Duration::from_secs(125)), "2m");
    }

    #[test]
    fn format_duration_hours() {
        assert_eq!(format_duration(Duration::from_secs(7265)), "2h 1m");
    }

    #[test]
    fn default_production_patterns_work() {
        let switch = make_switch(Some("staging"), Some("production-us-east-1"));
        let history = vec![switch];
        let warn_window = Duration::from_secs(300);
        // Empty patterns → use built-in defaults (includes "production").
        let result = is_context_mismatch_risk("kubectl delete pod foo", &history, warn_window, &[]);
        assert!(result.is_some());
    }
}
