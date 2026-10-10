# Writing slots for REPORT.md
> Write under each heading. `rt report` copies each section into the report; lines starting with `>` are hints and are dropped.
> Never edit REPORT.md by hand: it is regenerated whole every time.

## broken
> What's broken: one or two plain sentences, about 20 words each, in the owner's terms.
Atlas must complete jobs truthfully and protect data.

## cost
> What it could cost: money, customers, data or downtime, with numbers where you have them. 40 words or fewer.
A failed job can lose data, mislead users, expose private information, or trigger unauthorized action.

## need
> The one decision you need from the owner. 30 words or fewer.
Prioritize reliability defects; block protected actions until explicitly trusted.

## scenarios
> 3–6 cards when a long-running process exists, otherwise up to 6. Format in references/output.md (### title, then What happens · How likely · Would you notice? · How you'd recover today · Fixed by).
### Long job cancellation
What happens · A long job is cancelled and restarted. · How likely · Frequent. · Would you notice? · Sometimes. · How recover · Restart. · Fixed by · Truthful cancellation receipts. (BUG-003, BUG-005)
### File recovery
What happens · A file operation stops. · How likely · Common. · Would you notice? · Not always. · How recover · Search manually. · Fixed by · Recovery receipts. (BUG-007)
### Restricted action
What happens · Atlas is asked to access a protected account. · How likely · Expected in testing. · Would you notice? · If status is truthful. · How recover · Revoke manually. · Fixed by · Trust gates. (BUG-001)

## areas
> Coverage table | Area | Result | Note |: one row per Gap or Risk area (note 10 words or fewer, cite finding ids), then ONE row listing every OK area.
| Area | Result | Note |
|---|---|---|
| Reliability and job lifecycle | Needs verification | Confirm high leads |
| Data and file safety | Needs verification | Confirm recovery paths |
| Protected external actions | Needs verification | Confirm trust gates |
| Cross-device operation | Gap | Other platforms untested |
| Optional security scanners | Gap | Unavailable in baseline |
| Baseline scan | Complete | 948 triaged |

## not_checked
> Anything else you did not examine, by name, or 'nothing else'.
Other-platform and live-account runs untested.

## challenge
> One line: which critical and high findings you tried to disprove, what was downgraded, which two weak areas you re-hunted.
Re-hunt critical/high leads; revisit recovery and trust.
