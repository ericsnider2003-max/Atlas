#!/usr/bin/env python3
"""Check Apple's side of signing before the Mac spends minutes building.

Run by .github/workflows/ios.yml, before the Rust core is built. It uses the
same App Store Connect API key the build uses, and it:

  1. finds the three App IDs (the app, the share extension, the widgets);
  2. says whether App Groups is switched on for the app and the share
     extension, which need it;
  3. reads every provisioning profile Apple holds for those App IDs, and
     says whether each one grants the App Group (GROUP, below);
  4. with --fix, deletes the profiles that DON'T grant it;
  5. with --after-failure (run when the build fails), reads the profile Xcode
     just made and says exactly what it grants, which tells an App ID with
     no group attached apart from one with a differently named group.

Why step 4: after the group is attached to an App ID on developer.apple.com,
a profile made before that still lacks it. Xcode's automatic signing with an
API key re-downloads that stale profile instead of making a new one, and the
build fails with "Provisioning profile ... doesn't match the entitlements
file's value for the com.apple.security.application-groups entitlement".
These are profiles Xcode made by itself, and it makes a fresh one on the next
build, so deleting a stale one loses nothing. A profile that already grants
the group is never touched.

Needs: ASC_KEY_ID, ASC_ISSUER_ID, ASC_KEY_PATH in the environment, and the
PyJWT and cryptography packages.
"""
import base64
import json
import os
import plistlib
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

API = "https://api.appstoreconnect.apple.com/v1"
# The App Group as Apple registered it. The doubled "group." is real: the
# registration form on developer.apple.com puts "group." in front of whatever
# is typed, and "group.com.ericsnider.atlas" was typed (27 Sep 2026). The
# name is never shown to anyone, so the app uses it as registered rather than
# asking for a second group.
GROUP = "group.group.com.ericsnider.atlas"
APPS = {
    "com.ericsnider.atlas": True,         # needs the group
    "com.ericsnider.atlas.share": True,   # needs the group
    "com.ericsnider.atlas.live": True,    # the widgets read the glance from it (27 Sep 2026)
}


def token():
    import jwt  # PyJWT

    key = open(os.environ["ASC_KEY_PATH"]).read()
    now = int(time.time())
    return jwt.encode(
        {"iss": os.environ["ASC_ISSUER_ID"], "iat": now, "exp": now + 900, "aud": "appstoreconnect-v1"},
        key,
        algorithm="ES256",
        headers={"kid": os.environ["ASC_KEY_ID"], "typ": "JWT"},
    )


def call(method, path, tok):
    url = path if path.startswith("http") else API + path
    req = urllib.request.Request(url, method=method, headers={"Authorization": f"Bearer {tok}"})
    try:
        with urllib.request.urlopen(req, timeout=60) as r:
            body = r.read()
            return json.loads(body) if body else {}
    except urllib.error.HTTPError as e:
        detail = e.read().decode(errors="replace")[:400]
        raise SystemExit(f"Apple said {e.code} to {method} {url}: {detail}")


def groups_in(profile_content_b64):
    """The application-groups a profile grants, read from its signed plist."""
    raw = base64.b64decode(profile_content_b64)
    start, end = raw.find(b"<?xml"), raw.find(b"</plist>")
    if start < 0 or end < 0:
        return None
    plist = plistlib.loads(raw[start : end + len(b"</plist>")])
    return list(plist.get("Entitlements", {}).get("com.apple.security.application-groups", []))


def all_profiles(tok):
    """Every profile on the account, with the App ID each one is for."""
    out, url = [], "/profiles?" + urllib.parse.urlencode({"include": "bundleId", "limit": 200})
    while url:
        res = call("GET", url, tok)
        ids = {i["id"]: i["attributes"]["identifier"] for i in res.get("included", []) if i["type"] == "bundleIds"}
        for d in res.get("data", []):
            ref = (d.get("relationships", {}).get("bundleId", {}) or {}).get("data") or {}
            out.append((ids.get(ref.get("id"), "?"), d))
        url = res.get("links", {}).get("next")
    return out


def main():
    fix = "--fix" in sys.argv
    after = "--after-failure" in sys.argv
    tok = token()
    problems = []
    for ident, needs_group in APPS.items():
        q = urllib.parse.urlencode({"filter[identifier]": ident, "include": "bundleIdCapabilities", "limit": 5})
        res = call("GET", f"/bundleIds?{q}", tok)
        ids = [d for d in res.get("data", []) if d["attributes"]["identifier"] == ident]
        if not ids:
            print(f"{ident}: not registered yet. The build registers it; nothing to check.")
            continue
        bid = ids[0]
        included = {(i["type"], i["id"]): i for i in res.get("included", [])}
        caps = [
            included[("bundleIdCapabilities", c["id"])]["attributes"]["capabilityType"]
            for c in bid["relationships"]["bundleIdCapabilities"].get("data", [])
            if ("bundleIdCapabilities", c["id"]) in included
        ]
        has_cap = "APP_GROUPS" in caps
        print(f"{ident}: App Groups {'on' if has_cap else 'OFF'}")
        if needs_group and not has_cap:
            problems.append(
                f"{ident} doesn't have App Groups switched on. On developer.apple.com: "
                f"Identifiers -> {ident} -> tick App Groups -> Configure -> tick {GROUP} -> Save."
            )

    profiles = all_profiles(tok)
    print(f"\n{len(profiles)} profile(s) on the account.")
    for ident, needs_group in APPS.items():
        mine = [d for (i, d) in profiles if i == ident]
        if not mine:
            print(f"{ident}: no stored profile; Xcode makes one during the build.")
        for d in mine:
            a = d["attributes"]
            groups = groups_in(a.get("profileContent", "")) or []
            print(f"{ident}: profile '{a['name']}' ({a['profileType']}, {a['profileState']}) grants "
                  f"{', '.join(groups) if groups else 'no App Group at all'}")
            if not needs_group or GROUP in groups:
                continue
            if after:
                # Made during THIS build, from the App ID as it stands now: so the
                # App ID itself doesn't carry the group.
                other = (f" It carries {', '.join(groups)} instead: a group with a different name. "
                         "(The registration form puts 'group.' in front of what's typed, so typing the "
                         "full name gives a doubled 'group.group.'.)") if groups else ""
                problems.append(
                    f"Apple's fresh profile for {ident} doesn't grant {GROUP}, so the group isn't attached "
                    f"to that App ID.{other} On developer.apple.com: Identifiers -> {ident} -> App Groups -> "
                    f"Configure (or Edit) -> tick {GROUP} -> Continue -> Save, and confirm."
                )
            elif fix:
                call("DELETE", f"/profiles/{d['id']}", tok)
                print("    deleted: it predates the group; Xcode makes a fresh one during the build.")
            else:
                problems.append(f"profile '{a['name']}' for {ident} lacks {GROUP} (run with --fix)")
    if problems:
        print("\nApple's side isn't ready:")
        for p in problems:
            print(f"  - {p}")
            print(f"::error::{p}")
        return 1
    print("\nApple's side is ready for signing." if not after else "\nThe profiles grant the group; the failure is elsewhere.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
