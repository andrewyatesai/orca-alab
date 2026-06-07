//! Decoder for git's C-style quoted pathnames, ported from
//! `src/shared/git-cquoted-path.ts`.
//!
//! When `core.quotePath` is on, git wraps paths containing "unusual" bytes in
//! double quotes and escapes them (`\n`, `\t`, octal `\NNN`, …). Orca parses
//! porcelain output that may contain these, so the decode must match the TS
//! implementation exactly — including its byte/charcode treatment of octal
//! escapes (`char::from_u32`, mirroring `String.fromCharCode`).

pub fn decode_git_cquoted_path(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let n = chars.len();
    if n < 2 || chars[0] != '"' || chars[n - 1] != '"' {
        return value.to_string();
    }

    let mut decoded = String::new();
    let mut index = 1;
    while index < n - 1 {
        let ch = chars[index];
        if ch != '\\' {
            decoded.push(ch);
            index += 1;
            continue;
        }

        index += 1;
        if index >= n {
            break;
        }
        let escaped = chars[index];
        match escaped {
            'a' => decoded.push('\u{0007}'),
            'b' => decoded.push('\u{0008}'),
            'f' => decoded.push('\u{000C}'),
            'n' => decoded.push('\n'),
            'r' => decoded.push('\r'),
            't' => decoded.push('\t'),
            'v' => decoded.push('\u{000B}'),
            '\\' | '"' => decoded.push(escaped),
            c if ('0'..='7').contains(&c) => {
                let mut octal = String::new();
                octal.push(c);
                while index + 1 < n - 1 && octal.len() < 3 && chars[index + 1].is_digit(8) {
                    index += 1;
                    octal.push(chars[index]);
                }
                if let Some(decoded_ch) =
                    u32::from_str_radix(&octal, 8).ok().and_then(char::from_u32)
                {
                    decoded.push(decoded_ch);
                }
            }
            other => decoded.push(other),
        }
        index += 1;
    }
    decoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_unquoted_input_unchanged() {
        assert_eq!(decode_git_cquoted_path("src/index.ts"), "src/index.ts");
        assert_eq!(decode_git_cquoted_path(""), "");
        assert_eq!(decode_git_cquoted_path("\""), "\"");
    }

    #[test]
    fn strips_surrounding_quotes_for_plain_quoted_paths() {
        assert_eq!(decode_git_cquoted_path("\"src/index.ts\""), "src/index.ts");
    }

    #[test]
    fn decodes_named_escapes() {
        assert_eq!(decode_git_cquoted_path("\"a\\tb\""), "a\tb");
        assert_eq!(decode_git_cquoted_path("\"a\\nb\""), "a\nb");
        assert_eq!(decode_git_cquoted_path("\"a\\\\b\""), "a\\b");
        assert_eq!(decode_git_cquoted_path("\"a\\\"b\""), "a\"b");
    }

    #[test]
    fn decodes_octal_escapes() {
        // git quotes a UTF-8 "é" (0xC3 0xA9) as \303\251.
        assert_eq!(
            decode_git_cquoted_path("\"caf\\303\\251.txt\""),
            "caf\u{00c3}\u{00a9}.txt"
        );
        // A single octal byte.
        assert_eq!(decode_git_cquoted_path("\"\\101\""), "A");
    }
}
