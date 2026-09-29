# Signing Atlas for Windows: what to click, starting today

Goal: `atlas.exe` built by GitHub Actions comes out signed with your name on it, so Windows stops showing "Windows protected your PC". The workflow (`.github/workflows/windows.yml`) is already written. It signs automatically once six GitHub secrets exist. Until then it builds an unsigned exe and says so.

**What finishes today:** Part A (the account upgrade), Parts B–C (the Azure signing account and roles), starting Part D (identity check), Part F (the GitHub app and secrets).
**What waits on Microsoft:** Part D finishing. Microsoft says identity validation "takes from 1 to 20 business days (possibly longer if we need to request more documentation)". Part E (the certificate profile) can't be created until D says **Completed**.
**Meanwhile:** unsigned test builds work. On first run, Windows shows "Windows protected your PC" → click **More info** → **Run anyway**.

## Before you start: the doubt about your free account

- Microsoft refuses free, trial and sponsored subscriptions: "Artifact Signing doesn't support free, trial, or sponsored Azure subscriptions." You must upgrade to **pay-as-you-go**. Your current free account is the right *login*; it's the wrong *subscription type* until you upgrade it.
- As an individual (not a company), your Azure **billing account** must have Account Type **Individual**, and its **legal name and sold-to address must match your government ID exactly**. The identity form copies them from billing and won't let you edit them there. I couldn't confirm what account type a US free-account signup gets. Check it in step A5.
- Individuals are allowed only in the **United States or Canada**. You're in the US, so this is fine.

## What it costs

- **Basic tier: $9.99/month**, which covers 5,000 signatures (extra signatures are $0.005 each). Basic is enough for Atlas.
- Billing starts when the signing account is created (Part B), even if identity validation later fails. Microsoft says it is "not calculated on a pro rata basis", so you pay the full month however late in the month you start. If validation fails for good, delete the account to stop the charge.
- Upgrading to pay-as-you-go costs nothing by itself. You pay only for what you use, and you keep any remaining free credit until 30 days after your original signup.

## A. Upgrade the free account (today, ~10 min)

1. Go to https://portal.azure.com and sign in.
2. In the top search box, type **Subscriptions** and open it. Click your subscription (probably named "Azure subscription 1").
3. Click **Upgrade subscription** in the top bar, or the upgrade banner if the button isn't there.
4. Add a payment method and verify your phone if asked. Enter a subscription name, choose the free **Basic** support plan, and click **Upgrade**.
5. Check your billing identity: search **Cost Management + Billing** → **Billing scopes** / your billing account → **Properties**. Confirm that Account type is **Individual**, the name matches your ID exactly (including middle name if your ID shows one), and the sold-to address matches your ID. Fix anything that differs *before* Part D.

## B. Create the signing account (today, ~5 min)

1. Search **Subscriptions** → your subscription → left menu **Settings** → **Resource providers**. Search `Microsoft.CodeSigning`, select it, and click **Register**. Wait until it says **Registered**.
2. Search **Artifact Signing Accounts** → **Create**.
3. **Subscription**: the one you just upgraded. **Resource group**: **Create new** → `atlas-signing`.
4. **Account name**: 3–24 letters and numbers, starting with a letter, globally unique (e.g. `atlassign` plus your initials). Write it down. It becomes secret `ARTIFACT_SIGNING_ACCOUNT`.
5. **Region**: **East US**. Its endpoint is `https://eus.codesigning.azure.net/`, which becomes secret `ARTIFACT_SIGNING_ENDPOINT`. Other US choices are Central US (`cus`), North Central US (`ncus`), South Central US (`scus`), West Central US (`wcus`), West US (`wus`), West US 2 (`wus2`) and West US 3 (`wus3`), all at `https://<code>.codesigning.azure.net/`. The endpoint must match the region you pick.
6. **Pricing**: **Basic**. Click **Review + Create** → **Create** → **Go to resource**.

## C. Give yourself the identity role (today, ~3 min)

1. In the signing account, open **Access control (IAM)** → **Add** → **Add role assignment**.
2. Search `Artifact Signing Identity Verifier`, select it, and click **Next**.
3. **Members** → **Select members** → pick yourself → **Select** → **Review + assign**.
   (If **New identity** is greyed out in Part D, this role is missing.)

## D. Identity validation (start today; Microsoft finishes it in 1–20 business days)

