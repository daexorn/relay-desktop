use std::sync::OnceLock;

pub const SIZE: usize = 12;
pub type Pixels = [[Option<[u8; 3]>; SIZE]; SIZE];

/// The same artwork is packaged with Android and rendered with Braille in the TUI.
pub fn pixels(backend: &str) -> &'static Pixels {
    static ICONS: OnceLock<[Pixels; 4]> = OnceLock::new();
    let index = match backend {
        "codex" => 1,
        "agy" => 2,
        "api" => 3,
        _ => 0,
    };
    &ICONS.get_or_init(|| {
        let mut icons = [[[None; SIZE]; SIZE]; 4];
        for (icon, line) in icons
            .iter_mut()
            .zip(include_str!("../assets/agent-icons.txt").lines())
        {
            let mut fields = line.split_whitespace();
            fields.next(); // Engine name, also used by Android.
            let colors: Vec<[u8; 3]> = fields
                .next()
                .unwrap()
                .split(',')
                .map(|hex| {
                    let n = u32::from_str_radix(hex, 16).unwrap();
                    [(n >> 16) as u8, (n >> 8) as u8, n as u8]
                })
                .collect();
            for (row, text) in icon.iter_mut().zip(fields.next().unwrap().split('/')) {
                for (pixel, value) in row.iter_mut().zip(text.chars()) {
                    *pixel = value.to_digit(10).map(|n| colors[n as usize]);
                }
            }
        }
        icons
    })[index]
}
