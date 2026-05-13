use std::collections::BTreeMap;

use crate::error::{app_error, AppResult};
use crate::tools::types::{Tool, ToolInput, ToolOutput};
use crate::util::json::{parse_json_value, JsonValue};

pub struct RequestUserInputTool;

impl Tool for RequestUserInputTool {
    fn name(&self) -> &str {
        "request_user_input"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let request = UserInputRequest::from_tool_input(&input)?;
        let mut summary = String::new();
        summary.push_str("meta.user_input_required=true\n");
        summary.push_str(&format!("meta.questions={}\n", request.questions.len()));
        summary.push_str(
            "Ask the user these question(s) and wait for their answer before continuing.\n",
        );
        for (index, question) in request.questions.iter().enumerate() {
            summary.push_str(&format!(
                "\n{}. [{}] {}\n",
                index + 1,
                one_line(&question.id),
                one_line(&question.header)
            ));
            summary.push_str(&format!("question: {}\n", one_line(&question.question)));
            summary.push_str("options:\n");
            for option in &question.options {
                summary.push_str(&format!(
                    "- {}: {}\n",
                    one_line(&option.label),
                    one_line(&option.description)
                ));
            }
        }
        Ok(ToolOutput { summary })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UserInputRequest {
    questions: Vec<UserInputQuestion>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UserInputQuestion {
    header: String,
    id: String,
    question: String,
    options: Vec<UserInputOption>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UserInputOption {
    label: String,
    description: String,
}

impl UserInputRequest {
    fn from_tool_input(input: &ToolInput) -> AppResult<Self> {
        let raw = input
            .get("questions")
            .ok_or_else(|| app_error("request_user_input requires `questions`"))?;
        let value = parse_json_value(raw.trim())
            .map_err(|error| app_error(format!("Invalid request_user_input payload: {error}")))?;
        let JsonValue::Array(items) = value else {
            return Err(app_error(
                "request_user_input.questions must be a JSON array",
            ));
        };
        Self::from_question_values(&items)
    }

    fn from_question_values(items: &[JsonValue]) -> AppResult<Self> {
        if items.is_empty() {
            return Err(app_error("request_user_input.questions must be non-empty"));
        }
        if items.len() > 3 {
            return Err(app_error(
                "request_user_input.questions must contain 1 to 3 items",
            ));
        }

        let mut questions = Vec::with_capacity(items.len());
        for item in items {
            let JsonValue::Object(question) = item else {
                return Err(app_error(
                    "request_user_input.questions items must be objects",
                ));
            };
            questions.push(UserInputQuestion::from_object(question)?);
        }
        Ok(Self { questions })
    }
}

impl UserInputQuestion {
    fn from_object(map: &BTreeMap<String, JsonValue>) -> AppResult<Self> {
        let header = required_nonempty_string(
            map,
            "header",
            "request_user_input.questions.header cannot be empty",
        )?;
        let id =
            required_nonempty_string(map, "id", "request_user_input.questions.id cannot be empty")?;
        let question = required_nonempty_string(
            map,
            "question",
            "request_user_input.questions.question cannot be empty",
        )?;
        let options_value = map.get("options").ok_or_else(|| {
            app_error("request_user_input.questions.options must contain 2 or 3 items")
        })?;
        let JsonValue::Array(option_values) = options_value else {
            return Err(app_error(
                "request_user_input.questions.options must be a JSON array",
            ));
        };
        if option_values.len() < 2 || option_values.len() > 3 {
            return Err(app_error(
                "request_user_input.questions.options must contain 2 or 3 items",
            ));
        }

        let mut options = Vec::with_capacity(option_values.len());
        for value in option_values {
            let JsonValue::Object(option) = value else {
                return Err(app_error("request_user_input options must be objects"));
            };
            options.push(UserInputOption::from_object(option)?);
        }
        Ok(Self {
            header,
            id,
            question,
            options,
        })
    }
}

impl UserInputOption {
    fn from_object(map: &BTreeMap<String, JsonValue>) -> AppResult<Self> {
        let label = required_nonempty_string(
            map,
            "label",
            "request_user_input option label cannot be empty",
        )?;
        let description = required_nonempty_string(
            map,
            "description",
            "request_user_input option description cannot be empty",
        )?;
        Ok(Self { label, description })
    }
}

fn required_nonempty_string(
    map: &BTreeMap<String, JsonValue>,
    key: &str,
    empty_message: &str,
) -> AppResult<String> {
    let value = map
        .get(key)
        .ok_or_else(|| app_error(empty_message.to_string()))?;
    let JsonValue::String(value) = value else {
        return Err(app_error(empty_message.to_string()));
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(app_error(empty_message.to_string()));
    }
    Ok(trimmed.to_string())
}

fn one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_questions() -> String {
        r#"[
            {
                "header": "Mode",
                "id": "mode",
                "question": "Which execution mode should be used?",
                "options": [
                    {"label": "Plan", "description": "Draft a plan first."},
                    {"label": "Apply", "description": "Implement directly."}
                ]
            }
        ]"#
        .to_string()
    }

    #[test]
    fn request_user_input_renders_required_prompt_summary() {
        let output = RequestUserInputTool
            .execute(ToolInput::new().with_arg("questions", valid_questions()))
            .unwrap();

        assert!(output.summary.contains("meta.user_input_required=true"));
        assert!(output.summary.contains("meta.questions=1"));
        assert!(output.summary.contains("[mode] Mode"));
        assert!(output.summary.contains("- Plan: Draft a plan first."));
        assert!(output.summary.contains("- Apply: Implement directly."));
    }

    #[test]
    fn request_user_input_rejects_too_many_questions() {
        let questions = format!(
            "[{0},{0},{0},{0}]",
            r#"{"header":"Q","id":"q","question":"Pick?","options":[{"label":"A","description":"A"},{"label":"B","description":"B"}]}"#
        );
        let error = RequestUserInputTool
            .execute(ToolInput::new().with_arg("questions", questions))
            .unwrap_err();

        assert!(error
            .to_string()
            .contains("request_user_input.questions must contain 1 to 3 items"));
    }

    #[test]
    fn request_user_input_rejects_invalid_option_count() {
        let questions = r#"[{
            "header":"Mode",
            "id":"mode",
            "question":"Which mode?",
            "options":[{"label":"Only","description":"Not enough."}]
        }]"#;
        let error = RequestUserInputTool
            .execute(ToolInput::new().with_arg("questions", questions))
            .unwrap_err();

        assert!(error
            .to_string()
            .contains("request_user_input.questions.options must contain 2 or 3 items"));
    }

    #[test]
    fn request_user_input_rejects_empty_required_fields() {
        let questions = r#"[{
            "header":"Mode",
            "id":" ",
            "question":"Which mode?",
            "options":[
                {"label":"A","description":"A"},
                {"label":"B","description":"B"}
            ]
        }]"#;
        let error = RequestUserInputTool
            .execute(ToolInput::new().with_arg("questions", questions))
            .unwrap_err();

        assert!(error
            .to_string()
            .contains("request_user_input.questions.id cannot be empty"));
    }
}
