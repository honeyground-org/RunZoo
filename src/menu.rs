//! What the menu says, described once.
//!
//! Both platforms build their own native menu out of these nodes, so the two
//! cannot drift apart, and `--dump-menu` can print the whole thing without
//! there being a menu bar anywhere near.
use crate::animal::ANIMALS;
use crate::draw::{Spark, SPARK_LARGE, SPARK_SMALL};
use crate::metrics::Source;
use crate::state::App;
use crate::sys;
use crate::tint::PALETTE;

/// Where "buy me a coffee" goes. A Stripe payment link: the customer picks the
/// amount, five dollars to start, two at the least.
///
/// Empty would leave the row out entirely - a menu item that opens nothing is
/// worse than no menu item - which is also how this was tested before the link
/// existed.
pub const SUPPORT_URL: &str = "https://buy.stripe.com/5kQfZg7p7dCX5bv9jy8bS00";

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Cmd {
    PickSource(usize),
    PickAnimal(usize),
    PickAccent(usize),
    OpenProcesses,
    ToggleAlert,
    Support,
    Quit,
}

impl Cmd {
    /// A menu item carries one integer and nothing else on either platform, so
    /// the command has to survive as one. Kept clear of zero, which is what an
    /// item that was never given a tag reports.
    pub fn tag(self) -> u32 {
        match self {
            Cmd::PickSource(i) => 1000 + i as u32,
            Cmd::PickAnimal(i) => 2000 + i as u32,
            Cmd::PickAccent(i) => 3000 + i as u32,
            Cmd::OpenProcesses => 4001,
            Cmd::ToggleAlert => 4002,
            Cmd::Quit => 4003,
            Cmd::Support => 4004,
        }
    }

    pub fn from_tag(t: u32) -> Option<Cmd> {
        match t {
            1000..=1999 => Some(Cmd::PickSource((t - 1000) as usize)),
            2000..=2999 => Some(Cmd::PickAnimal((t - 2000) as usize)),
            3000..=3999 => Some(Cmd::PickAccent((t - 3000) as usize)),
            4001 => Some(Cmd::OpenProcesses),
            4002 => Some(Cmd::ToggleAlert),
            4003 => Some(Cmd::Quit),
            4004 => Some(Cmd::Support),
            _ => None,
        }
    }
}

/// A picture that belongs to a row. Resolved to pixels by the platform, which
/// is the only part that knows what an image is.
#[derive(Copy, Clone)]
pub enum Art {
    /// the sparkline of a source, at a size
    Spark(usize, Spark),
    /// the ramp a palette entry would paint with
    Swatch(usize),
    /// the cup on the row that asks for one
    Coffee,
}

/// A row's text: a label, then columns that hang off right-aligned stops.
pub struct Cells {
    pub label: String,
    pub cols: Vec<String>,
}

impl Cells {
    pub fn plain(label: &str) -> Self {
        Cells { label: label.to_string(), cols: Vec::new() }
    }

    /// The row as one line, columns separated by tabs. Both platforms want this
    /// shape: macOS hangs tab stops off it, Windows right-aligns after a tab.
    pub fn tabbed(&self) -> String {
        let mut s = self.label.clone();
        for c in &self.cols {
            s.push('\t');
            s.push_str(c);
        }
        s
    }
}

pub enum Node {
    Separator,
    /// A caption you cannot click
    Caption(String),
    /// A row you cannot click, whose numbers still line up
    Info(Cells),
    Row {
        cells: Cells,
        cmd: Cmd,
        checked: bool,
        /// Drawn a size up: this is the row driving the animal
        lead: bool,
        art: Option<Art>,
    },
    Sub {
        label: String,
        items: Vec<Node>,
    },
}

pub fn build(app: &App) -> Vec<Node> {
    let mut out = Vec::new();

    let row = |s: Source, lead: bool| Node::Row {
        cells: Cells {
            label: s.label().to_string(),
            cols: vec![app.metrics.detail[s.idx()].clone()],
        },
        cmd: Cmd::PickSource(s.idx()),
        checked: s == app.source,
        lead,
        art: Some(Art::Spark(s.idx(), if lead { SPARK_LARGE } else { SPARK_SMALL })),
    };

    // The source driving the animal leads, drawn larger. It is the one you
    // opened the menu to read, and it sits in the same place whichever you
    // pick, so the eye does not have to hunt for the checkmark.
    let lead_shown = app.metrics.available[app.source.idx()];
    if lead_shown {
        out.push(row(app.source, true));
        out.push(Node::Separator);
    }
    out.push(Node::Caption("Click a row to drive the animal".into()));
    for s in Source::ALL {
        if !app.metrics.available[s.idx()] || (lead_shown && s == app.source) {
            continue;
        }
        out.push(row(s, false));
    }

    out.push(Node::Separator);
    out.push(Node::Caption("Top processes".into()));
    if app.metrics.top.is_empty() {
        out.push(Node::Caption("   measuring…".into()));
    }
    for p in app.metrics.top.iter().take(5) {
        out.push(Node::Info(Cells {
            label: format!("   {}", p.name),
            cols: vec![
                format!("{:.0}%", p.cpu),
                format!("{:.0} MB", p.mem as f64 / 1024.0 / 1024.0),
            ],
        }));
    }
    // Read the culprit, then go and do something about it. Ending a process is
    // the task manager's job, not ours.
    out.push(Node::Row {
        cells: Cells::plain(sys::TASK_MANAGER),
        cmd: Cmd::OpenProcesses,
        checked: false,
        lead: false,
        art: None,
    });

    out.push(Node::Separator);

    out.push(Node::Sub {
        label: format!("Animal — {}", ANIMALS[app.animal].label),
        items: ANIMALS
            .iter()
            .enumerate()
            .map(|(i, a)| Node::Row {
                cells: Cells::plain(a.label),
                cmd: Cmd::PickAnimal(i),
                checked: i == app.animal,
                lead: false,
                art: None,
            })
            .collect(),
    });

    // Every palette entry carries its own ramp, so you pick the gradient you
    // can see rather than the name of a colour.
    let mut colours = vec![Node::Caption("idle    →    overloaded".into())];
    colours.extend(PALETTE.iter().enumerate().map(|(i, a)| Node::Row {
        cells: Cells::plain(a.label),
        cmd: Cmd::PickAccent(i),
        checked: i == app.accent,
        lead: false,
        art: Some(Art::Swatch(i)),
    }));
    out.push(Node::Sub {
        label: format!("Severity colour — {}", PALETTE[app.accent].label),
        items: colours,
    });

    out.push(Node::Row {
        cells: Cells::plain("Overload alert"),
        cmd: Cmd::ToggleAlert,
        checked: app.alert_on,
        lead: false,
        art: None,
    });

    if !SUPPORT_URL.is_empty() {
        out.push(Node::Row {
            cells: Cells::plain("Buy me a coffee"),
            cmd: Cmd::Support,
            checked: false,
            lead: false,
            art: Some(Art::Coffee),
        });
    }

    out.push(Node::Separator);
    out.push(Node::Row {
        cells: Cells::plain("Quit"),
        cmd: Cmd::Quit,
        checked: false,
        lead: false,
        art: None,
    });
    out
}
