use std::collections::HashMap;

pub(crate) fn format(
    template: &str,
    vars: &HashMap<String, String>,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<String, String> {
    let bytes = template.as_bytes();
    let mut out = String::with_capacity(template.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'$' {
            let start = i;
            while i < bytes.len() && bytes[i] != b'$' {
                i += 1;
            }
            out.push_str(&template[start..i]);
            continue;
        }
        i += 1; // past '$'
        match bytes.get(i) {
            Some(b'$') => {
                out.push('$');
                i += 1;
            }
            Some(b'{') => {
                let end = template[i..]
                    .find('}')
                    .map(|off| i + off)
                    .ok_or_else(|| format!("unclosed placeholder: {}", &template[i - 1..]))?;
                let inner = &template[i + 1..end];
                if let Some(name) = inner.strip_prefix('$') {
                    let value = env(name)
                        .ok_or_else(|| format!("environment variable ${name} is not set"))?;
                    out.push_str(&value);
                } else if inner.is_empty() {
                    return Err("empty placeholder ${}".to_string());
                } else {
                    let value = vars
                        .get(inner)
                        .ok_or_else(|| format!("unknown variable ${{{inner}}}"))?;
                    out.push_str(value);
                }
                i = end + 1;
            }
            Some(&c) if c == b'_' || c.is_ascii_alphabetic() => {
                let start = i;
                while i < bytes.len() && (bytes[i] == b'_' || bytes[i].is_ascii_alphanumeric()) {
                    i += 1;
                }
                let name = &template[start..i];
                let value =
                    env(name).ok_or_else(|| format!("environment variable ${name} is not set"))?;
                out.push_str(&value);
            }
            _ => {
                return Err("stray '$' (use $$ for a literal dollar sign)".to_string());
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn env(name: &str) -> Option<String> {
        match name {
            "HOME" => Some("stub-home".to_string()),
            _ => None,
        }
    }

    #[test]
    fn braced_names_come_from_vars() {
        let got = format("build ${target} now", &vars(&[("target", "web")]), &env).unwrap();
        assert_eq!(got, "build web now");
    }

    #[test]
    fn missing_var_is_an_error() {
        let err = format("${nope}", &vars(&[]), &env).unwrap_err();
        assert!(err.contains("unknown variable"), "got: {err}");
    }

    #[test]
    fn bare_dollar_names_come_from_env_only() {
        assert_eq!(
            format("cd $HOME", &vars(&[]), &env).unwrap(),
            "cd stub-home"
        );
        // a var with the same name must NOT satisfy an env placeholder
        let err = format("$TOKEN", &vars(&[("TOKEN", "from-vars")]), &env).unwrap_err();
        assert!(err.contains("environment variable"), "got: {err}");
    }

    #[test]
    fn braced_env_form_works() {
        assert_eq!(format("${$HOME}x", &vars(&[]), &env).unwrap(), "stub-homex");
        let err = format("${$NOPE}", &vars(&[]), &env).unwrap_err();
        assert!(err.contains("$NOPE is not set"), "got: {err}");
    }

    #[test]
    fn double_dollar_is_a_literal() {
        assert_eq!(format("cost: 5$$", &vars(&[]), &env).unwrap(), "cost: 5$");
        assert_eq!(format("$$HOME", &vars(&[]), &env).unwrap(), "$HOME");
    }

    #[test]
    fn stray_dollar_is_an_error() {
        for t in ["$", "$ x", "$1"] {
            let err = format(t, &vars(&[]), &env).unwrap_err();
            assert!(err.contains("stray '$'"), "template {t:?} got: {err}");
        }
    }

    #[test]
    fn unclosed_and_empty_braces_are_errors() {
        assert!(format("${oops", &vars(&[]), &env).is_err());
        assert!(format("${}", &vars(&[]), &env).is_err());
    }

    #[test]
    fn plain_text_passes_through() {
        assert_eq!(
            format("no placeholders", &vars(&[]), &env).unwrap(),
            "no placeholders"
        );
    }
}
