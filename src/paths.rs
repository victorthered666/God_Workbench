use std::path::{Component, Path, PathBuf};
use crate::{AppError, Result};

pub fn safe_name(name: &str) -> String {
    let mut result: String = name.chars().filter(|c| !c.is_control()).map(|c| if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') { '_' } else { c }).collect();
    result = result.trim_matches(|c: char| c == ' ' || c == '.').chars().take(90).collect();
    if result.is_empty() { "未命名策划文档".into() } else { result }
}

pub fn truncate_utf8(text: &str, limit: usize) -> &str {
    if text.len() <= limit { return text; }
    let mut index = limit;
    while index > 0 && !text.is_char_boundary(index) { index -= 1; }
    &text[..index]
}

pub fn contained_file(root: &Path, relative: &str) -> Result<PathBuf> {
    let relative_path = Path::new(relative);
    if relative_path.is_absolute() || relative_path.components().any(|c| matches!(c, Component::ParentDir | Component::RootDir | Component::Prefix(_))) {
        return Err(AppError::Invalid("素材路径越界".into()));
    }
    let path = root.join(relative_path).canonicalize().map_err(|_| AppError::Invalid("素材不存在".into()))?;
    let root = root.canonicalize()?;
    if !path.starts_with(&root) || !path.is_file() {
        return Err(AppError::Invalid("素材路径越界".into()));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn safe_names_and_utf8_limits() {
        assert_eq!(safe_name("a/b"), "a_b");
        assert_eq!(safe_name("..."), "未命名策划文档");
        assert_eq!(truncate_utf8("游戏设计", 3), "游");
        assert_eq!(truncate_utf8("游戏设计", 2), "");
    }
}
