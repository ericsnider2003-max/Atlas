# Atlas on Windows

**State, 27 Sep 2026.**
- Builds on GitHub with Microsoft's toolchain; the first run (27 Sep) built, and `atlas --version` ran on real Windows.
- **Unsigned** until Azure Artifact Signing is set up.
- The friend-ready Windows build (x86_64-pc-windows-gnu, cross-compiled) is on your Desktop in `Merged 2026-09-27 friend-ready\Atlas-for-Windows-friend-ready.zip`.

## Build

- **GitHub:** Actions → **Windows app** → Run workflow. It takes about 35 minutes the first time (the release build uses one codegen unit), and less once the Rust cache is warm. Windows minutes count double against the free allowance. The result is under the run's Artifacts: `Atlas-Windows-<n>` or `…-unsigned`.
- **Linux cross-build**, as the other chats did: `cargo build --release --target x86_64-pc-windows-gnu` (MinGW). It's used for the friend-ready zip.
- **The laptop can't compile** (OPEN_GAPS 1.1). It needs Visual Studio Build Tools plus `rustup default stable-x86_64-pc-windows-msvc`, or MinGW on PATH. Nothing was installed for you.

## Sign (Azure Artifact Signing): what you do, once

1. **Azure:** make a paid subscription at portal.azure.com (free and trial subscriptions are refused).
2. **The account:** create an **Artifact Signing account**. Note its region endpoint, for example `https://eus.codesigning.azure.net/`, and its name.
3. **Identity validation:** Individual, US. You need a government ID with your address, a face check (AU10TIX) and Microsoft Authenticator. This is yours to do; nobody can do it for you.
4. **The profile:** once validated, create a **Public Trust certificate profile**. Note its name.
5. **The app that signs:** Entra ID → App registrations → New. Note the tenant ID and client ID, then make a client secret.
6. **Permission:** on the signing account → Access control (IAM), give that app the **Artifact Signing Certificate Profile Signer** role.
7. **GitHub secrets:** in the repository, add the six secrets named at the top of `.github/workflows/windows.yml`: `AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `ARTIFACT_SIGNING_ENDPOINT`, `ARTIFACT_SIGNING_ACCOUNT`, `ARTIFACT_SIGNING_PROFILE`.
8. **Run the workflow.** The log's "What Windows will say about it" step must say `Signature: Valid`, and the build fails if it doesn't.

It's reported at about $10 a month for the basic tier; Microsoft's pricing page didn't show the figure.

**Check before paying** (from the main chat's signing plan, `improvements-project/step2-and-signing-plan-26sep.md`):
- One Microsoft answer says individual onboarding was **paused**.
- A developer's write-up from April 2026 says individuals can apply again.
- Billing starts the day the account is created, so look in the portal first.
- The signature covers `atlas.exe`, not the `tor.exe` Atlas starts (gap AO).

## Install and update

- **First install:** `atlas/docs/INSTALLING.md` and `atlas setup` (the double-click setup, doc 19).
- **Friends' machines:** friend-ready (doc `37_MERGE…` §4). Run `atlas get`, then `atlas get pictures` (the ~3 GB model); `atlas doctor` says what's missing.
- **Updates:** they come through the release courier (`release`, `courier`, `update_apply`), signed with your release key (`atlas release keygen`, not made yet). The first trial run rolls back by itself if it fails (O1).

## Not proven yet

- A signed build.
- A real restart into a new build (8.2).
- Toasts and the sign-in vault on the desktop (1.4).
- Tor beside Atlas on Windows: built 27 Sep (8.3 closed). `windows.yml` puts the Tor Project's expert bundle 15.0.23 in `dist/tor/` (SHA-256 checked) and runs `tor.exe --version`; the setup window, and `atlas get tor`, fetch the same file for installs made before. Its first run on the laptop is still to come.
- The firewall rule (8.9): the setup asks once ("Letting your own devices reach Atlas"), with the Windows "allow changes" prompt; not yet run on Windows.
- The Windows-only stubs in some tests (OPEN_GAPS §9).
