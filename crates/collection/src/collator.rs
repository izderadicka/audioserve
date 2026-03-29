use regex::Regex;
use std::{cmp::Ordering, sync::LazyLock};

use crate::{AudioFile, AudioFolderShort};

static NUMBER_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d{1,10}").unwrap());

pub(crate) trait Collate<T = Self> {
    fn collate(&self, other: &T) -> Ordering;
    fn collate_natural(&self, other: &T) -> Ordering {
        self.collate(other)
    }
}

/// Assuming digits were already found at beginning of name
fn split_name(name: &str) -> Option<(&str, u32, &str)> {
    let num = NUMBER_RE.find(name);
    match num {
        Some(num) => {
            let pos: u32 = num
                .as_str()
                .parse()
                .map_err(|_e| debug!("Cannot parse number {} in name {}", num.as_str(), name))
                .ok()?;
            let rest = &name[num.end()..];
            let prefix = &name[..num.start()];
            Some((prefix, pos, rest))
        }
        None => None,
    }
}

fn cmp_natural(me: &str, other: &str, compare: impl Fn(&str, &str) -> Ordering) -> Ordering {
    if let Some((my_prefix, my_pos, my_rest)) = split_name(me) {
        if let Some((other_prefix, other_pos, other_rest)) = split_name(other) {
            if my_prefix == other_prefix {
                return match my_pos.cmp(&other_pos) {
                    Ordering::Equal => compare(my_rest, other_rest),
                    other => other,
                };
            }
        }
    }

    compare(me, other)
}

#[cfg(not(feature = "collation"))]
pub(crate) mod standard {
    use super::*;

    impl Collate for AudioFile {
        fn collate(&self, other: &AudioFile) -> Ordering {
            self.name.cmp(&other.name)
        }

        fn collate_natural(&self, other: &Self) -> Ordering {
            cmp_natural(&self.name, &other.name, |a, b| a.cmp(b))
        }
    }

    impl Collate for AudioFolderShort {
        fn collate(&self, other: &AudioFolderShort) -> Ordering {
            self.name.cmp(&other.name)
        }

        fn collate_natural(&self, other: &Self) -> Ordering {
            cmp_natural(&self.name, &other.name, |a, b| a.cmp(b))
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_natural_order() {
            let mut terms = ["10 - v deset", "2 - dve", "10 - deset", "3 - tri"];
            terms.sort_by(|a, b| cmp_natural(a, b, |a, b| a.cmp(b)));
            assert_eq!("2 - dve", terms[0]);
            assert_eq!("3 - tri", terms[1]);
            assert_eq!("10 - deset", terms[2]);
            assert_eq!("10 - v deset", terms[3]);
        }

        #[test]
        fn test_natural_order_with_prefix() {
            let mut terms = ["Chapter 10", "Chapter 3", "Chapter 20", "Chapter 1"];
            terms.sort_unstable_by(|a, b| cmp_natural(a, b, |a, b| a.cmp(b)));
            assert_eq!("Chapter 1", terms[0]);
            assert_eq!("Chapter 3", terms[1]);
            assert_eq!("Chapter 10", terms[2]);
            assert_eq!("Chapter 20", terms[3]);
        }
    }
}

#[cfg(feature = "collation")]
pub(crate) mod locale {
    use std::sync::LazyLock;

    use super::*;
    use icu::collator::{
        options::{CollatorOptions, Strength},
        CollatorBorrowed as UCollator,
    };
    use icu::locale::Locale;

    static LOCALE_COLLATOR: LazyLock<Collator> = LazyLock::new(|| Collator::new());

    fn normalize_posix_locale(input: &str) -> String {
        let base = input
            .split('.')
            .next()
            .unwrap_or(input)
            .split('@')
            .next()
            .unwrap_or(input);

        base.replace('_', "-")
    }

    struct Collator(UCollator<'static>);

    impl Collator {
        pub(crate) fn collate<A, B>(&self, a: A, b: B) -> Ordering
        where
            A: AsRef<str>,
            B: AsRef<str>,
        {
            self.0.compare(a.as_ref(), b.as_ref())
        }

        pub(crate) fn new_from_locale(locale: &str) -> Result<Self, crate::error::Error> {
            let locale = normalize_posix_locale(locale);
            let locale: Locale = locale.parse()?;
            let mut options = CollatorOptions::default();
            options.strength = Some(Strength::Secondary);
            let collator = UCollator::try_new(locale.into(), options)
                .map_err(|e| crate::error::Error::InvalidCollationData(e.to_string()))?;

            Ok(Collator(collator))
        }

        #[cfg(not(test))]
        pub(crate) fn new() -> Self {
            use std::env;
            let locale = env::var("AUDIOSERVE_COLLATE")
                .or_else(|_| env::var("LC_ALL"))
                .or_else(|_| env::var("LC_COLLATE"))
                .or_else(|_| env::var("LANG"))
                .unwrap_or("en_US".into());

            info!("Using locale {} for Collator", locale);

            Collator::new_from_locale(&locale).expect("Cannot create Collator")
        }

        #[cfg(test)]
        pub(crate) fn new() -> Self {
            Collator::new_from_locale("cs").expect("Cannot create UCollator")
        }
    }

    impl Collate for AudioFile {
        fn collate(&self, other: &AudioFile) -> Ordering {
            LOCALE_COLLATOR.collate(&self.name, &other.name)
        }

        fn collate_natural(&self, other: &Self) -> Ordering {
            cmp_natural(&self.name, &other.name, |a, b| {
                LOCALE_COLLATOR.collate(a, b)
            })
        }
    }

    impl Collate for AudioFolderShort {
        fn collate(&self, other: &AudioFolderShort) -> Ordering {
            LOCALE_COLLATOR.collate(&self.name, &other.name)
        }

        fn collate_natural(&self, other: &Self) -> Ordering {
            cmp_natural(&self.name, &other.name, |a, b| {
                LOCALE_COLLATOR.collate(a, b)
            })
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn we_have_czech_language() {
            use icu::locale::locale;
            let _locale: icu::locale::Locale = "cs".parse().expect("Should have cs");
            let _locale: icu::locale::Locale = "cs-CZ".parse().expect("Should have cs_CZ");
            let locale: icu::locale::Locale = normalize_posix_locale("cs_CZ.UTF-8")
                .parse()
                .expect("Should have cs_CZ.UTF-8");

            assert_eq!(locale!("cs-CZ"), locale);
        }

        #[test]
        fn sort_unicode() {
            let phrase = "Přílíšžluťoučkýkůňúpělďábelkéódy";
            let mut letters = phrase.chars().map(|c| c.to_string()).collect::<Vec<_>>();
            letters.sort_unstable_by(|a, b| LOCALE_COLLATOR.collate(a, b));
            let sorted = letters.join("");
            assert_eq!("ábčdďeéěííkkkllllňoóPpřšťuuúůyýž", sorted);
        }

        #[test]
        fn sort_czech() {
            let mut words = vec!["Široký", "Sýpka"];
            let correct = vec!["Sýpka", "Široký"];
            words.sort_unstable_by(|a, b| LOCALE_COLLATOR.collate(a, b));
            assert_eq!(correct, words);
        }
    }
}
