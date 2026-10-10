//! The `eco` skill, teaching agents the CLI. It is embedded in the binary and
//! published to agent harnesses; a file there that eco does not own is never
//! overwritten.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

const SKILL: &str = include_str!("../skills/eco/SKILL.md");

/// Where each harness looks for skills, under the home directory.
const HARNESSES: [(&str, &str); 2] = [("agents", ".agents"), ("claude-code", ".claude")];

fn path(home: &Path, harness: &str) -> Result<PathBuf> {
    let Some((_, directory)) = HARNESSES.iter().find(|(name, _)| *name == harness) else {
        bail!("unknown harness {harness:?}; use agents or claude-code");
    };
    Ok(home.join(directory).join("skills/eco/SKILL.md"))
}

/// Whether `text` is eco's skill: its front matter names it and eco owns it.
fn owned(text: &str) -> bool {
    let Some((header, _)) = text
        .strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---\n"))
    else {
        return false;
    };
    let lines: Vec<&str> = header.lines().map(str::trim).collect();
    lines.contains(&"name: eco") && lines.contains(&"owner: eco")
}

/// Publish the skill to `harness` under `home`; what happened, for the user.
pub fn install(home: &Path, harness: &str) -> Result<String> {
    let path = path(home, harness)?;
    let outcome = match fs::read_to_string(&path) {
        Ok(existing) if existing == SKILL => "is up to date",
        Ok(existing) if owned(&existing) => "updated",
        Ok(_) => bail!(
            "{} is not eco's skill; it was left as it is",
            path.display()
        ),
        Err(error) if error.kind() == ErrorKind::NotFound => "installed",
        Err(error) => return Err(error.into()),
    };
    if outcome != "is up to date" {
        fs::create_dir_all(path.parent().expect("a skill path has a directory"))?;
        fs::write(&path, SKILL)?;
    }
    Ok(format!("skill {} {outcome}", path.display()))
}

/// The harnesses that already have eco's skill, so setup keeps them current.
pub fn installed(home: &Path) -> Vec<&'static str> {
    HARNESSES
        .iter()
        .map(|(name, _)| *name)
        .filter(|name| {
            path(home, name)
                .ok()
                .and_then(|path| fs::read_to_string(path).ok())
                .is_some_and(|text| owned(&text))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_skill_is_owned_by_eco() {
        assert!(owned(SKILL));
        assert!(!owned(
            "---\nname: omakiten\nmetadata:\n  owner: omakiten\n---\n"
        ));
        assert!(!owned("# eco sessions, by hand\n"));
    }

    #[test]
    fn installs_updates_and_leaves_foreign_skills_alone() {
        let home = tempfile::tempdir().unwrap();
        let home = home.path();
        let claude = home.join(".claude/skills/eco/SKILL.md");
        assert!(installed(home).is_empty());

        assert!(install(home, "claude-code").unwrap().ends_with("installed"));
        assert_eq!(fs::read_to_string(&claude).unwrap(), SKILL);
        assert!(
            install(home, "claude-code")
                .unwrap()
                .ends_with("is up to date")
        );
        assert_eq!(installed(home), ["claude-code"]);

        fs::write(&claude, SKILL.replace("# eco", "# eco, older")).unwrap();
        assert!(install(home, "claude-code").unwrap().ends_with("updated"));
        assert_eq!(fs::read_to_string(&claude).unwrap(), SKILL);

        let agents = home.join(".agents/skills/eco/SKILL.md");
        fs::create_dir_all(agents.parent().unwrap()).unwrap();
        fs::write(&agents, "my own sessions about eco\n").unwrap();
        let refused = install(home, "agents").unwrap_err().to_string();
        assert!(refused.contains("not eco's skill"), "{refused}");
        assert_eq!(
            fs::read_to_string(&agents).unwrap(),
            "my own sessions about eco\n"
        );
        assert!(install(home, "cursor").is_err());
    }

    #[test]
    fn a_skill_it_cannot_read_is_an_error() {
        let home = tempfile::tempdir().unwrap();
        let home = home.path();
        let claude = home.join(".claude/skills/eco/SKILL.md");
        fs::create_dir_all(&claude).unwrap();
        assert!(install(home, "claude-code").is_err());
        assert!(claude.is_dir(), "left as it is");
    }
}
