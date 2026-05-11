use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentSource {
    Project,
    User,
    File,
}

impl AgentSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::User => "user",
            Self::File => "file",
        }
    }

    fn rank(self) -> u8 {
        match self {
            Self::Project => 0,
            Self::User => 1,
            Self::File => 2,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AgentSpec {
    pub name: String,
    pub description: String,
    pub tools: Vec<String>,
    pub model: Option<String>,
    pub prompt: String,
    pub path: PathBuf,
    pub source: AgentSource,
}

#[derive(Debug, Clone)]
pub struct AgentLoadError {
    pub path: PathBuf,
    pub message: String,
}

pub type AgentLoadResult = Result<AgentSpec, AgentLoadError>;

pub fn project_agents_dir(config_dir: &str) -> PathBuf {
    PathBuf::from(config_dir).join("agents")
}

pub fn user_agents_dir() -> PathBuf {
    crate::skills::tilde::expand_tilde("~/.config/dscode/agents")
}

pub fn load_default_agents(config_dir: &str) -> Vec<AgentLoadResult> {
    load_agent_dirs(&[
        (project_agents_dir(config_dir), AgentSource::Project),
        (user_agents_dir(), AgentSource::User),
    ])
}

pub fn load_agent_dirs(dirs: &[(PathBuf, AgentSource)]) -> Vec<AgentLoadResult> {
    let mut results = Vec::new();
    for (dir, source) in dirs {
        if !dir.exists() {
            continue;
        }
        let mut paths = match fs::read_dir(dir) {
            Ok(entries) => entries
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("md"))
                .collect::<Vec<_>>(),
            Err(error) => {
                results.push(Err(AgentLoadError {
                    path: dir.clone(),
                    message: format!("failed to read agent directory: {error}"),
                }));
                continue;
            }
        };
        paths.sort();
        for path in paths {
            results.push(load_agent_file(&path, *source));
        }
    }
    results.sort_by(|left, right| agent_result_sort_key(left).cmp(&agent_result_sort_key(right)));
    results
}

pub fn load_agent_file(path: &Path, source: AgentSource) -> AgentLoadResult {
    let fallback_name = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("unknown");
    let content = fs::read_to_string(path).map_err(|error| AgentLoadError {
        path: path.to_path_buf(),
        message: format!("failed to read agent file: {error}"),
    })?;
    parse_agent_file(&content, fallback_name, path.to_path_buf(), source).map_err(|message| {
        AgentLoadError {
            path: path.to_path_buf(),
            message,
        }
    })
}

pub fn find_agent(config_dir: &str, name: &str) -> AgentLoadResult {
    let mut matching_errors = Vec::new();
    for result in load_default_agents(config_dir) {
        match result {
            Ok(agent) if agent.name == name => return Ok(agent),
            Ok(_) => {}
            Err(error) => matching_errors.push(error),
        }
    }

    if !matching_errors.is_empty() {
        let first = matching_errors.remove(0);
        return Err(first);
    }

    Err(AgentLoadError {
        path: project_agents_dir(config_dir),
        message: format!("agent `{name}` not found"),
    })
}

fn parse_agent_file(
    content: &str,
    fallback_name: &str,
    path: PathBuf,
    source: AgentSource,
) -> Result<AgentSpec, String> {
    let content = content.replace("\r\n", "\n");
    let (frontmatter, prompt) = split_frontmatter(&content)?;
    let mut agent = AgentSpec {
        name: fallback_name.to_string(),
        description: String::new(),
        tools: Vec::new(),
        model: None,
        prompt: prompt.trim().to_string(),
        path,
        source,
    };

    for raw_line in frontmatter.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            return Err(format!("invalid frontmatter line `{line}`"));
        };
        let key = key.trim();
        let value = value.trim();
        match key {
            "name" => agent.name = unquote(value),
            "description" => agent.description = unquote(value),
            "tools" => agent.tools = parse_string_list(value)?,
            "model" => {
                let value = unquote(value);
                if !value.trim().is_empty() {
                    agent.model = Some(value);
                }
            }
            other => return Err(format!("unsupported agent frontmatter key `{other}`")),
        }
    }

    validate_agent(&agent)?;
    Ok(agent)
}

