use anyhow::{Error, anyhow};
use std::path::Path;
use std::process::Command;

pub fn check_dependencies(name: String, dependencies: Vec<String>) -> Result<bool, Error> {
    let mut checker: Vec<bool> = Vec::new();
    for dependency in &dependencies {
        let current = dependency.split("@").collect::<Vec<&str>>();
        if let Some(tool) = current.first() {
            if let Some(version) = current.last() {
                checker.push(
                    Path::new(name.as_str())
                        .join("lib")
                        .join(tool.trim())
                        .join(version.trim())
                        .exists(),
                );
            }
        }
    }
    if checker.is_empty() {
        return Err(anyhow!("No dependencies found"));
    }
    if checker.contains(&false) {
        return Err(anyhow!("Missing dependencies"));
    }
    Ok(checker.contains(&false).eq(&false))
}

pub fn is_tool_available(tool: &str) -> Result<bool, Error> {
    Ok(Command::new("which")
        .arg(tool)
        .current_dir(".")
        .spawn()?
        .wait_with_output()
        .map(|o| o.status.success())
        .unwrap_or(false))
}
