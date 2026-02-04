use ratatui::style::Color;
use std::collections::VecDeque;

pub struct ChartStats {
    pub min: f64,
    pub max: f64,
    pub current: f64,
    pub avg: f64,
}

impl ChartStats {
    pub fn from_history(history: &VecDeque<f64>) -> Option<Self> {
        if history.is_empty() {
            return None;
        }
        let min = history.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = history.iter().cloned().fold(0.0_f64, f64::max);
        let current = *history.back().unwrap_or(&0.0);
        let avg = history.iter().sum::<f64>() / history.len() as f64;
        Some(Self {
            min,
            max,
            current,
            avg,
        })
    }
}

const BRAILLE_OFFSET: u32 = 0x2800;
const BRAILLE_DOTS: [[u32; 2]; 4] = [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]];

pub struct BrailleCanvas {
    width: usize,
    height: usize,
    dots: Vec<Vec<bool>>,
}

impl BrailleCanvas {
    pub fn new(char_width: usize, char_height: usize) -> Self {
        let width = char_width * 2;
        let height = char_height * 4;
        Self {
            width,
            height,
            dots: vec![vec![false; width]; height],
        }
    }

    pub fn set(&mut self, x: usize, y: usize) {
        if x < self.width && y < self.height {
            self.dots[y][x] = true;
        }
    }

    pub fn fill_column(&mut self, x: usize, from_y: usize, to_y: usize) {
        for y in from_y..=to_y.min(self.height.saturating_sub(1)) {
            self.set(x, y);
        }
    }

    pub fn render(&self) -> Vec<String> {
        let char_height = self.height.div_ceil(4);
        let char_width = self.width.div_ceil(2);

        let mut lines = Vec::with_capacity(char_height);
        for row in 0..char_height {
            let mut line = String::with_capacity(char_width);
            for col in 0..char_width {
                let mut code = BRAILLE_OFFSET;
                for (dy, dot_row) in BRAILLE_DOTS.iter().enumerate() {
                    for (dx, &dot) in dot_row.iter().enumerate() {
                        let y = row * 4 + dy;
                        let x = col * 2 + dx;
                        if y < self.height && x < self.width && self.dots[y][x] {
                            code |= dot;
                        }
                    }
                }
                line.push(char::from_u32(code).unwrap_or(' '));
            }
            lines.push(line);
        }
        lines
    }
}

#[derive(Clone)]
pub struct ChartRow {
    pub content: String,
    pub color: Color,
}

pub struct BarChartResult {
    pub rows: Vec<ChartRow>,
}

// Gradient: position 0.0 (top) = Red -> Yellow -> Green -> Cyan = position 1.0 (bottom)
fn gradient_color(position: f64) -> Color {
    if position < 0.25 {
        let t = position / 0.25;
        Color::Rgb(255, (128.0 + 127.0 * t) as u8, 0)
    } else if position < 0.5 {
        let t = (position - 0.25) / 0.25;
        Color::Rgb((255.0 * (1.0 - t)) as u8, 255, 0)
    } else if position < 0.75 {
        let t = (position - 0.5) / 0.25;
        Color::Rgb(0, 255, (255.0 * t) as u8)
    } else {
        let t = (position - 0.75) / 0.25;
        Color::Rgb(0, (255.0 * (1.0 - t * 0.3)) as u8, 255)
    }
}

fn format_axis_value(value: f64) -> String {
    if value >= 1_000_000.0 {
        format!("{:.0}M", value / 1_000_000.0)
    } else if value >= 10_000.0 {
        format!("{:.0}K", value / 1_000.0)
    } else if value >= 1_000.0 {
        format!("{:.1}K", value / 1_000.0)
    } else {
        format!("{:.0}", value)
    }
}

const Y_AXIS_WIDTH: usize = 6;

