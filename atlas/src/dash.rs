//! The dashboard you arrange yourself.
//!
//! The hub was twelve pages, each one a function that printed a fixed shape.
//! Nothing about what you see was yours — not the order, not which parts show,
//! not how much room each one gets. That is fine for a settings screen and
//! wrong for the thing you open on purpose when you want to know where you
//! stand.
//!
//! ## Why the layout is data
//!
//! The lesson worth stealing from Notion is not the dragging. It is that a
//! view is not a copy of the data — it is one arrangement of the same
//! underlying source, and you can keep several. Once the arrangement is data,
//! rearranging is an edit to a value rather than a change to a template, it
//! survives a restart, it can be reset, and the same card can be shown
//! somewhere else without being written twice.
//!
//! ## Why moving is a click, and dragging is the extra
//!
//! Dragging alone is not enough: WCAG 2.2's dragging-movements criterion asks
//! for a single-pointer alternative to every drag, and a sortable dashboard is
//! explicitly not one of the cases where dragging counts as essential. So the
//! move buttons are the real mechanism — a plain form post that works with no
//! script, on a phone, on a bad connection — and dragging, when it is added,
//! posts exactly the same thing. One path, not two that can disagree.
//!
//! ## What it does not do
//!
//! No free-floating pixel positions. A card sits in an order and takes either
//! half the width or all of it. A twenty-four-column collage is a lot of
//! machinery to let someone leave a gap, and gaps are what makes a homemade
//! dashboard look homemade.

use serde::{Deserialize, Serialize};

/// A thing the dashboard can show.
///
/// Each names a source Atlas already has rather than a page that already
/// exists, so the same card can be shown in the dashboard, in one of Atlas's
/// own windows, or both, without the shape being written down twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Card {
    /// What's outstanding, worst first.
    Outstanding,
    /// What Atlas did while you weren't watching.
    Activity,
    /// Whether the things Atlas depends on are answering.
    Connections,
    /// Memory, disk, power.
    Machine,
    /// What Atlas would change about itself.
    Ideas,
    /// What it couldn't do, and why.
    Stuck,
    /// The day so far.
    Today,
    /// What Atlas is trusted to do alone, and what would change it.
    Trust,
    /// Things you handed over from another device.
    Handed,
    /// Your projects, and what each is waiting on. Added with the command
    /// deck (Eric's design, 23 Sep 2026).
    Projects,
}

impl Card {
    /// Every card that exists. The catalogue.
    ///
    /// The order is the command deck's, Eric's design of 23 Sep 2026: what's
    /// waiting on you, your projects, the machine's health, then what Atlas
    /// did without being asked — and after those, the rest.
    pub fn all() -> [Card; 10] {
        [
            Card::Outstanding,
            Card::Projects,
            Card::Machine,
            Card::Activity,
            Card::Today,
            Card::Connections,
            Card::Ideas,
            Card::Stuck,
            Card::Handed,
            Card::Trust,
        ]
    }

    /// A name, not a sentence. `note()` carries the explanation.
    pub fn title(self) -> &'static str {
        match self {
            Card::Outstanding => "Waiting on you",
            Card::Activity => "What I did without being asked",
            Card::Connections => "Reach",
            Card::Machine => "Health",
            Card::Ideas => "Self-review",
            Card::Stuck => "Stopped",
            Card::Today => "Today",
            Card::Trust => "Trust",
            Card::Handed => "Handed over",
            Card::Projects => "Projects",
        }
    }

    /// The half-sentence under the card's name, so a short name does not cost
    /// you the meaning of it.
    pub fn note(self) -> &'static str {
        match self {
            Card::Outstanding => "Still open, most pressing first",
            Card::Activity => "What Atlas has done without being watched",
            Card::Connections => "Whether what Atlas depends on is answering",
            Card::Machine => "Memory, disk and power",
            Card::Ideas => "What Atlas would change about itself",
            Card::Stuck => "What stopped, and what stopped it",
            Card::Today => "What has moved since this morning",
            Card::Trust => "What Atlas may do without asking, and why",
            Card::Handed => "Links and files you sent from somewhere else",
            Card::Projects => "Each project, and what it is waiting on",
        }
    }

    /// The value written into a form, and read back out of one.
    pub fn key(self) -> &'static str {
        match self {
            Card::Outstanding => "outstanding",
            Card::Activity => "activity",
            Card::Connections => "connections",
            Card::Machine => "machine",
            Card::Ideas => "ideas",
            Card::Stuck => "stuck",
            Card::Today => "today",
            Card::Trust => "trust",
            Card::Handed => "handed",
            Card::Projects => "projects",
        }
    }

    pub fn from_key(k: &str) -> Option<Card> {
        Card::all().into_iter().find(|c| c.key() == k)
    }
}

