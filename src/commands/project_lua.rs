//! The fleet's `config/project.lua` files, read for `gh sync-metadata`.
//!
//! They are plain data: `NAME = "..."`
//! assignments (quoted or `[[long]]` strings), booleans (`PYPI = true`), a
//! `KEYWORDS = { "...", ... }` list, `--` comments and the odd `X = Y`
//! alias. That subset is parsed here rather than by shelling out to a lua
//! interpreter, so the binary stays self-contained. Anything outside it is
//! an error naming the line, never a guess.

use anyhow::{Context, Result};

/// What config/project.lua says about the repo. Description and keywords
/// are optional: "unset locally" means "leave GitHub alone", not "clear it".
#[derive(Debug, PartialEq, Eq)]
pub struct LocalMeta {
    pub name: String,
    pub description: Option<String>,
    pub keywords: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Ident(String),
    Str(String),
    Eq,
    LBrace,
    RBrace,
    Comma,
}

fn tokenize_lua(text: &str) -> Result<Vec<(usize, Token)>> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut line = 1;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\n' => {
                line += 1;
                i += 1;
            }
            c if c.is_whitespace() => i += 1,
            '-' if chars.get(i + 1) == Some(&'-') => {
                // Comment to end of line; the newline itself is counted
                // by the next iteration.
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '=' => {
                tokens.push((line, Token::Eq));
                i += 1;
            }
            '{' => {
                tokens.push((line, Token::LBrace));
                i += 1;
            }
            '}' => {
                tokens.push((line, Token::RBrace));
                i += 1;
            }
            ',' => {
                tokens.push((line, Token::Comma));
                i += 1;
            }
            '[' if chars.get(i + 1) == Some(&'[') => {
                // Long string: `[[` ... `]]`, verbatim, no escapes. Lua
                // drops a newline that immediately follows the opening.
                let start_line = line;
                i += 2;
                if chars.get(i) == Some(&'\n') {
                    i += 1;
                    line += 1;
                }
                let mut s = String::new();
                loop {
                    let Some(&ch) = chars.get(i) else {
                        anyhow::bail!("line {start_line}: unterminated long string");
                    };
                    if ch == ']' && chars.get(i + 1) == Some(&']') {
                        i += 2;
                        break;
                    }
                    if ch == '\n' {
                        line += 1;
                    }
                    s.push(ch);
                    i += 1;
                }
                tokens.push((start_line, Token::Str(s)));
            }
            '"' | '\'' => {
                let quote = c;
                let start_line = line;
                let mut s = String::new();
                i += 1;
                loop {
                    let Some(&ch) = chars.get(i) else {
                        anyhow::bail!("line {start_line}: unterminated string");
                    };
                    i += 1;
                    match ch {
                        ch if ch == quote => break,
                        '\\' => {
                            let Some(&esc) = chars.get(i) else {
                                anyhow::bail!("line {start_line}: unterminated string");
                            };
                            i += 1;
                            s.push(match esc {
                                'n' => '\n',
                                't' => '\t',
                                '\\' => '\\',
                                '"' => '"',
                                '\'' => '\'',
                                other => anyhow::bail!(
                                    "line {line}: unsupported escape \\{other} in string"
                                ),
                            });
                        }
                        '\n' => anyhow::bail!("line {start_line}: newline in string"),
                        ch => s.push(ch),
                    }
                }
                tokens.push((start_line, Token::Str(s)));
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let ident: String = chars[start..i].iter().collect();
                tokens.push((line, Token::Ident(ident)));
            }
            other => anyhow::bail!("line {line}: unsupported character {other:?}"),
        }
    }
    Ok(tokens)
}

/// A value on the right-hand side of a project.lua assignment.
#[derive(Debug, Clone, PartialEq, Eq)]
enum LuaValue {
    Str(String),
    List(Vec<String>),
    /// `true`, `false` or `nil`: carried so an alias to it resolves, never
    /// used by the sync itself.
    Keyword(String),
}

