//! Bookmarks as the HTML file every browser imports and exports (the
//! Netscape bookmark format: `<DT><H3>` folders holding `<DL>` lists of
//! `<DT><A HREF>` links). Import flattens nested folders to one level,
//! joining their names with " / "; the folder marked as the toolbar
//! (`PERSONAL_TOOLBAR_FOLDER`) becomes [`Bookmarks::BAR`]; links outside
//! any folder go to a group named by the caller. Export writes one
//! folder per group, the bar marked as the toolbar folder.

use url::Url;

use crate::bookmarks::{Bookmark, BookmarkGroup, Bookmarks};

/// Read the bookmarks out of an HTML bookmarks file. Links outside any
/// folder go to the group `loose`. Nothing is an error: a file with no
/// links is no bookmarks.
pub fn import(html: &str, loose: &str) -> Bookmarks {
    let mut bookmarks = Bookmarks::default();
    // The folders open around the position: `None` for the root list.
    let mut stack: Vec<Option<String>> = Vec::new();
    let mut pending: Option<String> = None;
    let mut rest = html;
    while let Some(start) = rest.find('<') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('>') else {
            break;
        };
        let tag = &after[..end];
        let mut following = &after[end + 1..];
        let name = tag
            .split(|c: char| c.is_whitespace() || c == '/')
            .find(|part| !part.is_empty())
            .unwrap_or("")
            .to_ascii_uppercase();
        let closing = tag.starts_with('/');
        match (name.as_str(), closing) {
            ("H3", false) => {
                let (text, next) = text_until(following, "</h3>");
                let toolbar = attribute(tag, "PERSONAL_TOOLBAR_FOLDER")
                    .is_some_and(|v| v.eq_ignore_ascii_case("true"));
                pending = Some(if toolbar {
                    Bookmarks::BAR.to_string()
                } else {
                    decode(text.trim())
                });
                following = next;
            }
            ("DL", false) => stack.push(pending.take()),
            ("DL", true) => {
                stack.pop();
            }
            ("A", false) => {
                let (text, next) = text_until(following, "</a>");
                following = next;
                if let Some(href) = attribute(tag, "HREF")
                    && let Ok(url) = Url::parse(&decode(&href))
                {
                    let group = group_name(&stack, loose);
                    let mut title = decode(text.trim());
                    if title.is_empty() {
                        title = url.to_string();
                    }
                    bookmarks.add(&group, vec![Bookmark { title, url }]);
                }
            }
            _ => {}
        }
        rest = following;
    }
    bookmarks
}

/// Write `bookmarks` as an HTML bookmarks file.
pub fn export(bookmarks: &Bookmarks) -> String {
    let mut out = String::from(
        "<!DOCTYPE NETSCAPE-Bookmark-file-1>\n\
         <!-- This is an automatically generated file.\n     \
         It will be read and overwritten.\n     \
         DO NOT EDIT! -->\n\
         <META HTTP-EQUIV=\"Content-Type\" CONTENT=\"text/html; charset=UTF-8\">\n\
         <TITLE>Bookmarks</TITLE>\n\
         <H1>Bookmarks</H1>\n\
         <DL><p>\n",
    );
    for group in &bookmarks.groups {
        write_group(&mut out, group);
    }
    out.push_str("</DL><p>\n");
    out
}

fn write_group(out: &mut String, group: &BookmarkGroup) {
    let toolbar = if group.name == Bookmarks::BAR {
        " PERSONAL_TOOLBAR_FOLDER=\"true\""
    } else {
        ""
    };
    out.push_str(&format!(
        "    <DT><H3{toolbar}>{}</H3>\n    <DL><p>\n",
        encode(&group.name)
    ));
    for b in &group.bookmarks {
        out.push_str(&format!(
            "        <DT><A HREF=\"{}\">{}</A>\n",
            encode(b.url.as_str()),
            encode(&b.title)
        ));
    }
    out.push_str("    </DL><p>\n");
}

