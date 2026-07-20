//! Project `.env` parsing.

use std::collections::HashMap;

pub(crate) use dotenvy::Error;

pub(crate) fn parse(contents: &str) -> Result<HashMap<String, String>, Error> {
    dotenvy::from_read_iter(contents.as_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pairs_comments_and_blanks() {
        let env = parse("# a comment\n\nFOO=bar\nBAZ=qux\n").unwrap();
        assert_eq!(env.get("FOO"), Some(&"bar".to_string()));
        assert_eq!(env.get("BAZ"), Some(&"qux".to_string()));
        assert_eq!(env.len(), 2);
    }

    #[test]
    fn handles_export_prefix_and_quotes() {
        let env = parse("export TOKEN=\"secret value\"\nLITERAL='no $expansion'\n").unwrap();
        assert_eq!(env.get("TOKEN"), Some(&"secret value".to_string()));
        assert_eq!(env.get("LITERAL"), Some(&"no $expansion".to_string()));
    }

    #[test]
    fn substitutes_from_earlier_file_var() {
        let env = parse("HOST=example.com\nURL=${HOST}/api\n").unwrap();
        assert_eq!(env.get("URL"), Some(&"example.com/api".to_string()));
    }

    #[test]
    fn substitutes_from_process_env() {
        let path = std::env::var("PATH").unwrap();
        let env = parse("REF=${PATH}\n").unwrap();
        assert_eq!(env.get("REF"), Some(&path));
    }

    #[test]
    fn malformed_line_is_an_error() {
        assert!(parse("KEY=\"unterminated\n").is_err());
    }
}
