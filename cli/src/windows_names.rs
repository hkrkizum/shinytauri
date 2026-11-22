use crate::Result;
use std::collections::BTreeMap;
use std::sync::LazyLock;

// Shared with the R bundler, including when cross-compiling on Linux.
static UPPERCASE: LazyLock<BTreeMap<char, char>> = LazyLock::new(|| {
    let mut map = BTreeMap::new();
    for line in include_str!("../../scripts/windows-name-case.txt").lines() {
        if line.starts_with('#') {
            continue;
        }
        let range: Vec<i32> = line
            .split_ascii_whitespace()
            .map(|n| n.parse().unwrap())
            .collect();
        let [first, last, step, delta] = range[..] else {
            panic!("Invalid Windows case table")
        };
        for code in (first..=last).step_by(step as usize) {
            map.insert(
                char::from_u32(code as u32).unwrap(),
                char::from_u32((code + delta) as u32).unwrap(),
            );
        }
    }
    map
});

pub(crate) fn name_key(name: &str) -> String {
    name.chars()
        .map(|c| UPPERCASE.get(&c).copied().unwrap_or(c))
        .collect()
}

fn invalid_component(name: &str, key: &str) -> bool {
    let stem = key.split('.').next().unwrap().trim_end_matches(' ');
    name.is_empty()
        || name.ends_with([' ', '.'])
        || name
            .chars()
            .any(|c| c <= '\u{1f}' || "<>:\"\\|?*".contains(c))
        || name.encode_utf16().count() > 255
        || matches!(key, "CONIN$" | "CONOUT$")
        || matches!(stem, "CON" | "PRN" | "AUX" | "NUL")
        || stem
            .strip_prefix("COM")
            .or_else(|| stem.strip_prefix("LPT"))
            .is_some_and(|n| {
                matches!(
                    n,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
}

pub(crate) fn check<'a>(paths: impl IntoIterator<Item = &'a str>) -> Result<()> {
    let mut seen = BTreeMap::new();
    for path in paths {
        let mut key = String::new();
        let mut end = 0;
        for part in path.split('/') {
            let folded = name_key(part);
            end += part.len();
            let spelling = &path[..end];
            if invalid_component(part, &folded) {
                return Err(format!("Invalid Windows app resource name: {spelling:?}").into());
            }
            key.push_str(&folded);
            if let Some(first) = seen.insert(key.clone(), spelling)
                && first != spelling
            {
                return Err(format!("App resource paths {first:?} and {spelling:?} collide on Windows. Keep one spelling.").into());
            }
            key.push('/');
            end += 1;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_windows_case_comparison() {
        for (left, right, same) in CASES {
            assert_eq!(check([left, right]).is_err(), same, "{left:?} {right:?}");
        }
    }

    const CASES: [(&str, &str, bool); 10] = [
        ("É", "é", true),
        ("ᾈ", "ᾀ", true),
        ("Σ", "σ", true),
        ("Σ", "ς", false),
        ("ß", "SS", false),
        ("İ", "i", false),
        ("I", "ı", false),
        ("é", "e\u{301}", false),
        ("𐐀", "𐐨", false),
        ("K", "k", false),
    ];

    #[cfg(windows)]
    #[test]
    fn native_windows_creates_the_same_distinct_names() {
        use std::fs::File;
        use std::io::ErrorKind;
        for (left, right, _) in CASES {
            let root = tempfile::tempdir().unwrap();
            File::create_new(root.path().join(left)).unwrap();
            let same_file = match File::create_new(root.path().join(right)) {
                Ok(_) => false,
                Err(e) if e.kind() == ErrorKind::AlreadyExists => true,
                Err(e) => panic!("Cannot check native Windows paths: {e}"),
            };
            assert_eq!(
                check([left, right]).is_err(),
                same_file,
                "{left:?} {right:?}"
            );
        }
    }
}
