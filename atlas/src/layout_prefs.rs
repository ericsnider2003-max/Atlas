//! Arranging the hub the way you want it.
//!
//! A dashboard someone else laid out is one you read once. The reason
//! drag-and-drop matters isn't the dragging — it's that after ten minutes of
//! rearranging, the thing you look at first is the thing you actually care
//! about, and that's what makes you open it again tomorrow.
//!
//! What's kept is an order and a size, nothing more. No pixel positions: a
//! layout pinned to coordinates breaks the moment you use a different screen,
//! and you have three.

use serde::{Deserialize, Serialize};

/// Something that can sit on the hub.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Block {
    /// What needs you, now.
    Now,
    /// The board.
    Board,
    /// Counts.
    Numbers,
    /// What Atlas is doing this moment.
    Running,
    /// Messages that need a reply.
    Messages,
    /// Content in progress.
    Content,
    /// A client's work, on its own.
    Client,
    /// What moved yesterday.
    Yesterday,
    /// What Atlas has been thinking.
    Thinking,
    /// Money.
    Ledger,
}

impl Block {
    pub fn title(&self) -> &'static str {
        match self {
            Block::Now => "Now",
            Block::Board => "Everything",
            Block::Numbers => "At a glance",
            Block::Running => "Running",
            Block::Messages => "Messages",
            Block::Content => "Content",
            Block::Client => "Client",
            Block::Yesterday => "Yesterday",
            Block::Thinking => "Thinking",
            Block::Ledger => "Money",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Size {
    Small,
    Half,
    Full,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Placed {
    pub block: Block,
    pub size: Size,
    /// Narrow it to one client or project.
    pub about: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    pub name: String,
    /// Top to bottom. The order is the whole layout.
    pub blocks: Vec<Placed>,
}

impl Layout {
    /// What ships. Deliberately short — a dashboard with nine panels is one
    /// where nothing stands out.
    pub fn default_layout() -> Layout {
        Layout {
            name: "Home".into(),
            blocks: vec![
                Placed { block: Block::Now, size: Size::Full, about: None },
                Placed { block: Block::Numbers, size: Size::Full, about: None },
                Placed { block: Block::Board, size: Size::Full, about: None },
                Placed { block: Block::Messages, size: Size::Half, about: None },
            ],
        }
    }

    pub fn add(&mut self, block: Block, size: Size, about: Option<String>) {
        self.blocks.push(Placed { block, size, about });
    }

    pub fn remove(&mut self, at: usize) -> bool {
        if at >= self.blocks.len() {
            return false;
        }
        self.blocks.remove(at);
        true
    }

    /// Back to how it shipped.
    pub fn reset(&mut self) {
        let d = Layout::default_layout();
        self.blocks = d.blocks;
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct LayoutConfig {
    /// Which layout you're looking at.
    pub current: String,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        LayoutConfig { current: "Home".into() }
    }
}

/// The drag handling, as plain HTML.
///
/// No framework: the hub has to work when everything else is broken, and a
/// build step is a thing that can be broken.
pub const DRAG_SCRIPT: &str = r#"
document.querySelectorAll('[data-block]').forEach(function (el) {
  el.draggable = true;
  el.addEventListener('dragstart', function (e) {
    e.dataTransfer.setData('text/plain', el.dataset.block);
    el.classList.add('lifting');
  });
  el.addEventListener('dragend', function () { el.classList.remove('lifting'); });
  el.addEventListener('dragover', function (e) { e.preventDefault(); el.classList.add('over'); });
  el.addEventListener('dragleave', function () { el.classList.remove('over'); });
  el.addEventListener('drop', function (e) {
    e.preventDefault();
    el.classList.remove('over');
    var from = e.dataTransfer.getData('text/plain');
    var f = new FormData();
    f.append('from', from);
    f.append('to', el.dataset.block);
    fetch('/hub/layout/move', { method: 'POST', body: new URLSearchParams(f) })
      .then(function () { location.reload(); });
  });
});
"#;
