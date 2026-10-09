use serde::{Deserialize, Serialize};

/// A user-defined request run against the transcript, e.g. "suggest a reply".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Action {
    pub name: String,
    pub prompt: String,
    pub format: String,
    /// Overrides the default model for this action.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// A shell command run once an answer is done, told which session it was for.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub hook: String,
    /// The hook runs on its own after every answer; otherwise only when the user sends it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hook_auto: bool,
}