/// The group a link at this depth belongs to: the open folders' names
/// joined, or `loose` at the root.
fn group_name(stack: &[Option<String>], loose: &str) -> String {
    let names: Vec<&str> = stack
        .iter()
        .filter_map(|name| name.as_deref())
        .filter(|name| !name.is_empty())
        .collect();
    if names.is_empty() {
        return loose.to_string();
    }
    // The toolbar folder keeps its name whatever it is nested in; its
    // own subfolders hang off it.
    if let Some(i) = names.iter().position(|n| *n == Bookmarks::BAR) {
        return names[i..].join(" / ");
    }
    names.join(" / ")
}

/// The text up to the closing tag `close` (matched without regard to
/// case) and what follows it; to the end without one.
fn text_until<'a>(text: &'a str, close: &str) -> (&'a str, &'a str) {
    let lower = text.to_ascii_lowercase();
    match lower.find(close) {
        Some(i) => (&text[..i], &text[i + close.len()..]),
        None => (text, ""),
    }
}

/// The value of `name` in a tag's attributes, quoted or bare.
fn attribute(tag: &str, name: &str) -> Option<String> {
    let upper = tag.to_ascii_uppercase();
    let mut from = 0;
    while let Some(i) = upper[from..].find(name) {
        let at = from + i;
        let before_ok = at == 0 || !upper.as_bytes()[at - 1].is_ascii_alphanumeric();
        let after = at + name.len();
        let rest = tag[after..].trim_start();
        if before_ok && let Some(value) = rest.strip_prefix('=') {
            let value = value.trim_start();
            return Some(match value.chars().next() {
                Some(q @ ('"' | '\'')) => {
                    let inner = &value[1..];
                    inner[..inner.find(q).unwrap_or(inner.len())].to_string()
                }
                _ => value
                    .split(|c: char| c.is_whitespace())
                    .next()
                    .unwrap_or("")
                    .to_string(),
            });
        }
        from = after;
    }
    None
}