/// How much room a card takes.
///
/// Two options, not a pixel width. A dashboard where every card can be any
/// size is a dashboard that ends up with a 340px hole in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Span {
    #[default]
    Half,
    Full,
}

impl Span {
    pub fn other(self) -> Span {
        match self {
            Span::Half => Span::Full,
            Span::Full => Span::Half,
        }
    }
}

/// One card, placed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Placed {
    pub card: Card,
    #[serde(default)]
    pub span: Span,
    /// Hidden rather than removed, so putting it back is one click and does
    /// not need you to remember what used to be there.
    #[serde(default)]
    pub hidden: bool,
}

/// The whole arrangement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    pub cards: Vec<Placed>,
}

impl Default for Layout {
    /// The arrangement before you have made one.
    ///
    /// Outstanding first and full width, because the question you open a
    /// dashboard to answer is what needs doing.
    ///
    /// The design Eric locked on 20-21 Sep puts what's waiting on you inside
    /// the Brief and today in Today, both above the cards, so neither is a
    /// card by default ("Waiting on you rides inside the Brief, not as a
    /// separate home panel"). Under them: projects, health, and what Atlas
    /// did without being asked, which runs wide. The rest start hidden —
    /// still there, one press of Arrange away — because a page of ten equal
    /// cards is the "nothing stands out" dashboard the design replaced.
    fn default() -> Self {
        let shown = [Card::Projects, Card::Machine, Card::Activity];
        let cards: Vec<Placed> = Card::all()
            .into_iter()
            .map(|card| Placed {
                card,
                span: if matches!(card, Card::Outstanding | Card::Activity) { Span::Full } else { Span::Half },
                hidden: !shown.contains(&card),
            })
            .collect();
        Layout { cards }
    }
}

/// Where a saved layout lives.
pub const FILE: &str = "dashboard";

impl Layout {
    /// A saved layout, made to match the cards that exist today.
    ///
    /// Two things go wrong here and both are silent. A card added to Atlas
    /// after you saved a layout would never appear, because your saved list
    /// does not mention it — so a new capability would ship invisible to
    /// exactly the people who use the dashboard most. And a card removed from
    /// Atlas would still be named in your file, which either renders nothing
    /// or refuses to load the whole layout and quietly resets everything you
    /// arranged.
    ///
    /// So: unknown cards are dropped, missing cards are appended at the end
    /// rather than inserted somewhere opinionated, and the order you chose for
    /// everything else is left alone.
    pub fn reconciled(mut self) -> Layout {
        self.cards.retain(|p| Card::all().contains(&p.card));
        // Two entries for one card would make "move it" ambiguous.
        let mut seen: Vec<Card> = Vec::new();
        self.cards.retain(|p| {
            if seen.contains(&p.card) {
                false
            } else {
                seen.push(p.card);
                true
            }
        });
        for card in Card::all() {
            if !self.cards.iter().any(|p| p.card == card) {
                self.cards.push(Placed {
                    card,
                    span: Span::Half,
                    hidden: false,
                });
            }
        }
        self
    }

    pub fn load(store: &crate::store::Store) -> Layout {
        store.load::<Layout>(FILE).reconciled()
    }

    pub fn save(&self, store: &crate::store::Store) -> crate::error::Result<()> {
        store.save(FILE, self)
    }

    fn index_of(&self, card: Card) -> Option<usize> {
        self.cards.iter().position(|p| p.card == card)
    }

