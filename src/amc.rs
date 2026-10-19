//! Official AMC macro files: UTF-16 XML with KeyDown and KeyUp syntax, a
//! repeat type and a file format. See Data/Mouse/DefaultLog/*/Sniper/*.amc.
use crate::{
    highlanguage,
    macros::{self, Program, Repeat},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Macro {
    pub description: String,
    pub high_language: bool,
    pub repeat: Repeat,
    pub press: String,
    pub release: String,
}

fn decode(bytes: &[u8]) -> Result<String, String> {
    let utf16 = |bytes: &[u8], big: bool| {
        let units: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| {
                if big {
                    u16::from_be_bytes(*pair)
                } else {
                    u16::from_le_bytes(*pair)
                }
            })
            .collect();
        String::from_utf16(&units).map_err(|_| "AMC 文件不是有效的 UTF-16".to_string())
    };
    match bytes {
        [0xff, 0xfe, rest @ ..] => utf16(rest, false),
        [0xfe, 0xff, rest @ ..] => utf16(rest, true),
        [0xef, 0xbb, 0xbf, rest @ ..] => {
            String::from_utf8(rest.to_vec()).map_err(|e| e.to_string())
        }
        [b'<', 0, ..] => utf16(bytes, false),
        _ => String::from_utf8(bytes.to_vec()).map_err(|_| "无法识别 AMC 文件编码".into()),
    }
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

// The first <tag>…</tag> inside `text`, or an empty string for <tag/>.
fn element<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    if let Some(start) = text.find(&open) {
        let start = start + open.len();
        let end = text[start..].find(&format!("</{tag}>"))?;
        return Some(&text[start..start + end]);
    }
    text.contains(&format!("<{tag}/>")).then_some("")
}

pub fn parse(bytes: &[u8]) -> Result<Macro, String> {
    let text = decode(bytes)?;
    let body = element(&text, "Macro").ok_or("缺少 Macro 元素")?;
    let syntax = |tag| {
        element(body, tag)
            .and_then(|block| element(block, "Syntax"))
            .map(unescape)
            .unwrap_or_default()
    };
    let repeat = match element(body, "RepeatType").map(str::trim) {
        None | Some("" | "0") => Repeat::Once,
        Some("1") => Repeat::Held,
        Some("2") => Repeat::Toggle,
        Some(other) => return Err(format!("未知的重复方式 {other}")),
    };
    Ok(Macro {
        description: element(body, "Description")
            .map(unescape)
            .unwrap_or_default(),
        high_language: element(body, "FileFormat")
            .is_some_and(|format| format.trim().eq_ignore_ascii_case("highlanguage")),
        repeat,
        press: syntax("KeyDown"),
        release: syntax("KeyUp"),
    })
}

impl Macro {
    /// Compile as EXE 00629638 does for type M: HighLanguage first, then M.
    pub fn compile(&self) -> Result<Program, String> {
        let text = |source: &str| {
            if self.high_language {
                highlanguage::translate(source)
            } else {
                Ok(source.to_owned())
            }
        };
        macros::compile(&text(&self.press)?, &text(&self.release)?, self.repeat)
    }

    /// UTF-16 file in the layout of the official default macros.
    pub fn to_bytes(&self) -> Vec<u8> {
        let repeat = match self.repeat {
            Repeat::Once => 0,
            Repeat::Held => 1,
            Repeat::Toggle => 2,
        };
        let crlf = |text: &str| text.replace("\r\n", "\n").replace('\n', "\r\n");
        let xml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-16\"?>\r\n<Macro>\r\n\t<Name></Name>\r\n\t<Guid></Guid>\r\n\t<FileFormat>{}</FileFormat>\r\n\t<DefaultMacro>\r\n\t\t<Major></Major>\r\n\t\t<Software></Software>\r\n\t\t<Description>{}</Description>\r\n\t\t<Comment></Comment>\r\n\t\t<Resolution></Resolution>\r\n\t\t<CPIRemark></CPIRemark>\r\n\t\t<GUIOption>\r\n\t\t\t<RepeatType>{repeat}</RepeatType>\r\n\t\t</GUIOption>\r\n\t\t<KeyUp>\r\n\t\t\t<Syntax>{}</Syntax>\r\n\t\t</KeyUp>\r\n\t\t<KeyDown>\r\n\t\t\t<Syntax>{}</Syntax>\r\n\t\t</KeyDown>\r\n\t</DefaultMacro>\r\n\t<IsFolder></IsFolder>\r\n\t<FolderGuid></FolderGuid>\r\n</Macro>\r\n",
            if self.high_language {
                "HighLanguage"
            } else {
                ""
            },
            escape(&self.description),
            escape(&crlf(&self.release)),
            escape(&crlf(&self.press)),
        );
        let mut bytes = vec![0xff, 0xfe];
        bytes.extend(xml.encode_utf16().flat_map(u16::to_le_bytes));
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(text: &str) -> Vec<u8> {
        let mut bytes = vec![0xff, 0xfe];
        bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        bytes
    }

    #[test]
    fn official_sniper_layout() {
        let file = utf16(
            "<?xml version=\"1.0\" encoding=\"UTF-16\"?>\r\n<Macro>\r\n\t<Name></Name>\r\n\t<FileFormat>HighLanguage</FileFormat>\r\n\t<DefaultMacro>\r\n\t\t<Description>Sniper2</Description>\r\n\t\t<GUIOption>\r\n\t\t\t<RepeatType>0</RepeatType>\r\n\t\t</GUIOption>\r\n\t\t<KeyUp>\r\n\t\t\t<Syntax>RightUp 1\r\ndpi accurateout</Syntax>\r\n\t\t</KeyUp>\r\n\t\t<KeyDown>\r\n\t\t\t<Syntax>RightDown 1\r\ndpi accuratein</Syntax>\r\n\t\t</KeyDown>\r\n\t</DefaultMacro>\r\n</Macro>\r\n",
        );
        let parsed = parse(&file).unwrap();
        assert!(parsed.high_language);
        assert_eq!(parsed.description, "Sniper2");
        assert_eq!(parsed.press, "RightDown 1\r\ndpi accuratein");
        let program = parsed.compile().unwrap();
        assert_eq!(program.press, [0x2502, 0x2628, crate::storage::END]);
        assert_eq!(program.release, [0x2402, 0x2629, crate::storage::END]);
        assert_eq!(
            parse(&parsed.to_bytes()).unwrap(),
            Macro {
                press: "RightDown 1\r\ndpi accuratein".into(),
                release: "RightUp 1\r\ndpi accurateout".into(),
                ..parsed
            }
        );
    }

    #[test]
    fn plain_round_trip_and_escapes() {
        let source = Macro {
            description: "a < b & c".into(),
            high_language: false,
            repeat: Repeat::Toggle,
            press: "if VarA != 1 goto 1".into(),
            release: String::new(),
        };
        let parsed = parse(&source.to_bytes()).unwrap();
        assert_eq!(parsed, source);
        assert!(parsed.compile().is_ok());
        assert!(parse(b"<Other/>").is_err());
    }
}
