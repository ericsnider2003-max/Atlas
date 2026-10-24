# The Hub is not slop: the guard (24 Oct 2026)

The house style is the locked design in `SPEC.md`: Warm Paper, Ember Dark and Access. This note covers only the guard that stops the Hub drifting from it into a generic template look. The guard is Plan E from the video analysis.

`taste::review` (`atlas/src/taste.rs`) is the offline design lint that Atlas already ran on pages it builds. It now knows four more tells. Each can be switched off in the house style (`Rules.slop`):

| Rule | Severity | What it catches |
|---|---|---|
| generic gradient | blocking | a `linear-gradient` running purple to blue |
| cards in cards | blocking | an element with class `card` inside another |
| contrast | blocking | text and background in one CSS rule that measure under 4.5:1 once the page's colour tokens resolve, checked in the light colours and again in the dark ones |
| stock typeface | advisory | Inter, Poppins, Montserrat, Space Grotesk or Manrope as the first `font-family` |

The rule ideas come from the Impeccable design rules (Apache-2.0). The code is Atlas's own; nothing was copied.

`atlas/tests/the_hub_is_not_slop.rs` renders Home, Now, Outstanding, Talk, Projects, Messages and Settings, each with its stylesheet in place. It fails the build on any blocking finding and on any of the four tells.

On 24 Oct 2026 the Hub showed 0 problems and 2 advisories. A probe written with the Hub's own colour tokens proves the contrast check really reads them, in both colourways.

Fixed in the lint on the way: the control-name rule raised 204 false alarms on the Hub's forms. It flagged hidden inputs, and inputs inside their own `<label>`. Neither needs another name.

What it can't see: anything that needs a rendered page, such as touch-target size, real layout or focus rings. Those are checked by the barrier-check rendered pass (see `verification/runs/20261023T000000Z-claude-hub-redesign/`).
