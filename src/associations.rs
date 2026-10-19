//! Host-side application associations. Profile numbers are zero-based here.
//! EXE 00492150 compares the foreground program name against enabled lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    pub app: String,
    pub profile: u8,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rules {
    pub enabled: bool,
    pub fallback: u8,
    pub entries: Vec<Rule>,
}

impl Rules {
    pub fn validate(&self) -> Result<(), String> {
        if self.fallback >= 7 || self.entries.iter().any(|r| r.profile >= 7) {
            return Err("关联配置应为 1 至 7".into());
        }
        for (index, rule) in self.entries.iter().enumerate() {
            if rule.app.trim().is_empty() || rule.app.chars().any(char::is_control) {
                return Err("应用标识不能为空，也不能包含控制字符".into());
            }
            if self.entries[..index]
                .iter()
                .any(|r| r.app.trim().eq_ignore_ascii_case(rule.app.trim()))
            {
                return Err(format!("应用标识重复：{}", rule.app.trim()));
            }
        }
        Ok(())
    }

    pub fn target(&self, app: Option<&str>) -> Option<u8> {
        if !self.enabled {
            return None;
        }
        Some(
            app.and_then(|app| {
                self.entries
                    .iter()
                    .find(|r| r.app.trim().eq_ignore_ascii_case(app))
                    .map(|r| r.profile)
            })
            .unwrap_or(self.fallback),
        )
    }

    pub fn encode(&self) -> String {
        let mut text = format!("{}\t{}", u8::from(self.enabled), self.fallback);
        for rule in &self.entries {
            text.push_str(&format!("\n{}\t{}", rule.profile, rule.app));
        }
        text
    }

    pub fn decode(text: &str) -> Result<Self, String> {
        let mut lines = text.lines();
        let (enabled, fallback) = lines
            .next()
            .and_then(|s| s.split_once('\t'))
            .ok_or("关联设置头不完整")?;
        let mut rules = Self {
            enabled: match enabled {
                "0" => false,
                "1" => true,
                _ => return Err("关联设置开关无效".into()),
            },
            fallback: fallback.parse().map_err(|_| "默认关联配置无效")?,
            entries: Vec::new(),
        };
        for line in lines {
            let (profile, app) = line.split_once('\t').ok_or("关联设置记录不完整")?;
            rules.entries.push(Rule {
                app: app.into(),
                profile: profile.parse().map_err(|_| "关联配置无效")?,
            });
        }
        rules.validate()?;
        Ok(rules)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_fallback_disabled_and_saved_rules() {
        let mut rules = Rules {
            enabled: true,
            fallback: 2,
            entries: vec![Rule {
                app: " org.kde.konsole ".into(),
                profile: 6,
            }],
        };
        assert_eq!(rules.target(Some("ORG.KDE.KONSOLE")), Some(6));
        assert_eq!(rules.target(Some("konsole")), Some(2));
        assert_eq!(rules.target(None), Some(2));
        assert_eq!(Rules::decode(&rules.encode()), Ok(rules.clone()));
        rules.enabled = false;
        assert_eq!(rules.target(Some("org.kde.konsole")), None);
    }

    #[test]
    fn invalid_saved_rules_do_not_select_a_profile() {
        for text in [
            "1\t7",
            "1\t0\n7\tapp",
            "1\t0\n1\t",
            "1\t0\n1\tAPP\n2\tapp",
            "yes\t0",
        ] {
            assert!(Rules::decode(text).is_err(), "{text}");
        }
    }
}
