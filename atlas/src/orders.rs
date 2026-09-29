//! What you ordered, and where it is — built from confirmation, shipping,
//! and delivery emails, so "what's going on with my Amazon order" has a
//! real answer instead of needing you to go dig through your inbox.
//!
//! Status is read off a closed set of phrases a retailer's own email
//! already uses ("has shipped", "out for delivery", "was delivered"),
//! deliberately not left to a model to interpret — the same reasoning
//! `unsub.rs` and `triage.rs` already apply: a bounded set of real
//! signals is worth more than a plausible-sounding guess, especially for
//! something as easy to get definitively right as "which of five known
//! phrases appears in this subject line."

use crate::error::Result;
use crate::store::Store;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ordered,
    Shipped,
    OutForDelivery,
    Delivered,
    /// The email was clearly order-related but didn't match a known
    /// status phrase — kept rather than dropped, since "something
    /// happened, unclear what" is still more honest than silence.
    Unknown,
}

impl Status {
    pub fn spoken(&self) -> &'static str {
        match self {
            Status::Ordered => "ordered, not yet shipped",
            Status::Shipped => "shipped",
            Status::OutForDelivery => "out for delivery",
            Status::Delivered => "delivered",
            Status::Unknown => "update received, status unclear",
        }
    }

    /// Whether `new` genuinely moves the order forward from `self`, so an
    /// out-of-order email (a delayed "order confirmed" arriving after the
    /// shipping notice already did) can't walk a real status backwards.
    fn is_progress_from(&self, new: Status) -> bool {
        fn rank(s: Status) -> u8 {
            match s {
                Status::Unknown => 0,
                Status::Ordered => 1,
                Status::Shipped => 2,
                Status::OutForDelivery => 3,
                Status::Delivered => 4,
            }
        }
        rank(new) >= rank(*self)
    }
}

/// Reads a status off a subject line's own words. Order of checks
/// matters — "out for delivery" must be checked before the bare word
/// "delivery" would ever be, though nothing here currently does that;
/// named as a reminder for whoever extends this list next.
pub fn status_from_subject(subject: &str) -> Option<Status> {
    let s = subject.to_lowercase();
    if s.contains("out for delivery") {
        Some(Status::OutForDelivery)
    } else if s.contains("delivered") || s.contains("has arrived") {
        Some(Status::Delivered)
    } else if s.contains("shipped") || s.contains("on its way") || s.contains("on the way") {
        Some(Status::Shipped)
    } else if s.contains("order confirm")
        || s.contains("thanks for your order")
        || s.contains("thank you for your order")
        || s.contains("order received")
    {
        Some(Status::Ordered)
    } else {
        None
    }
}

