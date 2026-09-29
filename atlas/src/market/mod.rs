//! Reading a market: structure, levels, regime, sessions, news -- and not
//! seeing the future.
//!
//! Pure `std`. No external crates, deliberately: this is the part of Atlas that
//! has to run offline on a bare Windows machine, and every dependency here is
//! a thing that can fail to build on the machine that matters. The arithmetic is small
//! enough to own, and the two places it would normally be borrowed -- a
//! timezone database and a calendar crate -- are exactly the two places Windows
//! makes borrowing unreliable.

pub mod bars;
pub mod claims;
pub mod events;
pub mod feed;
pub mod fixtures;
pub mod levels;
pub mod multiframe;
pub mod params;
pub mod regime;
pub mod session;
pub mod structure;
pub mod time;
pub mod timeframe;
pub mod verify;
