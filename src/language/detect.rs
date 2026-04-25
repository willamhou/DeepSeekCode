use std::path::Path;

use crate::error::AppResult;
use crate::language::profile::LanguageProfile;

pub fn detect_profile(root: &str) -> AppResult<LanguageProfile> {
    let profile_name = if Path::new(root).join("Cargo.toml").exists() {
        "rust"
    } else if Path::new(root).join("package.json").exists() {
        "typescript"
    } else if Path::new(root).join("pyproject.toml").exists() || Path::new(root).join("requirements.txt").exists() {
        "python"
    } else if Path::new(root).join("go.mod").exists() {
        "go"
    } else if Path::new(root).join("pom.xml").exists() || Path::new(root).join("build.gradle").exists() {
        "java"
    } else {
        "generic"
    };

    Ok(profile_by_name(profile_name))
}

fn profile_by_name(name: &str) -> LanguageProfile {
    match name {
        "rust" => LanguageProfile {
            name: "rust".to_string(),
            file_priority: vec![
                "Cargo.toml".to_string(),
                "src/main.rs".to_string(),
                "src/lib.rs".to_string(),
                "tests/".to_string(),
            ],
            ignore_patterns: vec![".git/".to_string(), "target/".to_string()],
            test_commands: vec!["cargo test".to_string()],
            lint_commands: vec!["cargo clippy --all-targets --all-features".to_string()],
            build_commands: vec!["cargo build".to_string()],
            hints: vec!["Prefer minimal compile-safe changes.".to_string()],
        },
        "python" => LanguageProfile {
            name: "python".to_string(),
            file_priority: vec![
                "pyproject.toml".to_string(),
                "requirements.txt".to_string(),
                "src/".to_string(),
                "tests/".to_string(),
            ],
            ignore_patterns: vec![
                ".git/".to_string(),
                ".venv/".to_string(),
                "__pycache__/".to_string(),
            ],
            test_commands: vec!["pytest".to_string()],
            lint_commands: vec!["ruff check .".to_string()],
            build_commands: vec!["python -m build".to_string()],
            hints: vec!["Prefer minimal runtime-safe changes and rerun only relevant tests.".to_string()],
        },
        "typescript" => LanguageProfile {
            name: "typescript".to_string(),
            file_priority: vec![
                "package.json".to_string(),
                "tsconfig.json".to_string(),
                "src/".to_string(),
                "test/".to_string(),
            ],
            ignore_patterns: vec![
                ".git/".to_string(),
                "node_modules/".to_string(),
                "dist/".to_string(),
            ],
            test_commands: vec!["pnpm test".to_string(), "npm test".to_string()],
            lint_commands: vec!["pnpm lint".to_string(), "npm run lint".to_string()],
            build_commands: vec!["pnpm build".to_string(), "npm run build".to_string()],
            hints: vec!["Keep changes narrow and respect the package manager already in use.".to_string()],
        },
        "go" => LanguageProfile {
            name: "go".to_string(),
            file_priority: vec![
                "go.mod".to_string(),
                "cmd/".to_string(),
                "pkg/".to_string(),
                "internal/".to_string(),
            ],
            ignore_patterns: vec![".git/".to_string(), "vendor/".to_string()],
            test_commands: vec!["go test ./...".to_string()],
            lint_commands: vec!["go vet ./...".to_string()],
            build_commands: vec!["go build ./...".to_string()],
            hints: vec!["Preserve package boundaries and prefer direct fixes over abstractions.".to_string()],
        },
        "java" => LanguageProfile {
            name: "java".to_string(),
            file_priority: vec![
                "pom.xml".to_string(),
                "build.gradle".to_string(),
                "src/main/".to_string(),
                "src/test/".to_string(),
            ],
            ignore_patterns: vec![".git/".to_string(), "target/".to_string(), "build/".to_string()],
            test_commands: vec!["mvn test".to_string(), "gradle test".to_string()],
            lint_commands: vec![],
            build_commands: vec!["mvn package -DskipTests".to_string(), "gradle build -x test".to_string()],
            hints: vec!["Respect the existing build tool and minimize package-level churn.".to_string()],
        },
        _ => LanguageProfile {
            name: "generic".to_string(),
            file_priority: vec!["README.md".to_string(), "docs/".to_string(), "src/".to_string()],
            ignore_patterns: vec![
                ".git/".to_string(),
                "node_modules/".to_string(),
                "target/".to_string(),
                "dist/".to_string(),
            ],
            test_commands: vec![],
            lint_commands: vec![],
            build_commands: vec![],
            hints: vec!["Start with repository structure and the smallest relevant files.".to_string()],
        },
    }
}
