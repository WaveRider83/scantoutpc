#!/usr/bin/env python3
# ScantoutPC — sidecar de traduction hors-ligne (Argos Translate).
# Usage :
#   translate_sidecar.py translate <from> <to>     lit le texte sur stdin, écrit la traduction sur stdout
#   translate_sidecar.py install <lang> [lang...]  télécharge les modèles <lang><->en depuis le dépôt Argos
#   translate_sidecar.py list                      liste les paires installées
# Codes de sortie : 0 ok, 1 erreur (message sur stderr).
import sys


def fail(msg: str) -> "NoReturn":  # noqa: F821
    print(msg, file=sys.stderr)
    sys.exit(1)


def load_argos():
    try:
        import argostranslate.package as pkg
        import argostranslate.translate as tr
        return pkg, tr
    except ImportError:
        fail("argostranslate non installé (menu Options → Installer/MAJ le moteur de traduction)")


def vtuple(v):
    out = []
    for part in str(v).replace("-", ".").split("."):
        try:
            out.append(int(part))
        except ValueError:
            out.append(0)
    return tuple(out)


def cmd_translate(frm: str, to: str) -> None:
    _, tr = load_argos()
    text = sys.stdin.read()
    if not text.strip():
        print("")
        return
    langs = {l.code: l for l in tr.get_installed_languages()}

    def pair(a, b):
        la, lb = langs.get(a), langs.get(b)
        if la is None or lb is None:
            return None
        return la.get_translation(lb)

    direct = pair(frm, to)
    if direct is not None:
        print(direct.translate(text))
        return
    # pivot par l'anglais si la paire directe n'est pas installée
    if frm != "en" and to != "en":
        leg1, leg2 = pair(frm, "en"), pair("en", to)
        if leg1 is not None and leg2 is not None:
            print(leg2.translate(leg1.translate(text)))
            return
    fail(f"modèle {frm}->{to} non installé (menu Options → Télécharger les modèles de traduction)")


def cmd_install(langs) -> None:
    pkg, _ = load_argos()
    try:
        pkg.update_package_index()
    except Exception as e:
        fail(f"index des modèles inaccessible: {e}")
    available = pkg.get_available_packages()
    wanted = set()
    for l in langs:
        l = l.strip().lower()
        if not l or l == "en":
            continue
        wanted.add((l, "en"))
        wanted.add(("en", l))
    installed, missing = [], []
    for a, b in sorted(wanted):
        cands = [p for p in available if p.from_code == a and p.to_code == b]
        if not cands:
            missing.append(f"{a}->{b}")
            continue
        best = max(cands, key=lambda p: vtuple(getattr(p, "package_version", "0")))
        try:
            path = best.download()
            pkg.install_from_path(path)
            installed.append(f"{a}->{b}")
        except Exception as e:
            missing.append(f"{a}->{b} ({e})")
    if installed:
        print("installés: " + ", ".join(installed))
    if missing:
        print("échecs: " + ", ".join(missing), file=sys.stderr)
        sys.exit(1 if not installed else 0)


def cmd_list() -> None:
    _, tr = load_argos()
    pairs = []
    for l in tr.get_installed_languages():
        for t in l.translations_from:
            pairs.append(f"{l.code}->{t.to_lang.code}")
    print(", ".join(sorted(pairs)) if pairs else "(aucun modèle installé)")


def main() -> None:
    args = sys.argv[1:]
    if not args:
        fail("usage: translate_sidecar.py translate <from> <to> | install <lang...> | list")
    cmd = args[0]
    if cmd == "translate" and len(args) == 3:
        cmd_translate(args[1].lower(), args[2].lower())
    elif cmd == "install" and len(args) >= 2:
        cmd_install(args[1:])
    elif cmd == "list":
        cmd_list()
    else:
        fail("usage: translate_sidecar.py translate <from> <to> | install <lang...> | list")


if __name__ == "__main__":
    main()
