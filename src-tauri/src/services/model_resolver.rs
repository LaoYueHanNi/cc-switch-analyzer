use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

use regex::{Regex, RegexBuilder};

type Resolution = Result<Option<String>, String>;

/// Global stages: model ID, historical/user exact alias, then portable regex.
/// Names are folded only for ASCII case; stored canonical IDs remain unchanged.
#[derive(Default)]
pub struct ModelResolver {
    ids: HashMap<String, HashSet<String>>,
    aliases: HashMap<String, HashSet<String>>,
    patterns: Vec<(String, Vec<Regex>)>,
    cache: RwLock<HashMap<String, Resolution>>,
}

/// The upstream portable subset: literals, escaped punctuation, groups, | and ?.
/// Reject engine-specific syntax rather than accepting different JS/Rust meanings.
pub fn compile_alias_pattern(pattern: &str) -> Result<Regex, String> {
    let invalid = || format!("非法模型别名正则: {}", pattern);
    if !pattern.starts_with('^') || !pattern.ends_with('$') || pattern.len() < 3 {
        return Err(invalid());
    }
    let mut chars = pattern[1..pattern.len() - 1].chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => match chars.next() {
                Some(literal) if literal.is_ascii_punctuation() => (),
                _ => return Err(invalid()),
            },
            '(' if chars.peek() == Some(&'?') => return Err(invalid()),
            '(' | ')' | '|' | '?' | '-' | '_' | ':' | '/' | ' ' => (),
            literal if literal.is_ascii_alphanumeric() => (),
            _ => return Err(invalid()),
        }
    }
    RegexBuilder::new(pattern)
        .case_insensitive(true)
        .unicode(false)
        .size_limit(1024 * 1024)
        .build()
        .map_err(|e| format!("{}: {}", invalid(), e))
}

impl ModelResolver {
    pub fn add_model(&mut self, model_id: &str) {
        self.cache.get_mut().unwrap().clear();
        self.ids.entry(model_id.to_ascii_lowercase()).or_default().insert(model_id.into());
    }

    pub fn add_alias(&mut self, model_id: &str, alias: &str) {
        self.cache.get_mut().unwrap().clear();
        self.aliases.entry(alias.to_ascii_lowercase()).or_default().insert(model_id.into());
    }

    pub fn add_patterns(&mut self, model_id: &str, patterns: &[String]) -> Result<(), String> {
        self.cache.get_mut().unwrap().clear();
        let mut seen = HashSet::new();
        let mut compiled = Vec::new();
        for pattern in patterns {
            if !seen.insert(pattern) {
                return Err(format!("模型 {} 的别名正则重复: {}", model_id, pattern));
            }
            compiled.push(compile_alias_pattern(pattern)?);
        }
        self.patterns.push((model_id.into(), compiled));
        Ok(())
    }

    fn unique(name: &str, hits: &HashSet<String>) -> Resolution {
        if hits.len() > 1 {
            let mut models: Vec<_> = hits.iter().cloned().collect();
            models.sort();
            Err(format!("模型别名存在歧义: {} → {}", name, models.join(", ")))
        } else {
            Ok(hits.iter().next().cloned())
        }
    }

    fn resolve_uncached(&self, name: &str) -> Resolution {
        let key = name.to_ascii_lowercase();
        if let Some(hits) = self.ids.get(&key) {
            return Self::unique(name, hits);
        }
        if let Some(hits) = self.aliases.get(&key) {
            return Self::unique(name, hits);
        }
        let hits = self.patterns.iter().filter(|(_, patterns)| patterns.iter().any(|regex| {
            regex.find(name).is_some_and(|m| m.start() == 0 && m.end() == name.len())
        })).map(|(model_id, _)| model_id.clone()).collect();
        Self::unique(name, &hits)
    }

    pub fn resolve(&self, name: &str) -> Resolution {
        // Cache raw names: never fold non-ASCII characters for regex lookup.
        if let Ok(cache) = self.cache.read() {
            if let Some(result) = cache.get(name) {
                return result.clone();
            }
        }
        let result = self.resolve_uncached(name);
        if let Err(error) = &result {
            log::error!("[PRICING] {}", error);
        }
        if let Ok(mut cache) = self.cache.write() {
            // Bound negative lookups as well as successful resolutions.
            if cache.len() >= 4096 {
                cache.clear();
            }
            cache.insert(name.into(), result.clone());
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_cases_match_frontend_and_preserve_model_identity() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!("../../../tests/fixtures/model-aliases.json")).unwrap();
        let mut resolver = ModelResolver::default();
        for model in fixture["models"].as_array().unwrap() {
            let id = model["modelId"].as_str().unwrap();
            resolver.add_model(id);
            for field in ["aliases", "userAliases"] {
                if let Some(aliases) = model[field].as_array() {
                    for alias in aliases { resolver.add_alias(id, alias.as_str().unwrap()); }
                }
            }
            let patterns: Vec<String> = serde_json::from_value(model["aliasPatterns"].clone()).unwrap();
            resolver.add_patterns(id, &patterns).unwrap();
        }
        for case in fixture["cases"].as_array().unwrap() {
            let name = case[0].as_str().unwrap();
            let expected = case[1].as_str().map(String::from);
            for variant in [name.to_string(), name.to_ascii_uppercase()] {
                assert_eq!(resolver.resolve(&variant).unwrap(), expected, "{}", variant);
            }
        }
    }

    #[test]
    fn global_stages_ambiguity_and_multi_pattern_deduplication() {
        let mut r = ModelResolver::default();
        r.add_patterns("pattern", &["^example$".into(), "^EXAMPLE$".into()]).unwrap();
        assert_eq!(r.resolve("EXAMPLE").unwrap().as_deref(), Some("pattern"));
        r.add_alias("alias", "example");
        assert_eq!(r.resolve("EXAMPLE").unwrap().as_deref(), Some("alias"));
        r.add_model("Example");
        assert_eq!(r.resolve("EXAMPLE").unwrap().as_deref(), Some("Example"));
        r.add_model("EXAMPLE");
        assert!(r.resolve("example").unwrap_err().contains("歧义"));
        let mut r = ModelResolver::default();
        r.add_alias("a", "name");
        r.add_alias("b", "NAME");
        assert!(r.resolve("Name").is_err());
        let mut r = ModelResolver::default();
        r.add_patterns("a", &["^name$".into()]).unwrap();
        r.add_patterns("b", &["^NAME$".into()]).unwrap();
        assert!(r.resolve("Name").is_err());
    }

    #[test]
    fn invalid_unportable_and_duplicate_patterns_fail_validation() {
        for pattern in ["", "^$", "name", "^a.*$", "^a+$", "^a{2}$", "^(?=a)a$", "^a\\1$", "(?i)^a$", "^a.$", "^a($"] {
            assert!(compile_alias_pattern(pattern).is_err(), "{}", pattern);
        }
        assert!(ModelResolver::default().add_patterns("a", &["^a$".into(), "^a$".into()]).is_err());
    }
}
