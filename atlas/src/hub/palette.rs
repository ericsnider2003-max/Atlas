//! The command palette and Find.
//!
//! Moved out of `hub.rs` unchanged (audit Q6, 6 Oct 2026): one file of
//! thousands of lines was where every chat's edits collided.

use super::*;

/// One result, as a row you can act on.
///
/// A destination is a link and an action is a form, so picking either does the
/// real thing rather than taking you to the page that holds the button.
pub(super) fn palette_row(e: &crate::palette::Entry, first: bool) -> String {
    let inner = format!(
        "<span class=name>{}</span><span class=what>{}</span>",
        esc(e.label),
        esc(e.hint)
    );
    let mark = if first { " first" } else { "" };
    match &e.does {
        crate::palette::Does::Go(href) => format!(
            "<a class='hit{mark}' href='{}'>{inner}</a>",
            esc(href)
        ),
        crate::palette::Does::Run(action, fields) => {
            let hidden: String = fields
                .iter()
                .map(|(k, v)| {
                    format!("<input type=hidden name='{}' value='{}'>", esc(k), esc(v))
                })
                .collect();
            format!(
                "<form class=hitform method=post action='{}'>{hidden}\
                 <button class='hit{mark}'>{inner}</button></form>",
                esc(action)
            )
        }
    }
}

/// The palette, on every page.
///
/// Rendered into the page rather than fetched, because Atlas works offline and
/// a palette that needs a round trip per keystroke is a palette that stutters
/// on the day the machine is busy — which is exactly the day you are looking
/// for something.
///
/// The whole thing is inside a plain `<form>` pointing at a real page, so with
/// no script at all typing and pressing enter still lands somewhere useful.
pub fn palette_overlay(entries: &[crate::palette::Entry], recent: &crate::palette::Recent) -> String {
    // On the phone app, nothing that goes to a page it hasn't got (`phonemode`).
    let kept: Vec<crate::palette::Entry> = entries
        .iter()
        .filter(|e| {
            let (crate::palette::Does::Go(to) | crate::palette::Does::Run(to, _)) = e.does;
            route_any(to.split('?').next().unwrap_or(to)).is_none_or(|p| p.here())
        })
        .cloned()
        .collect();
    let entries = &kept[..];
    let first = recent.first(entries);
    let shown: Vec<&crate::palette::Entry> = if first.is_empty() {
        entries.iter().take(crate::palette::SHOW).collect()
    } else {
        first
            .into_iter()
            .chain(entries.iter().filter(|e| !recent.picked.iter().any(|p| p == e.id())))
            .take(crate::palette::SHOW)
            .collect()
    };

    let mut rows = String::new();
    for (i, e) in shown.iter().enumerate() {
        rows.push_str(&palette_row(e, i == 0));
    }
    // Every entry, hidden, so typing filters instantly instead of only
    // filtering the seven that happened to be offered first.
    let mut all = String::new();
    for e in entries {
        all.push_str(&format!(
            "<div class=allhit data-words='{}'>{}</div>",
            esc(&format!(
                "{} {} {}",
                e.label.to_lowercase(),
                e.hint.to_lowercase(),
                e.also.join(" ")
            )),
            palette_row(e, false)
        ));
    }

    format!(
        "<div class=palette id=palette hidden>\
         <form class=palbox method=get action='/hub/find'>\
         <input class=palq id=palq name=q aria-label='Find anything' autocomplete=off spellcheck=false \
         placeholder='What do you want to do?'>\
         <div class=hits id=palhits>{rows}</div>\
         <div class=allhits id=palall hidden>{all}</div>\
         <p class=palfoot>Type to filter. Enter opens the first one. Escape closes.</p>\
         </form></div>{PALETTE_SCRIPT}"
    )
}

/// The plain version, for when there is no script — and the page the palette's
/// form posts to if you press enter before it has filtered anything.
pub fn find_page(
    query: &str,
    results: &[&crate::palette::Entry],
    did_you_mean: Option<&crate::palette::Entry>,
) -> String {
    let mut body = format!(
        "<form method=get action='/hub/find'>\
         <input name=q value='{}' aria-label='What do you want to do?' placeholder='What do you want to do?' autocomplete=off>\
         <button class=primary>Find</button></form>",
        esc(query)
    );
    if results.is_empty() {
        match did_you_mean {
            // Asked, not opened: a typo guess is a question.
            Some(e) => {
                body.push_str(&format!(
                    "<p class=note>Nothing matches \"{}\" exactly. Did you mean:</p><div class='hits plain'>",
                    esc(query.trim())
                ));
                body.push_str(&palette_row(e, true));
                body.push_str("</div>");
            }
            None => body.push_str(&nothing(&format!(
                "Nothing here matches \"{}\". Try what you'd call it out loud.",
                query.trim()
            ))),
        }
    } else {
        body.push_str("<div class='hits plain'>");
        for (i, e) in results.iter().take(crate::palette::SHOW).enumerate() {
            body.push_str(&palette_row(e, i == 0));
        }
        body.push_str("</div>");
    }
    shell("Find", &body)
}