/// Undo HTML's character references: the named ones files use and the
/// numeric ones.
fn decode(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let after = &rest[i..];
        let Some(end) = after.find(';').filter(|&e| e <= 10) else {
            out.push('&');
            rest = &after[1..];
            continue;
        };
        let entity = &after[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            _ => entity
                .strip_prefix('#')
                .and_then(|n| match n.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => n.parse().ok(),
                })
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &after[end + 1..];
            }
            None => {
                out.push('&');
                rest = &after[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHROME_EXPORT: &str = r#"<!DOCTYPE NETSCAPE-Bookmark-file-1>
<!-- This is an automatically generated file.
     It will be read and overwritten.
     DO NOT EDIT! -->
<META HTTP-EQUIV="Content-Type" CONTENT="text/html; charset=UTF-8">
<TITLE>Bookmarks</TITLE>
<H1>Bookmarks</H1>
<DL><p>
    <DT><H3 ADD_DATE="1700000000" LAST_MODIFIED="1700000001" PERSONAL_TOOLBAR_FOLDER="true">Bookmarks bar</H3>
    <DL><p>
        <DT><A HREF="https://a.example/" ADD_DATE="1700000000" ICON="data:image/png;base64,AAAA">A &amp; Co</A>
        <DT><H3 ADD_DATE="1700000000">Reading</H3>
        <DL><p>
            <DT><A HREF="https://r.example/?x=1&amp;y=2">R&#39;s page</A>
        </DL><p>
    </DL><p>
    <DT><H3 ADD_DATE="1700000000">Other bookmarks</H3>
    <DL><p>
        <DT><A HREF="https://o.example/">Other</A>
        <DT><A HREF="not a url">Broken</A>
    </DL><p>
    <DT><A HREF="https://loose.example/"></A>
</DL><p>
"#;

    fn titles(b: &Bookmarks, group: &str) -> Vec<(String, String)> {
        b.group(group)
            .map(|g| {
                g.bookmarks
                    .iter()
                    .map(|x| (x.title.clone(), x.url.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn imports_folders_the_toolbar_and_loose_links() {
        let b = import(CHROME_EXPORT, "Imported");
        assert_eq!(
            b.group_names(),
            vec![
                Bookmarks::BAR,
                "Bookmarks bar / Reading",
                "Other bookmarks",
                "Imported"
            ]
        );
        assert_eq!(
            titles(&b, Bookmarks::BAR),
            vec![("A & Co".to_string(), "https://a.example/".to_string())]
        );
        assert_eq!(
            titles(&b, "Bookmarks bar / Reading"),
            vec![(
                "R's page".to_string(),
                "https://r.example/?x=1&y=2".to_string()
            )]
        );
        // The unparsable address is skipped; the untitled link is titled
        // by its address.
        assert_eq!(
            titles(&b, "Other bookmarks"),
            vec![("Other".to_string(), "https://o.example/".to_string())]
        );
        assert_eq!(
            titles(&b, "Imported"),
            vec![(
                "https://loose.example/".to_string(),
                "https://loose.example/".to_string()
            )]
        );
    }

    #[test]
    fn tags_match_without_regard_to_case_and_quotes() {
        let html = "<dl><p><dt><h3 personal_toolbar_folder=true>Bar</h3><dl><p>\
                    <dt><a href='https://x.example/'>X</a></dl></dl>";
        let b = import(html, "Loose");
        assert_eq!(b.group_names(), vec![Bookmarks::BAR]);
        assert_eq!(
            titles(&b, Bookmarks::BAR),
            vec![("X".to_string(), "https://x.example/".to_string())]
        );
        assert_eq!(
            import("<html>nothing here</html>", "Loose"),
            Bookmarks::default()
        );
        assert_eq!(import("", "Loose"), Bookmarks::default());
    }

    #[test]
    fn export_round_trips_and_marks_the_bar() {
        let mut b = Bookmarks::default();
        b.add(
            Bookmarks::BAR,
            vec![Bookmark {
                title: "A <b> & \"c\"".to_string(),
                url: Url::parse("https://a.example/?q=1&r=2").unwrap(),
            }],
        );
        b.add(
            "Work",
            vec![Bookmark {
                title: "W".to_string(),
                url: Url::parse("https://w.example/").unwrap(),
            }],
        );
        let html = export(&b);
        assert!(
            html.starts_with("<!DOCTYPE NETSCAPE-Bookmark-file-1>"),
            "{html}"
        );
        assert!(
            html.contains("<DT><H3 PERSONAL_TOOLBAR_FOLDER=\"true\">Bookmarks bar</H3>"),
            "{html}"
        );
        assert!(html.contains("<DT><H3>Work</H3>"), "{html}");
        assert!(
            html.contains(
                "<A HREF=\"https://a.example/?q=1&amp;r=2\">A &lt;b&gt; &amp; &quot;c&quot;</A>"
            ),
            "{html}"
        );
        assert_eq!(import(&html, "Loose"), b);
    }

    #[test]
    fn character_references() {
        assert_eq!(
            decode("a &amp; b &lt;c&gt; &#65;&#x42; &unknown; & x"),
            "a & b <c> AB &unknown; & x"
        );
        assert_eq!(decode("plain"), "plain");
        assert_eq!(
            encode("a & <b> \"c\" 'd'"),
            "a &amp; &lt;b&gt; &quot;c&quot; 'd'"
        );
    }

    #[test]
    fn attributes_are_found_by_name_only() {
        assert_eq!(
            attribute("A HREF=\"https://x/\" DATA-HREF=\"no\"", "HREF").as_deref(),
            Some("https://x/")
        );
        assert_eq!(attribute("A XHREF=\"no\"", "HREF"), None);
        assert_eq!(attribute("H3 FOO = bar baz", "FOO").as_deref(), Some("bar"));
        assert_eq!(attribute("H3", "FOO"), None);
    }
}
