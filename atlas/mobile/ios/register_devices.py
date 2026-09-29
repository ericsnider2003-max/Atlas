#!/usr/bin/env python3
"""Register the phones in devices.txt with Apple, for the iPhone build.

Atlas collects each phone's UDID (the hub's Your phone page, `phoneadd`), and
Eric's list comes here as devices.txt. This:

  1. registers any that Apple doesn't have yet (App Store Connect API:
     GET /v1/devices, POST /v1/devices), and switches back on any that are
     registered but disabled (PATCH /v1/devices/{id});
  2. deletes every stored ad hoc profile for Atlas's three App IDs that
     doesn't list every phone in devices.txt, so the build's automatic
     signing makes fresh ones that include them.

Step 2 reads the phones each profile actually lists rather than trusting
"something was added this run": a run that registered a phone and then
stopped before deleting the old profiles would otherwise leave that phone out
of every later build, since it would then count as "already registered".

devices.txt lines are `<UDID> <name>`, separated by a tab or by spaces (a
paste through a chat often turns the tab into spaces). A line whose first
word isn't a UDID stops the run and says which line, before anything is sent
to Apple.

Run by .github/workflows/ios.yml with the same key as asc_profiles.py. The
key's role must be Admin (or App Manager with access to devices) for Apple to
accept POST /v1/devices.
"""
import base64
import json
import os
import plistlib
import re
import sys
import urllib.error
import urllib.parse
import urllib.request

sys.path.insert(0, os.path.dirname(__file__))
import asc_profiles  # noqa: E402  (token, call, all_profiles, APPS)

API = asc_profiles.API
# As phoneadd::looks_like_udid: 40 hex (older devices) or 8-16 hex with a dash.
UDID = re.compile(r"^(?:[0-9A-Fa-f]{40}|[0-9A-Fa-f]{8}-[0-9A-Fa-f]{16})$")


def clean_name(name):
    """A name Apple's device list takes: printable ASCII, at most 50."""
    name = name.replace("’", "'").replace("‘", "'").replace("“", '"').replace("”", '"')
    name = "".join(c for c in name if 32 <= ord(c) < 127).strip()
    return (name or "An iPhone")[:50]


def read_devices(path):
    out, bad = [], []
    for n, line in enumerate(open(path, encoding="utf-8-sig"), 1):
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        parts = line.split(None, 1)
        udid = parts[0].strip()
        name = parts[1] if len(parts) > 1 else ""
        if not UDID.match(udid):
            bad.append(f"line {n}: {udid[:45]!r} isn't a UDID")
            continue
        out.append((udid, clean_name(name)))
    if bad:
        for b in bad:
            print(f"::error::devices.txt {b}")
        raise SystemExit("devices.txt has lines that aren't `<UDID> <name>`; nothing was sent to Apple.")
    return out


def send(method, path, body, tok):
    req = urllib.request.Request(
        API + path,
        data=json.dumps(body).encode(),
        method=method,
        headers={"Authorization": f"Bearer {tok}", "Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=60) as r:
            return json.loads(r.read() or b"{}")
    except urllib.error.HTTPError as e:
        raise SystemExit(f"Apple said {e.code} to {method} {path}: {e.read().decode(errors='replace')[:400]}")


def devices_in(profile_content_b64):
    """The UDIDs a profile lists, read from its signed plist (lower case)."""
    raw = base64.b64decode(profile_content_b64 or "")
    start, end = raw.find(b"<?xml"), raw.find(b"</plist>")
    if start < 0 or end < 0:
        return None
    plist = plistlib.loads(raw[start : end + len(b"</plist>")])
    return {u.lower() for u in plist.get("ProvisionedDevices", [])}


def main():
    path = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(__file__), "devices.txt")
    wanted = read_devices(path)
    if not wanted:
        print("devices.txt lists no phones; nothing to register.")
        return
    tok = asc_profiles.token()
    have = {}
    url = "/devices?" + urllib.parse.urlencode({"limit": 200, "fields[devices]": "udid,status,name"})
    while url:
        res = asc_profiles.call("GET", url, tok)
        for d in res.get("data", []):
            have[d["attributes"]["udid"].lower()] = d
        url = res.get("links", {}).get("next")
    for udid, name in wanted:
        d = have.get(udid.lower())
        if d is None:
            send("POST", "/devices", {"data": {"type": "devices", "attributes": {"name": name, "platform": "IOS", "udid": udid}}}, tok)
            print(f"{name}: registered.")
        elif d["attributes"].get("status") == "DISABLED":
            send("PATCH", f"/devices/{d['id']}", {"data": {"type": "devices", "id": d["id"], "attributes": {"status": "ENABLED"}}}, tok)
            print(f"{name}: was registered but disabled; switched back on.")
        else:
            print(f"{name}: already registered.")
    need = {u.lower() for u, _ in wanted}
    for ident, d in asc_profiles.all_profiles(tok):
        if ident not in asc_profiles.APPS or d["attributes"].get("profileType") != "IOS_APP_ADHOC":
            continue
        listed = devices_in(d["attributes"].get("profileContent"))
        missing = need - (listed or set())
        if listed is not None and not missing:
            print(f"{ident}: ad hoc profile '{d['attributes'].get('name')}' already lists every phone.")
            continue
        asc_profiles.call("DELETE", f"/profiles/{d['id']}", tok)
        print(f"{ident}: ad hoc profile '{d['attributes'].get('name')}' left out {len(missing)} phone(s); "
              "deleted, so the build makes one with every registered phone.")


if __name__ == "__main__":
    main()
