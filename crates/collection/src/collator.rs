use lazy_static::lazy_static;
use regex::Regex;
use std::cmp::Ordering;

use crate::{AudioFile, AudioFolderShort};

lazy_static! {
    static ref LEADING_NUMBER_RE: Regex = Regex::new(r"^\d+").unwrap();
}

pub(crate) trait Collate<T = Self> {
    fn collate(&self, other: &T) -> Ordering;
    fn collate_natural(&self, other: &T) -> Ordering {
        self.collate(other)
    }
}

/// Assuming digits were already found at beginning of name
fn split_name(name: &str) -> (u32, &str) {
    let prefix = LEADING_NUMBER_RE.find(name).unwrap();
    let pos: u32 = prefix.as_str().parse().unwrap();
    let rest = &name[prefix.end()..];
    (pos, rest)
}

#[cfg(not(any(feature = "collation", feature = "collation-static")))]
pub(crate) mod standard {
    use super::*;

    fn cmp_natural(me: &str, other: &str) -> Ordering {
        if LEADING_NUMBER_RE.is_match(me) && LEADING_NUMBER_RE.is_match(other) {
            let (my_pos, my_rest) = split_name(me);
            let (other_pos, other_rest) = split_name(other);

            match my_pos.cmp(&other_pos) {
                Ordering::Equal => my_rest.cmp(other_rest),
                other => other,
            }
        } else {
            me.cmp(other)
        }
    }

    impl Collate for AudioFile {
        fn collate(&self, other: &AudioFile) -> Ordering {
            self.name.cmp(&other.name)
        }

        fn collate_natural(&self, other: &Self) -> Ordering {
            cmp_natural(&self.name, &other.name)
        }
    }

    impl Collate for AudioFolderShort {
        fn collate(&self, other: &AudioFolderShort) -> Ordering {
            self.name.cmp(&other.name)
        }

        fn collate_natural(&self, other: &Self) -> Ordering {
            cmp_natural(&self.name, &other.name)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_natural_order() {
            let mut terms = vec!["10 - v deset", "2 - dve", "10 - deset", "3 - tri"];
            terms.sort_by(|a, b| cmp_natural(a, b));
            assert_eq!("2 - dve", terms[0]);
            assert_eq!("3 - tri", terms[1]);
            assert_eq!("10 - deset", terms[2]);
            assert_eq!("10 - v deset", terms[3]);
        }
    }
}

#[cfg(any(feature = "collation", feature = "collation-static"))]
pub(crate) mod locale {
    use super::*;
    use lazy_static::lazy_static;
    use rust_icu_ucol::UCollator;
    use std::convert::TryFrom;

    lazy_static! {
        static ref LOCALE_COLLATOR: Collator = Collator::new();
    }

    fn cmp_natural(me: &str, other: &str) -> Ordering {
        if LEADING_NUMBER_RE.is_match(me) && LEADING_NUMBER_RE.is_match(other) {
            let (my_pos, my_rest) = split_name(me);
            let (other_pos, other_rest) = split_name(other);

            match my_pos.cmp(&other_pos) {
                Ordering::Equal => LOCALE_COLLATOR.collate(my_rest, other_rest),
                other => other,
            }
        } else {
            LOCALE_COLLATOR.collate(me, other)
        }
    }

    struct Collator(UCollator);

    // According to ICU documentation C implementation should be thread safe for ucol_strcoll methods
    // See https://unicode-org.github.io/icu/userguide/icu/design.html#thread-safe-const-apis
    // Use of recent ICU library is assumed
    unsafe impl Sync for Collator {}

    impl Collator {
        pub(crate) fn collate<A, B>(&self, a: A, b: B) -> Ordering
        where
            A: AsRef<str>,
            B: AsRef<str>,
        {
            self.0
                .strcoll_utf8(a.as_ref(), b.as_ref())
                .unwrap_or_else(|e| {
                    error!("Collation error {}", e);
                    Ordering::Greater
                })
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

            let col = UCollator::try_from(locale.as_str()).expect("Cannot create UCollator");
            Collator(col)
        }

        #[cfg(test)]
        pub(crate) fn new() -> Self {
            let col = UCollator::try_from("cs_CZ").expect("Cannot create UCollator");
            Collator(col)
        }
    }

    impl Collate for AudioFile {
        fn collate(&self, other: &AudioFile) -> Ordering {
            LOCALE_COLLATOR.collate(&self.name, &other.name)
        }

        fn collate_natural(&self, other: &Self) -> Ordering {
            cmp_natural(&self.name, &other.name)
        }
    }

    impl Collate for AudioFolderShort {
        fn collate(&self, other: &AudioFolderShort) -> Ordering {
            LOCALE_COLLATOR.collate(&self.name, &other.name)
        }

        fn collate_natural(&self, other: &Self) -> Ordering {
            cmp_natural(&self.name, &other.name)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn we_have_czech_language() {
            let locales = rust_icu_ucol::get_available_locales().expect("cannot get locales");
            assert!(locales
                .map(|i| i.expect("invalid string"))
                .find(|loc| loc == "cs")
                .is_some());
        }

        #[test]
        fn sort_unicode() {
            let phrase = "Přílíšžluťoučkýkůňúpělďábelkéódy";
            let mut letters = phrase.chars().map(|c| c.to_string()).collect::<Vec<_>>();
            letters.sort_unstable_by(|a, b| LOCALE_COLLATOR.collate(a, b));
            let sorted = letters.join("");
            assert_eq!("ábčdďeéěííkkkllllňoópPřšťuuúůyýž", sorted);
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