Have ready a phone with **Microsoft Authenticator** installed and a **government photo ID** (driver's license, state ID or passport). If asked for proof of address, a utility bill or bank statement from the last three months works. The ID must not expire within 2 months. Take photos flat, with no flash, uncropped, and nothing covering the ID.

1. In the signing account, go to **Objects** → **Identity validations**.
2. Change the **Organization** dropdown to **Individual** → **New identity** → **Public**.
3. Select your billing account. The form fills itself in. Check **Certificate subject preview** (the certificate shows your name, city and state, not your street or email). Click **Create**. Status: **In Progress**.
4. When status becomes **Action Required** (also emailed to you), click your name, then the link under "Please complete your verification here". Sign in with the same email.
5. Click **Get verified here through our trusted ID-verifiers** → AU10TIX → **Let's Begin**. Enter your email, then the emailed PIN, then your phone number → **Start**.
6. Scan the QR code with your phone and follow the prompts to photograph your ID. Keep the browser open.
7. On the phone, tap **Open Authenticator**. Scan the next QR code in the browser → **Add**. Then scan the "Present your Verified ID" code → select the **Verifiable Credential** → **Share**. The browser shows **Verification Successful**.
8. Wait. The status must reach **Completed** before Part E. Microsoft emails updates. Any emailed link expires after 7 days.

## E. Certificate profile (after D says Completed, ~2 min)

1. Signing account → **Objects** → **Certificate profiles** → **Create** → **Public Trust**.
2. **Certificate Profile Name**: 5–100 letters and numbers, e.g. `atlaspublic`. This becomes secret `ARTIFACT_SIGNING_PROFILE`. Leave **Program Type** as **None**.
3. **Verified CN and O**: pick your completed validation. Leave **Include street address** and **Include postal code** unchecked unless you want them public. Click **Create**.

## F. The robot login for GitHub (today, ~10 min)

1. Search **Microsoft Entra ID** → **App registrations** → **New registration**. Name: `atlas-github-signer`. Supported account types: **Accounts in this organizational directory only**. Leave Redirect URI blank → **Register**.
2. On its **Overview**, copy **Application (client) ID** (→ `AZURE_CLIENT_ID`) and **Directory (tenant) ID** (→ `AZURE_TENANT_ID`).
3. **Certificates & secrets** → **Client secrets** → **New client secret**. Description `github`. **Expires**: the longest offered (24 months) → **Add**. Copy the **Value** column right now. It is shown only once. It becomes `AZURE_CLIENT_SECRET`. Set a calendar reminder to renew it before it expires.
4. Go back to the signing account → **Access control (IAM)** → **Add** → **Add role assignment** → `Artifact Signing Certificate Profile Signer` → **Next** → **User, group, or service principal** → **Select members**. Type `atlas-github-signer` (apps don't appear until you type the name) → **Select** → **Review + assign**.

## G. Add the six secrets to GitHub

In your repo on github.com: **Settings** → **Secrets and variables** → **Actions** → **New repository secret**. For each one, paste the exact name, paste the value, and click **Add secret**.

| Secret name | What goes in it | From |
|---|---|---|
| `AZURE_TENANT_ID` | Directory (tenant) ID | F2 |
| `AZURE_CLIENT_ID` | Application (client) ID | F2 |
| `AZURE_CLIENT_SECRET` | the secret **Value** (not the Secret ID) | F3 |
| `ARTIFACT_SIGNING_ENDPOINT` | e.g. `https://eus.codesigning.azure.net/` | B5 |
| `ARTIFACT_SIGNING_ACCOUNT` | signing account name | B4 |
| `ARTIFACT_SIGNING_PROFILE` | certificate profile name | E2 (after validation) |

The workflow signs only when **all six** exist. Add the first five today and the profile name once Part E is done. Then go to **Actions** → **Windows app** → **Run workflow**. The "What Windows will say about it" step should print `Signature: Valid` and your name. Download the build from the run's **Artifacts** section. Its name no longer ends in `-unsigned`.

**Not checked:** whether a newly signed exe still gets a SmartScreen warning for a while. I didn't research this, so don't treat signing as a guaranteed end to every warning until you see it on a real PC.

Sources: [FAQ](https://learn.microsoft.com/en-us/azure/artifact-signing/faq) · [Quickstart](https://learn.microsoft.com/en-us/azure/artifact-signing/quickstart) ([source text](https://github.com/MicrosoftDocs/azure-docs/blob/main/articles/artifact-signing/quickstart.md)) · [Assign roles](https://learn.microsoft.com/en-us/azure/artifact-signing/tutorial-assign-roles) · [Upgrade free account](https://learn.microsoft.com/en-us/azure/cost-management-billing/manage/upgrade-azure-subscription) · [App registration + secret](https://learn.microsoft.com/en-us/entra/identity-platform/howto-create-service-principal-portal) · [Price](https://azure.microsoft.com/en-us/products/artifact-signing) · [Signing action](https://github.com/Azure/artifact-signing-action)

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE
