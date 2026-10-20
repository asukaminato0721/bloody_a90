//! Display language. Text is written in Simplified Chinese and looked up in
//! a table per language when shown. Captions the official program has come
//! from its own translations; the rest falls back to English.
use std::{
    collections::HashMap,
    sync::{
        LazyLock, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

/// The languages the official program ships, in its folder order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    ChineseS,
    ChineseT,
    English,
    Dutch,
    French,
    German,
    Italian,
    Japanese,
    Korean,
    Polish,
    Portuguese,
    Russian,
    Spanish,
    Turkish,
}

impl Language {
    pub const ALL: [Language; 14] = [
        Language::ChineseS,
        Language::ChineseT,
        Language::English,
        Language::Dutch,
        Language::French,
        Language::German,
        Language::Italian,
        Language::Japanese,
        Language::Korean,
        Language::Polish,
        Language::Portuguese,
        Language::Russian,
        Language::Spanish,
        Language::Turkish,
    ];

    /// The official folder name, also used in the state file.
    pub fn code(self) -> &'static str {
        match self {
            Language::ChineseS => "ChineseS",
            Language::ChineseT => "ChineseT",
            Language::English => "English",
            Language::Dutch => "Dutch",
            Language::French => "French",
            Language::German => "German",
            Language::Italian => "Italian",
            Language::Japanese => "Japanese",
            Language::Korean => "Korean",
            Language::Polish => "Polish",
            Language::Portuguese => "Portuguese",
            Language::Russian => "Russian",
            Language::Spanish => "Spanish",
            Language::Turkish => "Turkish",
        }
    }

    /// The language's name in itself.
    pub fn name(self) -> &'static str {
        match self {
            Language::ChineseS => "简体中文",
            Language::ChineseT => "繁體中文",
            Language::English => "English",
            Language::Dutch => "Nederlands",
            Language::French => "Français",
            Language::German => "Deutsch",
            Language::Italian => "Italiano",
            Language::Japanese => "日本語",
            Language::Korean => "한국어",
            Language::Polish => "Polski",
            Language::Portuguese => "Português",
            Language::Russian => "Русский",
            Language::Spanish => "Español",
            Language::Turkish => "Türkçe",
        }
    }

    pub fn from_code(code: &str) -> Option<Language> {
        Language::ALL
            .into_iter()
            .find(|language| language.code() == code)
    }

    fn data(self) -> &'static str {
        match self {
            Language::ChineseS => "",
            Language::ChineseT => include_str!("i18n/ChineseT.tsv"),
            Language::English => include_str!("i18n/English.tsv"),
            Language::Dutch => include_str!("i18n/Dutch.tsv"),
            Language::French => include_str!("i18n/French.tsv"),
            Language::German => include_str!("i18n/German.tsv"),
            Language::Italian => include_str!("i18n/Italian.tsv"),
            Language::Japanese => include_str!("i18n/Japanese.tsv"),
            Language::Korean => include_str!("i18n/Korean.tsv"),
            Language::Polish => include_str!("i18n/Polish.tsv"),
            Language::Portuguese => include_str!("i18n/Portuguese.tsv"),
            Language::Russian => include_str!("i18n/Russian.tsv"),
            Language::Spanish => include_str!("i18n/Spanish.tsv"),
            Language::Turkish => include_str!("i18n/Turkish.tsv"),
        }
    }
}

static CURRENT: AtomicUsize = AtomicUsize::new(0);

pub fn language() -> Language {
    Language::ALL[CURRENT.load(Ordering::Relaxed)]
}

pub fn set_language(language: Language) {
    let index = Language::ALL
        .iter()
        .position(|l| *l == language)
        .unwrap_or(0);
    CURRENT.store(index, Ordering::Relaxed);
}

/// One language's translations. Keys with `{}` are templates for formatted
/// text; they are tried longest first.
struct Table {
    exact: HashMap<&'static str, &'static str>,
    templates: Vec<(Vec<&'static str>, &'static str)>,
}

static TABLES: LazyLock<Vec<Table>> =
    LazyLock::new(|| Language::ALL.iter().map(|l| parse(l.data())).collect());

/// Results of [`tr_text`] for text built at run time.
static CACHE: LazyLock<Mutex<HashMap<(usize, String), String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn unescape(text: &str) -> &'static str {
    if !text.contains('\\') {
        return Box::leak(text.to_owned().into_boxed_str());
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    Box::leak(out.into_boxed_str())
}

/// `key<TAB>value` lines; `#` starts a comment, `\n`, `\t` and `\\` escape.
/// An empty value drops the text, for words other languages don't need.
fn parse(data: &'static str) -> Table {
    let mut table = Table {
        exact: HashMap::new(),
        templates: Vec::new(),
    };
    for line in data.lines() {
        if line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('\t') else {
            continue;
        };
        let (key, value) = (unescape(key), unescape(value));
        if key.contains("{}") {
            table.templates.push((key.split("{}").collect(), value));
        } else {
            table.exact.insert(key, value);
        }
    }
    table.templates.sort_by_key(|(parts, _)| {
        std::cmp::Reverse(parts.iter().map(|part| part.len()).sum::<usize>())
    });
    table
}

/// The tables to try for the current language, most specific first.
/// Traditional Chinese and English have full tables of their own; the
/// other languages fall back to English.
fn chain() -> impl Iterator<Item = &'static Table> {
    const ENGLISH: usize = 2;
    let index = CURRENT.load(Ordering::Relaxed);
    let list = match index {
        0 => [None, None],
        1 | ENGLISH => [Some(index), None],
        _ => [Some(index), Some(ENGLISH)],
    };
    let tables: &'static [Table] = &TABLES;
    list.into_iter().flatten().map(move |i| &tables[i])
}

/// Translate a fixed caption.
pub fn tr(text: &'static str) -> &'static str {
    chain()
        .find_map(|table| table.exact.get(text).copied())
        .unwrap_or(text)
}

fn exact(text: &str) -> Option<&'static str> {
    chain().find_map(|table| table.exact.get(text).copied())
}

/// Split `text` along the literal parts of a template.
fn captures<'a>(parts: &[&str], text: &'a str) -> Option<Vec<&'a str>> {
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if text.len() < first.len() + last.len() || !text.starts_with(first) || !text.ends_with(last) {
        return None;
    }
    let mut rest = &text[first.len()..text.len() - last.len()];
    let mut found = Vec::with_capacity(parts.len() - 1);
    for part in &parts[1..parts.len() - 1] {
        let at = rest.find(part)?;
        found.push(&rest[..at]);
        rest = &rest[at + part.len()..];
    }
    found.push(rest);
    Some(found)
}

/// Fill `{}` (in order) or `{0}`, `{1}` … of a translated template.
fn fill(template: &str, values: &[&str]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut next = 0;
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let tail = &rest[start + 1..];
        let Some(end) = tail.find('}') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let inside = &tail[..end];
        let index = if inside.is_empty() {
            next += 1;
            Some(next - 1)
        } else {
            inside.parse::<usize>().ok()
        };
        match index.and_then(|i| values.get(i)) {
            Some(value) => out.push_str(exact(value).unwrap_or(value)),
            None => {
                out.push('{');
                out.push_str(inside);
                out.push('}');
            }
        }
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    out
}

fn translate(text: &str) -> Option<String> {
    if let Some(found) = exact(text) {
        return Some(found.to_owned());
    }
    for table in chain() {
        for (parts, value) in &table.templates {
            if let Some(values) = captures(parts, text) {
                return Some(fill(value, &values));
            }
        }
    }
    None
}

/// Translate text built at run time: an exact caption, a formatted message
/// whose template is known, or else line by line.
pub fn tr_text(text: &str) -> String {
    let index = CURRENT.load(Ordering::Relaxed);
    if index == 0 || text.is_empty() || text.is_ascii() {
        return text.to_owned();
    }
    let key = (index, text.to_owned());
    if let Some(found) = CACHE.lock().ok().and_then(|cache| cache.get(&key).cloned()) {
        return found;
    }
    let result = translate(text).unwrap_or_else(|| {
        if text.contains('\n') {
            text.split('\n')
                .map(|line| translate(line).unwrap_or_else(|| line.to_owned()))
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            text.to_owned()
        }
    });
    if let Ok(mut cache) = CACHE.lock() {
        if cache.len() > 4096 {
            cache.clear();
        }
        cache.insert(key, result.clone());
    }
    result
}

/// `format!` whose result is shown in the current language.
#[macro_export]
macro_rules! tf {
    ($($arg:tt)*) => {
        $crate::i18n::tr_text(&format!($($arg)*))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_fill_in_order_and_by_index() {
        let parts: Vec<&str> = "配置 {} 的记录 {}".split("{}").collect();
        let values = captures(&parts, "配置 2 的记录 7").unwrap();
        assert_eq!(values, ["2", "7"]);
        assert_eq!(
            fill("Record {1} of profile {0}", &values),
            "Record 7 of profile 2"
        );
        assert_eq!(
            fill("Profile {}, record {}", &values),
            "Profile 2, record 7"
        );
        assert!(captures(&parts, "配置 2").is_none());
    }

    #[test]
    fn every_table_parses_and_keeps_placeholders() {
        for language in Language::ALL {
            let table = parse(language.data());
            for (parts, value) in &table.templates {
                let wanted = parts.len() - 1;
                let have = value.matches("{}").count()
                    + (0..wanted)
                        .filter(|i| value.contains(&format!("{{{i}}}")))
                        .count();
                assert_eq!(have, wanted, "{language:?}: {value}");
            }
        }
    }
}
