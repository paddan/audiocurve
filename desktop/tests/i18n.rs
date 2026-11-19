use audiocurve_app::i18n::{Lang, lang, set_lang, tr, trf};
use std::sync::Mutex;

static LOCK: Mutex<()> = Mutex::new(());

const CATALOG: &str = include_str!("../strings/Localizable.xcstrings");
const NON_ENGLISH: [&str; 7] = ["sv", "da", "nb", "fi", "de", "fr", "es"];

fn catalog() -> serde_json::Value {
    serde_json::from_str(CATALOG).unwrap()
}

fn value(key: &str, lang: &str) -> String {
    catalog()["strings"][key]["localizations"][lang]["stringUnit"]["value"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn swedish_lookup() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    set_lang(Lang::Sv);
    assert_eq!(lang(), Lang::Sv);
    assert_eq!(tr("Active on"), value("Active on", "sv"));
    set_lang(Lang::En);
}

#[test]
fn every_language_resolves_every_translated_key() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let cat = catalog();
    let langs = [
        (Lang::Sv, "sv"),
        (Lang::Da, "da"),
        (Lang::Nb, "nb"),
        (Lang::Fi, "fi"),
        (Lang::De, "de"),
        (Lang::Fr, "fr"),
        (Lang::Es, "es"),
    ];
    for (l, code) in langs {
        set_lang(l);
        for (key, entry) in cat["strings"].as_object().unwrap() {
            if let Some(v) = entry["localizations"][code]["stringUnit"]["value"].as_str() {
                assert_eq!(tr(key), v, "{code}: {key}");
            }
        }
    }
    set_lang(Lang::En);
}

#[test]
fn missing_key_falls_back_to_english() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    set_lang(Lang::De);
    assert_eq!(tr("No such string 123"), "No such string 123");
    assert_eq!(trf("No such %@ 123", &["x"]), "No such x 123");
    set_lang(Lang::En);
}

#[test]
fn format_arguments() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    set_lang(Lang::En);
    assert_eq!(
        trf("Profile for %@", &["AirPods Pro"]),
        "Profile for AirPods Pro"
    );
    assert_eq!(
        trf("At least %lld points per ear", &["3"]),
        "At least 3 points per ear"
    );
    assert_eq!(trf("%lld %%", &["50"]), "50 %");
    assert_eq!(trf("%2$@ then %1$@", &["a", "b"]), "b then a");
    set_lang(Lang::Sv);
    assert_eq!(
        trf("Active on: %lld outputs", &["2"]),
        value("Active on: %lld outputs", "sv").replace("%lld", "2")
    );
    set_lang(Lang::En);
}

#[test]
fn locale_tags() {
    assert_eq!(Lang::from_locale("sv-SE"), Lang::Sv);
    assert_eq!(Lang::from_locale("nb-NO"), Lang::Nb);
    assert_eq!(Lang::from_locale("no"), Lang::Nb);
    assert_eq!(Lang::from_locale("pt-BR"), Lang::En);
    assert_eq!(Lang::from_locale("de_AT"), Lang::De);
}

#[test]
fn every_key_has_all_eight_languages_or_english_only() {
    for (key, entry) in catalog()["strings"].as_object().unwrap() {
        let locs = entry["localizations"].as_object();
        let has_any = locs.is_some_and(|l| l.keys().any(|k| k != "en"));
        if has_any {
            for code in NON_ENGLISH {
                assert!(locs.unwrap().contains_key(code), "{key} lacks {code}");
            }
        }
    }
}

#[test]
fn new_strings_exist() {
    for key in [
        "The sound processing stopped",
        "Try Again",
        "AudioCurve on",
        "Settings…",
    ] {
        assert!(catalog()["strings"][key].is_object(), "{key}");
    }
}

/// The capture prompt addresses the user formally in German, as the rest of the app does.
#[test]
fn the_german_capture_prompt_is_formal() {
    let de = value(
        "AudioCurve processes the sound your Mac plays so it matches your hearing.",
        "de",
    );
    assert_eq!(
        de,
        "AudioCurve bearbeitet den Ton, den Ihr Mac wiedergibt, damit er zu Ihrem Gehör passt."
    );
}
