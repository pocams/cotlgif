use once_cell::sync::OnceCell;
use regex::Regex;

pub fn slugify_string(s: &str) -> String {
    static NON_ALPHA: OnceCell<Regex> = OnceCell::new();
    static LOWER_UPPER: OnceCell<Regex> = OnceCell::new();
    let non_alpha = NON_ALPHA.get_or_init(|| Regex::new(r"[^A-Za-z0-9]").unwrap());
    let lower_upper = LOWER_UPPER.get_or_init(|| Regex::new(r"([a-z])([A-Z])").unwrap());
    let s = non_alpha.replace(s, "-");
    let s = lower_upper.replace(&s, "$2-$1");
    s.to_ascii_lowercase()
}
