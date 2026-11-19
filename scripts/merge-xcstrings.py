#!/usr/bin/env python3
"""Merge two String Catalogs (xcstrings) into one: merge-xcstrings.py IN1 IN2 OUT.

On a key both files have, the entry from IN2 wins. Strings added for the egui app are in
EXTRA below and are applied last. The app's catalog started as the Swift app's catalog
merged with the AU's; now that the Swift app is gone, regenerate it in place:

    python3 scripts/merge-xcstrings.py desktop/strings/Localizable.xcstrings \
        AU/Localizable.xcstrings desktop/strings/Localizable.xcstrings
"""
import json
import sys

LANGS = ["sv", "da", "nb", "fi", "de", "fr", "es"]

EXTRA = {
    # The macOS app menu (desktop/src/platform/macos/app_menu.rs), in Apple's own wording.
    "About AudioCurve": dict(zip(LANGS, [
        "Om AudioCurve", "Om AudioCurve", "Om AudioCurve", "Tietoja: AudioCurve",
        "Über AudioCurve", "À propos de AudioCurve", "Acerca de AudioCurve",
    ])),
    "Hide AudioCurve": dict(zip(LANGS, [
        "Göm AudioCurve", "Skjul AudioCurve", "Skjul AudioCurve", "Kätke AudioCurve",
        "AudioCurve ausblenden", "Masquer AudioCurve", "Ocultar AudioCurve",
    ])),
    "Hide Others": dict(zip(LANGS, [
        "Göm övriga", "Skjul andre", "Skjul andre", "Kätke muut",
        "Andere ausblenden", "Masquer les autres", "Ocultar otros",
    ])),
    "Show All": dict(zip(LANGS, [
        "Visa alla", "Vis alle", "Vis alle", "Näytä kaikki",
        "Alle einblenden", "Tout afficher", "Mostrar todo",
    ])),
    "Window": dict(zip(LANGS, [
        "Fönster", "Vindue", "Vindu", "Ikkuna", "Fenster", "Fenêtre", "Ventana",
    ])),
    "Minimize": dict(zip(LANGS, [
        "Minimera", "Minimer", "Minimer", "Pienennä",
        "Im Dock ablegen", "Placer dans le Dock", "Minimizar",
    ])),
    "Close": dict(zip(LANGS, [
        "Stäng", "Luk", "Lukk", "Sulje", "Schließen", "Fermer", "Cerrar",
    ])),
    "The sound processing stopped": dict(zip(LANGS, [
        "Ljudbehandlingen stoppade",
        "Lydbehandlingen stoppede",
        "Lydbehandlingen stoppet",
        "Äänenkäsittely pysähtyi",
        "Die Klangverarbeitung wurde angehalten",
        "Le traitement du son s’est arrêté",
        "El procesamiento de sonido se detuvo",
    ])),
    "Try Again": dict(zip(LANGS, [
        "Försök igen", "Prøv igen", "Prøv igjen", "Yritä uudelleen",
        "Erneut versuchen", "Réessayer", "Reintentar",
    ])),
    # NSAudioCaptureUsageDescription in desktop/macos/Info.plist; the build turns it into
    # <lang>.lproj/InfoPlist.strings (scripts/infoplist-strings.py).
    "AudioCurve processes the sound your Mac plays so it matches your hearing.": dict(zip(LANGS, [
        "AudioCurve behandlar ljudet som din Mac spelar så att det passar din hörsel.",
        "AudioCurve behandler lyden, din Mac afspiller, så den passer til din hørelse.",
        "AudioCurve behandler lyden Macen spiller av, slik at den passer til hørselen din.",
        "AudioCurve käsittelee Macin toistamaa ääntä, jotta se sopii kuuloosi.",
        "AudioCurve bearbeitet den Ton, den Ihr Mac wiedergibt, damit er zu Ihrem Gehör passt.",
        "AudioCurve traite le son que lit votre Mac pour l’adapter à votre audition.",
        "AudioCurve procesa el sonido que reproduce tu Mac para que se adapte a tu audición.",
    ])),
    # The title of the Settings window (macOS names it itself).
    "Settings": dict(zip(LANGS, [
        "Inställningar", "Indstillinger", "Innstillinger", "Asetukset",
        "Einstellungen", "Réglages", "Ajustes",
    ])),
}


def main(in1, in2, out):
    a = json.load(open(in1, encoding="utf-8"))
    b = json.load(open(in2, encoding="utf-8"))
    strings = dict(a["strings"])
    strings.update(b["strings"])
    for key, tr in EXTRA.items():
        strings[key] = {"localizations": {
            lang: {"stringUnit": {"state": "translated", "value": v}} for lang, v in tr.items()}}
    merged = {"sourceLanguage": "en", "version": a.get("version", "1.0"),
              "strings": dict(sorted(strings.items()))}
    with open(out, "w", encoding="utf-8") as f:
        json.dump(merged, f, ensure_ascii=False, indent=2, sort_keys=False)
        f.write("\n")


if __name__ == "__main__":
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    main(*sys.argv[1:])
