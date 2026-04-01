use serde::Deserialize;

#[derive(Debug, Deserialize, Default)]
#[allow(dead_code)]
pub struct Input {
    pub model: Option<Model>,
    pub context_window: Option<ContextWindow>,
    pub rate_limits: Option<RateLimits>,
    pub workspace: Option<Workspace>,
    pub cost: Option<Cost>,
}

#[derive(Debug, Deserialize, Default)]
pub struct Model {
    pub display_name: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ContextWindow {
    pub used_percentage: Option<f64>,
    pub context_window_size: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
pub struct RateLimits {
    pub five_hour: Option<WindowLimit>,
    pub seven_day: Option<WindowLimit>,
}

#[derive(Debug, Deserialize, Default)]
pub struct WindowLimit {
    pub used_percentage: Option<f64>,
    pub resets_at: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
pub struct Workspace {
    pub project_dir: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[allow(dead_code)]
pub struct Cost {
    pub total_cost_usd: Option<f64>,
}

impl Input {
    /// Read stdin and return both the typed struct and the raw JSON value.
    pub fn from_stdin() -> (Self, serde_json::Value) {
        let mut buf = String::new();
        if std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf).is_err() {
            return (Self::default(), serde_json::Value::Null);
        }
        let raw: serde_json::Value = serde_json::from_str(&buf).unwrap_or_default();
        let typed: Self = serde_json::from_value(raw.clone()).unwrap_or_default();
        (typed, raw)
    }

    /// Model display name with redundant suffixes stripped and context size appended.
    pub fn model_label(&self) -> String {
        let name = self
            .model
            .as_ref()
            .and_then(|m| m.display_name.as_deref())
            .unwrap_or("?");

        // Strip trailing context size info like " (200K context)" since we show it separately
        let name = if let Some(idx) = name.rfind(" (") {
            let suffix = &name[idx..];
            if suffix.contains("context") || suffix.contains("Context") {
                &name[..idx]
            } else {
                name
            }
        } else {
            name
        };

        let ctx_size = self
            .context_window
            .as_ref()
            .and_then(|c| c.context_window_size);

        match ctx_size {
            Some(s) if s >= 1_000_000 => format!("{name} ({:.0}M)", s as f64 / 1_000_000.0),
            Some(s) if s >= 1_000 => format!("{name} ({:.0}K)", s as f64 / 1_000.0),
            Some(0) | None => name.to_string(),
            Some(s) => format!("{name} ({s})"),
        }
    }

    pub fn context_used_pct(&self) -> Option<f64> {
        self.context_window.as_ref().and_then(|c| c.used_percentage)
    }

    pub fn context_size_label(&self) -> String {
        let s = self
            .context_window
            .as_ref()
            .and_then(|c| c.context_window_size)
            .unwrap_or(0);
        if s >= 1_000_000 {
            format!("{:.0}M", s as f64 / 1_000_000.0)
        } else if s >= 1_000 {
            format!("{:.0}K", s as f64 / 1_000.0)
        } else {
            format!("{s}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_full_input() {
        let json = r#"{
            "model": {"display_name": "Claude Opus 4.6"},
            "context_window": {"used_percentage": 42, "context_window_size": 1000000},
            "rate_limits": {
                "five_hour": {"used_percentage": 39.2, "resets_at": 1775070000},
                "seven_day": {"used_percentage": 43.1, "resets_at": 1775500000}
            },
            "workspace": {"project_dir": "/home/user/project"},
            "cost": {"total_cost_usd": 1.23}
        }"#;
        let input: Input = serde_json::from_str(json).unwrap();
        assert_eq!(
            input.model.as_ref().unwrap().display_name.as_deref(),
            Some("Claude Opus 4.6")
        );
        assert!((input.rate_limits.as_ref().unwrap().five_hour.as_ref().unwrap().used_percentage.unwrap() - 39.2).abs() < 0.01);
    }

    #[test]
    fn parse_minimal_input() {
        let json = r#"{}"#;
        let input: Input = serde_json::from_str(json).unwrap();
        assert!(input.model.is_none());
        assert!(input.rate_limits.is_none());
    }

    #[test]
    fn model_label_strips_context_suffix() {
        let json = r#"{"model":{"display_name":"Claude Opus 4.6 (1M context)"},"context_window":{"context_window_size":1000000}}"#;
        let input: Input = serde_json::from_str(json).unwrap();
        assert_eq!(input.model_label(), "Claude Opus 4.6 (1M)");
    }

    #[test]
    fn model_label_appends_size() {
        let json = r#"{"model":{"display_name":"Claude Sonnet 4.5"},"context_window":{"context_window_size":200000}}"#;
        let input: Input = serde_json::from_str(json).unwrap();
        assert_eq!(input.model_label(), "Claude Sonnet 4.5 (200K)");
    }

    #[test]
    fn model_label_no_size_when_zero() {
        let json = r#"{"model":{"display_name":"Claude Haiku"},"context_window":{"context_window_size":0}}"#;
        let input: Input = serde_json::from_str(json).unwrap();
        assert_eq!(input.model_label(), "Claude Haiku");
    }
}
