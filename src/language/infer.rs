use crate::language::profile::LanguageProfile;

pub fn default_test_command(profile: &LanguageProfile) -> Option<&str> {
    profile.test_commands.first().map(String::as_str)
}
