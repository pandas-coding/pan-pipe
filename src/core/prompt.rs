#![allow(dead_code)]

use anyhow::Result;
use async_trait::async_trait;

pub struct PromptOption {
    pub value: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptResult {
    Value(String),
    Cancelled,
}

#[async_trait]
pub trait Prompter {
    async fn select(&mut self, message: &str, options: &[PromptOption]) -> Result<PromptResult>;
    fn log_info(&mut self, message: &str);
    fn is_cancelled(&self, result: &PromptResult) -> bool {
        matches!(result, PromptResult::Cancelled)
    }
}

pub struct InquirePrompter;

#[async_trait]
impl Prompter for InquirePrompter {
    async fn select(&mut self, message: &str, options: &[PromptOption]) -> Result<PromptResult> {
        let items: Vec<String> = options.iter().map(|o| o.label.clone()).collect();
        let ans = inquire::Select::new(message, items).prompt()?;
        let value = options
            .iter()
            .find(|o| o.label == ans)
            .map(|o| o.value.clone())
            .unwrap_or_default();
        Ok(PromptResult::Value(value))
    }

    fn log_info(&mut self, message: &str) {
        println!("{}", message);
    }
}

/// A prompter for tests that replays scripted responses.
pub struct ScriptPrompter {
    responses: Vec<String>,
    index: usize,
    pub logs: Vec<String>,
}

impl ScriptPrompter {
    pub fn new(responses: Vec<String>) -> Self {
        Self {
            responses,
            index: 0,
            logs: Vec::new(),
        }
    }
}

#[async_trait]
impl Prompter for ScriptPrompter {
    async fn select(&mut self, _message: &str, options: &[PromptOption]) -> Result<PromptResult> {
        if self.index >= self.responses.len() {
            return Ok(PromptResult::Cancelled);
        }
        let response = &self.responses[self.index];
        self.index += 1;
        if options.iter().any(|o| o.value == *response) {
            Ok(PromptResult::Value(response.clone()))
        } else {
            // Allow matching by label as fallback
            if let Some(opt) = options.iter().find(|o| o.label == *response) {
                Ok(PromptResult::Value(opt.value.clone()))
            } else {
                Ok(PromptResult::Cancelled)
            }
        }
    }

    fn log_info(&mut self, message: &str) {
        self.logs.push(message.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_script_prompter_replays_values() {
        let mut prompter = ScriptPrompter::new(vec!["overwrite".to_string()]);
        let options = vec![
            PromptOption {
                value: "overwrite".to_string(),
                label: "Overwrite".to_string(),
            },
            PromptOption {
                value: "skip".to_string(),
                label: "Skip".to_string(),
            },
        ];
        let result = prompter.select("choose", &options).await.unwrap();
        assert_eq!(result, PromptResult::Value("overwrite".to_string()));
    }

    #[tokio::test]
    async fn test_script_prompter_cancel_when_exhausted() {
        let mut prompter = ScriptPrompter::new(vec![]);
        let options = vec![PromptOption {
            value: "a".to_string(),
            label: "A".to_string(),
        }];
        let result = prompter.select("choose", &options).await.unwrap();
        assert_eq!(result, PromptResult::Cancelled);
    }

    #[test]
    fn test_script_prompter_logs() {
        let mut prompter = ScriptPrompter::new(vec![]);
        prompter.log_info("hello");
        assert_eq!(prompter.logs, vec!["hello"]);
    }
}