pub fn render_bar_chart(
    data: &VecDeque<f64>,
    char_width: usize,
    char_height: usize,
) -> BarChartResult {
    if data.is_empty() || char_width < Y_AXIS_WIDTH + 2 || char_height < 1 {
        return BarChartResult {
            rows: vec![
                ChartRow {
                    content: " ".repeat(char_width),
                    color: Color::Gray
                };
                char_height
            ],
        };
    }

    let chart_width = char_width.saturating_sub(Y_AXIS_WIDTH);
    let mut canvas = BrailleCanvas::new(chart_width, char_height);
    let pixel_width = chart_width * 2;
    let pixel_height = char_height * 4;

    let num_bars = pixel_width;
    let display_data: Vec<f64> = if data.len() > num_bars {
        data.iter().skip(data.len() - num_bars).copied().collect()
    } else {
        data.iter().copied().collect()
    };

    let max_val = display_data
        .iter()
        .cloned()
        .fold(0.0_f64, f64::max)
        .max(100.0)
        * 1.05;

    let start_x = if display_data.len() < num_bars {
        num_bars - display_data.len()
    } else {
        0
    };

    for (i, &val) in display_data.iter().enumerate() {
        let x = start_x + i;

        let bar_height = if max_val > 0.0 {
            ((val / max_val) * pixel_height as f64).round() as usize
        } else {
            0
        };

        if bar_height > 0 {
            let top_y = pixel_height.saturating_sub(bar_height);
            let bottom_y = pixel_height.saturating_sub(1);
            canvas.fill_column(x, top_y, bottom_y);
        }
    }

    let chart_lines = canvas.render();

    let mut rows = Vec::with_capacity(char_height);
    for (i, chart_line) in chart_lines.iter().enumerate() {
        let position = i as f64 / (char_height.max(1) - 1).max(1) as f64;
        let color = gradient_color(position);

        let y_label = if i == 0 {
            format!("{:>5}│", format_axis_value(max_val))
        } else if i == char_height / 2 {
            format!("{:>5}│", format_axis_value(max_val / 2.0))
        } else if i == char_height - 1 {
            format!("{:>5}│", "0")
        } else {
            "     │".to_string()
        };

        let content = format!("{}{}", y_label, chart_line);
        rows.push(ChartRow { content, color });
    }

    BarChartResult { rows }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chart_stats() {
        let mut history = VecDeque::new();
        history.push_back(100.0);
        history.push_back(200.0);
        history.push_back(150.0);

        let stats = ChartStats::from_history(&history).unwrap();
        assert_eq!(stats.min, 100.0);
        assert_eq!(stats.max, 200.0);
        assert_eq!(stats.current, 150.0);
        assert_eq!(stats.avg, 150.0);
    }

    #[test]
    fn test_empty_history() {
        let history = VecDeque::new();
        assert!(ChartStats::from_history(&history).is_none());
    }

    #[test]
    fn test_braille_canvas() {
        let mut canvas = BrailleCanvas::new(10, 5);
        canvas.set(0, 0);
        canvas.set(1, 1);
        let lines = canvas.render();
        assert_eq!(lines.len(), 5);
        assert!(!lines[0].is_empty());
    }

    #[test]
    fn test_render_bar_chart() {
        let mut data = VecDeque::new();
        for i in 0..100 {
            data.push_back((i as f64) * 10.0);
        }
        let result = render_bar_chart(&data, 40, 6);
        assert_eq!(result.rows.len(), 6);
        assert!(result.rows[0].content.len() >= 40);
    }

    #[test]
    fn test_fill_column() {
        let mut canvas = BrailleCanvas::new(5, 3);
        canvas.fill_column(2, 5, 11);
        let lines = canvas.render();
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn test_gradient_colors() {
        let top = gradient_color(0.0);
        let bottom = gradient_color(1.0);
        match top {
            Color::Rgb(r, _, _) => assert_eq!(r, 255),
            _ => panic!("Expected RGB color"),
        }
        match bottom {
            Color::Rgb(_, _, b) => assert_eq!(b, 255),
            _ => panic!("Expected RGB color"),
        }
    }

    #[test]
    fn test_format_axis_value() {
        assert_eq!(format_axis_value(1500.0), "1.5K");
        assert_eq!(format_axis_value(16600.0), "17K");
        assert_eq!(format_axis_value(1500000.0), "2M");
        assert_eq!(format_axis_value(150.0), "150");
        assert_eq!(format_axis_value(15.5), "16");
    }
}