    /// Put a card at a position. What a drag posts, and what the move buttons
    /// post — the same call either way, so the two can't drift.
    ///
    /// An out-of-range target lands at the end rather than being refused: a
    /// drop past the last card means "put it last", and an error there would
    /// be a shrug at something the person clearly meant.
    pub fn place(&mut self, card: Card, to: usize) -> bool {
        let Some(from) = self.index_of(card) else {
            return false;
        };
        let to = to.min(self.cards.len().saturating_sub(1));
        if from == to {
            return false;
        }
        let moved = self.cards.remove(from);
        self.cards.insert(to, moved);
        true
    }

    /// The single-pointer alternative, in the direction you can see.
    pub fn move_up(&mut self, card: Card) -> bool {
        match self.index_of(card) {
            Some(0) | None => false,
            Some(i) => self.place(card, i - 1),
        }
    }

    pub fn move_down(&mut self, card: Card) -> bool {
        match self.index_of(card) {
            Some(i) if i + 1 < self.cards.len() => self.place(card, i + 1),
            _ => false,
        }
    }

    pub fn set_hidden(&mut self, card: Card, hidden: bool) -> bool {
        match self.index_of(card) {
            Some(i) => {
                let changed = self.cards[i].hidden != hidden;
                self.cards[i].hidden = hidden;
                changed
            }
            None => false,
        }
    }

    pub fn widen(&mut self, card: Card) -> bool {
        match self.index_of(card) {
            Some(i) => {
                self.cards[i].span = self.cards[i].span.other();
                true
            }
            None => false,
        }
    }

    /// The cards actually on screen, in order.
    pub fn visible(&self) -> Vec<&Placed> {
        self.cards.iter().filter(|p| !p.hidden).collect()
    }

    /// Whether anything has been arranged, so the dashboard can say "this is
    /// the arrangement you were given" rather than implying you chose it.
    pub fn is_default(&self) -> bool {
        *self == Layout::default()
    }

    /// Every card back, in the order Atlas ships.
    pub fn reset(&mut self) {
        *self = Layout::default();
    }

    /// Nothing left to look at.
    ///
    /// Hiding every card is allowed — it is your dashboard — but a blank page
    /// with no explanation is indistinguishable from a broken one, so the
    /// caller is told to say something.
    pub fn is_empty(&self) -> bool {
        self.visible().is_empty()
    }
}

/// What a move request asked for.
///
/// Parsed away from the HTTP layer so the rules are testable without a socket,
/// and so the drag path and the button path produce the same value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Move {
    Up(Card),
    Down(Card),
    To(Card, usize),
    Hide(Card),
    Show(Card),
    Widen(Card),
    Reset,
}

impl Move {
    /// Read a move out of form fields.
    ///
    /// Returns `None` on anything it does not recognise rather than picking a
    /// default. A dashboard that rearranges itself because a field arrived
    /// misspelled is worse than one that does nothing.
    pub fn parse(what: &str, card: Option<&str>, to: Option<&str>) -> Option<Move> {
        if what == "reset" {
            return Some(Move::Reset);
        }
        let card = Card::from_key(card?)?;
        match what {
            "up" => Some(Move::Up(card)),
            "down" => Some(Move::Down(card)),
            "hide" => Some(Move::Hide(card)),
            "show" => Some(Move::Show(card)),
            "widen" => Some(Move::Widen(card)),
            "to" => to?.parse().ok().map(|n| Move::To(card, n)),
            _ => None,
        }
    }
}

impl Layout {
    /// Apply a move. `true` when something actually changed, so the caller can
    /// skip writing a file that would be identical.
    pub fn apply(&mut self, m: &Move) -> bool {
        match m {
            Move::Up(c) => self.move_up(*c),
            Move::Down(c) => self.move_down(*c),
            Move::To(c, n) => self.place(*c, *n),
            Move::Hide(c) => self.set_hidden(*c, true),
            Move::Show(c) => self.set_hidden(*c, false),
            Move::Widen(c) => self.widen(*c),
            Move::Reset => {
                let was = self.clone();
                self.reset();
                was != *self
            }
        }
    }
}