fn split_frontmatter(content: &str) -> Result<(&str, &str), String> {
    let Some(rest) = content.strip_prefix("---\n") else {
        return Err("agent file must start with YAML frontmatter (`---`)".to_string());
    };
    let Some((frontmatter, body)) = rest.split_once("\n---\n") else {
        return Err("agent frontmatter must end with `---`".to_string());
    };
    Ok((frontmatter, body))
}

fn validate_agent(agent: &AgentSpec) -> Result<(), String> {
    if !is_valid_name(&agent.name) {
        return Err("agent `name` must use only letters, numbers, `_`, `-`, or `.`".to_string());
    }
    if agent.description.trim().is_empty() {
        return Err("agent `description` is required".to_string());
    }
    if agent.prompt.trim().is_empty() {
        return Err("agent prompt body is required".to_string());
    }

    let mut seen = BTreeSet::new();
    for tool in &agent.tools {
        if !is_valid_tool_name(tool) {
            return Err(format!("invalid tool name `{tool}`"));
        }
        if !seen.insert(tool) {
            return Err(format!("duplicate tool `{tool}`"));
        }
    }
    Ok(())
}

fn parse_string_list(value: &str) -> Result<Vec<String>, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed == "[]" {
        return Ok(Vec::new());
    }
    let inner = if let Some(inner) = trimmed.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
        inner
    } else {
        trimmed
    };
    inner
        .split(',')
        .map(|item| {
            let value = unquote(item.trim());
            if value.is_empty() {
                Err("tools list contains an empty item".to_string())
            } else {
                Ok(value)
            }
        })
        .collect()
}

fn unquote(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_string()
}

fn is_valid_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
}

fn is_valid_tool_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
}

fn agent_result_sort_key(result: &AgentLoadResult) -> (u8, String, String) {
    match result {
        Ok(agent) => (
            agent.source.rank(),
            agent.name.clone(),
            agent.path.display().to_string(),
        ),
        Err(error) => (
            AgentSource::File.rank(),
            String::new(),
            error.path.display().to_string(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "deepseek-agents-{name}-{}-{suffix}",
            std::process::id()
        ))
    }

    #[test]
    fn parses_agent_markdown_frontmatter() {
        let path = PathBuf::from(".dscode/agents/reviewer.md");
        let agent = parse_agent_file(
            "---\nname: reviewer\ndescription: Reviews code\ntools: [read_file, search_text]\nmodel: deepseek-coder\n---\nReview carefully.\n",
            "fallback",
            path.clone(),
            AgentSource::Project,
        )
        .unwrap();

        assert_eq!(agent.name, "reviewer");
        assert_eq!(agent.description, "Reviews code");
        assert_eq!(agent.tools, vec!["read_file", "search_text"]);
        assert_eq!(agent.model.as_deref(), Some("deepseek-coder"));
        assert_eq!(agent.prompt, "Review carefully.");
        assert_eq!(agent.path, path);
        assert_eq!(agent.source, AgentSource::Project);
    }

    #[test]
    fn rejects_missing_description() {
        let error = parse_agent_file(
            "---\nname: reviewer\n---\nReview carefully.\n",
            "fallback",
            PathBuf::from("reviewer.md"),
            AgentSource::Project,
        )
        .unwrap_err();

        assert!(error.contains("description"));
    }

    #[test]
    fn load_agent_dirs_reads_project_and_user_agents() {
        let root = temp_root("load");
        let project = root.join("project-agents");
        let user = root.join("user-agents");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&user).unwrap();
        std::fs::write(
            project.join("project.md"),
            "---\nname: project\ndescription: Project agent\n---\nProject prompt.\n",
        )
        .unwrap();
        std::fs::write(
            user.join("user.md"),
            "---\nname: user\ndescription: User agent\ntools: read_file\n---\nUser prompt.\n",
        )
        .unwrap();

        let agents = load_agent_dirs(&[
            (project.clone(), AgentSource::Project),
            (user.clone(), AgentSource::User),
        ])
        .into_iter()
        .map(Result::unwrap)
        .collect::<Vec<_>>();

        assert_eq!(agents.len(), 2);
        assert_eq!(agents[0].source, AgentSource::Project);
        assert_eq!(agents[0].name, "project");
        assert_eq!(agents[1].source, AgentSource::User);
        assert_eq!(agents[1].tools, vec!["read_file"]);

        let _ = std::fs::remove_dir_all(root);
    }
}
