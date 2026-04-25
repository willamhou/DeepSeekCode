use crate::skills::registry::SkillRegistry;
use crate::skills::schema::SkillSpec;

pub fn resolve_skill<'a>(registry: &'a SkillRegistry, name: Option<&str>) -> Option<&'a SkillSpec> {
    let name = name?;
    registry.all().iter().find(|skill| skill.name == name)
}

