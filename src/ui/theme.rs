//! The green-screen CRT palette shared across every panel.
use ratatui::style::Color;

pub const PHOSPHOR: Color = Color::Rgb(0, 255, 90); // bright contact / sweep head
pub const PHOSPHOR_DIM: Color = Color::Rgb(0, 150, 60); // labels, trailing sweep
pub const PHOSPHOR_FAINT: Color = Color::Rgb(0, 80, 35); // rings, faded blips
pub const AMBER: Color = Color::Rgb(255, 176, 0); // accents, the "BISHOP:" tag
pub const GRID: Color = Color::Rgb(40, 52, 72); // background lattice — cool slate so
                                                // it doesn't blend with green walls/nodes