/// Parse the assignment subset of lua used by config/project.lua into the
/// fields sync-metadata cares about.
pub fn parse_project_lua(text: &str) -> Result<LocalMeta> {
    let tokens = tokenize_lua(text)?;
    let mut vars: Vec<(String, LuaValue)> = Vec::new();
    let lookup = |vars: &[(String, LuaValue)], name: &str| -> Option<LuaValue> {
        vars.iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.clone())
    };
    let mut i = 0;
    while i < tokens.len() {
        let (line, ref tok) = tokens[i];
        let Token::Ident(name) = tok else {
            anyhow::bail!("line {line}: expected an assignment, found {tok:?}");
        };
        let Some((_, Token::Eq)) = tokens.get(i + 1) else {
            anyhow::bail!("line {line}: expected `=` after {name}");
        };
        i += 2;
        let value = match tokens.get(i) {
            Some((_, Token::Str(s))) => {
                i += 1;
                LuaValue::Str(s.clone())
            }
            Some((_, Token::Ident(kw))) if matches!(kw.as_str(), "true" | "false" | "nil") => {
                i += 1;
                LuaValue::Keyword(kw.clone())
            }
            Some((l, Token::Ident(other))) => {
                i += 1;
                lookup(&vars, other)
                    .with_context(|| format!("line {l}: {name} refers to undefined {other}"))?
            }
            Some((_, Token::LBrace)) => {
                i += 1;
                let mut items = Vec::new();
                loop {
                    match tokens.get(i) {
                        Some((_, Token::RBrace)) => {
                            i += 1;
                            break;
                        }
                        Some((_, Token::Str(s))) => {
                            items.push(s.clone());
                            i += 1;
                            match tokens.get(i) {
                                Some((_, Token::Comma)) => i += 1,
                                Some((_, Token::RBrace)) => {}
                                Some((l, t)) => {
                                    anyhow::bail!(
                                        "line {l}: expected `,` or `}}` in list, found {t:?}"
                                    )
                                }
                                None => anyhow::bail!("line {line}: unterminated list for {name}"),
                            }
                        }
                        Some((l, t)) => {
                            anyhow::bail!("line {l}: expected a string in list, found {t:?}")
                        }
                        None => anyhow::bail!("line {line}: unterminated list for {name}"),
                    }
                }
                LuaValue::List(items)
            }
            Some((l, t)) => anyhow::bail!("line {l}: unsupported value for {name}: {t:?}"),
            None => anyhow::bail!("line {line}: missing value for {name}"),
        };
        vars.push((name.clone(), value));
    }

    let name = match lookup(&vars, "NAME") {
        Some(LuaValue::Str(s)) if !s.is_empty() => s,
        Some(LuaValue::Str(_)) | None => anyhow::bail!("no NAME"),
        Some(other) => anyhow::bail!("NAME is {other:?}, expected a string"),
    };
    let description = match lookup(&vars, "DESCRIPTION_SHORT") {
        Some(LuaValue::Str(s)) => Some(s),
        Some(LuaValue::Keyword(k)) if k == "nil" => None,
        Some(other) => anyhow::bail!("DESCRIPTION_SHORT is {other:?}, expected a string"),
        None => None,
    };
    let keywords = match lookup(&vars, "KEYWORDS") {
        Some(LuaValue::List(l)) => Some(l),
        Some(LuaValue::Keyword(k)) if k == "nil" => None,
        Some(other) => anyhow::bail!("KEYWORDS is {other:?}, expected a list"),
        None => None,
    };
    Ok(LocalMeta {
        name,
        description,
        keywords,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_project_lua_multiline_keywords() {
        let text = r#"-- project definitions
NAME = "demos-os-linux"
DESCRIPTION_SHORT = "demos-os-linux is a project to demo the Linux API"
DESCRIPTION_LONG = DESCRIPTION_SHORT
KEYWORDS = {
    "linux",
    "api", -- trailing comment
    "c-plus-plus",
}
"#;
        let meta = parse_project_lua(text).unwrap();
        assert_eq!(
            meta,
            LocalMeta {
                name: "demos-os-linux".to_string(),
                description: Some("demos-os-linux is a project to demo the Linux API".to_string()),
                keywords: Some(vec![
                    "linux".to_string(),
                    "api".to_string(),
                    "c-plus-plus".to_string()
                ]),
            }
        );
    }

    #[test]
    fn parse_project_lua_single_line_keywords_and_extra_fields() {
        let text = r#"NAME = "x"
NAME_FANCY = "X"
LICENSE_TYPE = "MIT"
PYPI = "x-py"
KEYWORDS = {"a", "b", "c"}
"#;
        let meta = parse_project_lua(text).unwrap();
        assert_eq!(meta.name, "x");
        assert_eq!(meta.description, None);
        assert_eq!(
            meta.keywords,
            Some(vec!["a".to_string(), "b".to_string(), "c".to_string()])
        );
    }

    #[test]
    fn parse_project_lua_string_escapes_and_single_quotes() {
        let text = "NAME = 'n'\nDESCRIPTION_SHORT = \"say \\\"hi\\\"\"\n";
        let meta = parse_project_lua(text).unwrap();
        assert_eq!(meta.name, "n");
        assert_eq!(meta.description.as_deref(), Some("say \"hi\""));
    }

    #[test]
    fn parse_project_lua_booleans_and_long_strings() {
        let text = "NAME = \"n\"\nPYPI = true\nDESCRIPTION_LONG = [[\nfirst line\nsecond line]]\nDESCRIPTION_SHORT = [[one-liner]]\nKEYWORDS = nil\n";
        let meta = parse_project_lua(text).unwrap();
        assert_eq!(meta.name, "n");
        assert_eq!(meta.description.as_deref(), Some("one-liner"));
        assert_eq!(meta.keywords, None);
        // A boolean where a string is expected is a type error, not a guess.
        let err = parse_project_lua("NAME = \"n\"\nDESCRIPTION_SHORT = true\n").unwrap_err();
        assert!(err.to_string().contains("DESCRIPTION_SHORT"), "{err}");
    }

    #[test]
    fn parse_project_lua_long_string_keeps_inner_newlines() {
        let text = "NAME = \"n\"\nDESCRIPTION_SHORT = [[\na\nb]]\n";
        let meta = parse_project_lua(text).unwrap();
        assert_eq!(meta.description.as_deref(), Some("a\nb"));
    }

    #[test]
    fn parse_project_lua_requires_name() {
        let err = parse_project_lua("KEYWORDS = {\"a\"}\n").unwrap_err();
        assert!(err.to_string().contains("NAME"), "{err}");
    }

    #[test]
    fn parse_project_lua_rejects_unsupported_syntax_with_line() {
        let err = parse_project_lua("NAME = \"n\"\nKEYWORDS = { \"a\" .. \"b\" }\n").unwrap_err();
        assert!(err.to_string().contains("line 2"), "{err}");
        let err = parse_project_lua("NAME = \"n\"\nlocal x = 1\n").unwrap_err();
        assert!(err.to_string().contains("line 2"), "{err}");
    }

    #[test]
    fn parse_project_lua_alias_to_undefined_is_an_error() {
        let err = parse_project_lua("NAME = \"n\"\nDESCRIPTION_LONG = NOPE\n").unwrap_err();
        assert!(err.to_string().contains("NOPE"), "{err}");
    }
}