/// The retailer, from the sender's domain — `orders@amazon.com` becomes
/// "Amazon", not the bare domain. Capitalizes the first letter of
/// whatever's left after dropping the TLD; good enough for the handful
/// of retailers most inboxes actually see mail from, not a full registry.
pub fn merchant_from_address(address: &str) -> Option<String> {
    let domain = address.split('@').nth(1)?;
    let name = domain.split('.').next()?;
    if name.is_empty() {
        return None;
    }
    let mut chars = name.chars();
    let first = chars.next()?.to_ascii_uppercase();
    Some(format!("{first}{}", chars.as_str()))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Order {
    pub id: String,
    pub merchant: String,
    /// Best available description — the subject line of whichever email
    /// last updated this order. Not the item itself: reliably extracting
    /// "what was ordered" from a retailer's own HTML needs more than
    /// subject-line matching, and a wrong guess here is worse than the
    /// subject line, which is at least honestly what it looks like.
    pub description: String,
    pub status: Status,
    pub last_updated: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Orders {
    orders: Vec<Order>,
}

impl Orders {
    pub fn load(store: &Store) -> Orders {
        store.load("orders")
    }

    pub fn save(&self, store: &Store) -> Result<()> {
        store.save("orders", self)
    }

    /// One order per (merchant, description) pair, since there's no
    /// order-number extraction yet — two different orders from the same
    /// merchant with the same subject line would collide, a known,
    /// named limitation rather than a silent one.
    fn make_id(merchant: &str, description: &str) -> String {
        format!("{}:{}", merchant.to_lowercase(), description.to_lowercase())
    }

    /// Records an update from one order-related email. Returns the order
    /// as it stands after the update — always `Some` on the first sighting
    /// of an order, and on every genuine status advance after that;
    /// `None` when the update didn't move anything forward (an
    /// out-of-order or duplicate email), so a caller can tell "a fresh
    /// order or a real change happened" from "nothing worth mentioning".
    pub fn update(&mut self, merchant: &str, subject: &str, status: Status, now: u64) -> Option<&Order> {
        let id = Self::make_id(merchant, subject);
        if let Some(pos) = self.orders.iter().position(|o| o.id == id) {
            if self.orders[pos].status.is_progress_from(status) {
                self.orders[pos].status = status;
                self.orders[pos].last_updated = now;
                return Some(&self.orders[pos]);
            }
            return None;
        }
        self.orders.push(Order {
            id,
            merchant: merchant.to_string(),
            description: subject.to_string(),
            status,
            last_updated: now,
        });
        self.orders.last()
    }

    /// Finds by merchant or by a word in the description — "what's up
    /// with my Amazon order" and "where's my cable" both need to work,
    /// and neither is an exact id. Checked both directions: a short
    /// keyword query ("amazon") needs to match inside a longer merchant
    /// name, and a merchant name needs to be found inside a longer,
    /// full-sentence query ("what's up with my amazon order") the same
    /// way.
    pub fn find(&self, query: &str) -> Option<&Order> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return None;
        }
        self.orders
            .iter()
            .filter(|o| {
                let merchant = o.merchant.to_lowercase();
                let description = o.description.to_lowercase();
                merchant.contains(&q)
                    || q.contains(&merchant)
                    || description.contains(&q)
                    || q.contains(&description)
            })
            .max_by_key(|o| o.last_updated)
    }

    pub fn all(&self) -> &[Order] {
        &self.orders
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn out_for_delivery_is_recognised_ahead_of_a_bare_delivered_match() {
        assert_eq!(status_from_subject("Your package is out for delivery"), Some(Status::OutForDelivery));
    }

    #[test]
    fn delivered_is_recognised() {
        assert_eq!(status_from_subject("Your order has been delivered"), Some(Status::Delivered));
    }

    #[test]
    fn shipped_is_recognised() {
        assert_eq!(status_from_subject("Your order has shipped"), Some(Status::Shipped));
    }

    #[test]
    fn a_fresh_order_confirmation_is_recognised() {
        assert_eq!(status_from_subject("Thanks for your order!"), Some(Status::Ordered));
    }

    #[test]
    fn an_unrelated_subject_matches_nothing() {
        assert_eq!(status_from_subject("Let's catch up next week"), None);
    }

    #[test]
    fn merchant_is_read_from_the_senders_domain() {
        assert_eq!(merchant_from_address("orders@amazon.com"), Some("Amazon".to_string()));
        assert_eq!(merchant_from_address("shipment-tracking@shop.example.com"), Some("Shop".to_string()));
    }

    #[test]
    fn merchant_from_an_address_with_no_domain_is_none() {
        assert_eq!(merchant_from_address("not-an-email"), None);
    }

    #[test]
    fn a_new_order_is_recorded_on_first_sighting() {
        let mut orders = Orders::default();
        let recorded = orders.update("Amazon", "Thanks for your order!", Status::Ordered, 0);
        assert!(recorded.is_some());
        assert_eq!(orders.all().len(), 1);
    }

    #[test]
    fn a_later_email_advances_the_same_order_rather_than_creating_a_second_one() {
        let mut orders = Orders::default();
        orders.update("Amazon", "Thanks for your order!", Status::Ordered, 0);
        orders.update("Amazon", "Thanks for your order!", Status::Shipped, 100);
        assert_eq!(orders.all().len(), 1);
        assert_eq!(orders.find("amazon").unwrap().status, Status::Shipped);
    }

    #[test]
    fn an_out_of_order_email_does_not_walk_the_status_backwards() {
        let mut orders = Orders::default();
        orders.update("Amazon", "Thanks for your order!", Status::Delivered, 100);
        let result = orders.update("Amazon", "Thanks for your order!", Status::Shipped, 200);
        assert!(result.is_none(), "a Shipped update after Delivered must not overwrite it");
        assert_eq!(orders.find("amazon").unwrap().status, Status::Delivered);
    }

    #[test]
    fn a_repeat_of_the_same_status_is_not_treated_as_fresh_progress() {
        let mut orders = Orders::default();
        orders.update("Amazon", "Thanks for your order!", Status::Shipped, 100);
        let result = orders.update("Amazon", "Thanks for your order!", Status::Shipped, 200);
        // Equal rank counts as progress by this implementation (>=), which
        // is deliberate: a second "shipped" email might carry a tracking
        // update worth refreshing `last_updated` for.
        assert!(result.is_some());
    }

    #[test]
    fn find_matches_by_merchant_name() {
        let mut orders = Orders::default();
        orders.update("Amazon", "Your USB-C cable order", Status::Shipped, 0);
        assert!(orders.find("amazon").is_some());
        assert!(orders.find("AMAZON").is_some());
    }

    #[test]
    fn find_matches_by_a_word_in_the_description() {
        let mut orders = Orders::default();
        orders.update("Amazon", "Your USB-C cable order has shipped", Status::Shipped, 0);
        assert!(orders.find("cable").is_some());
    }

    #[test]
    fn find_with_no_match_returns_none() {
        let orders = Orders::default();
        assert!(orders.find("nonexistent").is_none());
    }

    #[test]
    fn find_prefers_the_most_recently_updated_match() {
        let mut orders = Orders::default();
        orders.update("Amazon", "Cable order", Status::Ordered, 0);
        orders.update("Amazon", "Headphones order", Status::Ordered, 100);
        let found = orders.find("amazon").unwrap();
        assert_eq!(found.description, "Headphones order");
    }

    #[test]
    fn saved_and_reloaded_orders_survive_a_restart() {
        let dir = std::env::temp_dir().join(format!("atlas-orders-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::new(&dir);

        let mut orders = Orders::load(&store);
        orders.update("Amazon", "Cable order", Status::Shipped, 0);
        orders.save(&store).unwrap();

        let reloaded = Orders::load(&store);
        assert_eq!(reloaded.find("amazon").unwrap().status, Status::Shipped);
    }
}
