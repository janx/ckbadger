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
        let current = *history.back()?;
        let mut min = f64::INFINITY;
        let mut max = 0.0_f64;
        let mut sum = 0.0_f64;
        let mut count = 0usize;

        for &value in history {
            if value <= 0.0 {
                continue;
            }
            min = min.min(value);
            max = max.max(value);
            sum += value;
            count += 1;
        }

        if count == 0 {
            return Some(Self {
                min: 0.0,
                max: 0.0,
                current,
                avg: 0.0,
            });
        }

        Some(Self {
            min,
            max,
            current,
            avg: sum / count as f64,
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

fn gradient_color(position: f64) -> Color {
    if position < 0.33 {
        let t = position / 0.33;
        Color::Rgb(0, (255.0 - 51.0 * t) as u8, (65.0 - 14.0 * t) as u8)
    } else if position < 0.66 {
        let t = (position - 0.33) / 0.33;
        Color::Rgb(0, (204.0 - 76.0 * t) as u8, (51.0 - 20.0 * t) as u8)
    } else {
        let t = (position - 0.66) / 0.34;
        Color::Rgb(0, (128.0 - 88.0 * t) as u8, (31.0 - 21.0 * t) as u8)
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
                    color: Color::Rgb(90, 106, 127),
                };
                char_height
            ],
        };
    }

    let chart_width = char_width.saturating_sub(Y_AXIS_WIDTH);
    let mut canvas = BrailleCanvas::new(chart_width, char_height);
    let pixel_width = chart_width * 2;
    let pixel_height = char_height * 4;

    let data_len = data.len();
    let visible_start = data_len.saturating_sub(pixel_width);
    let visible_len = data_len.saturating_sub(visible_start);

    let visible_max = data
        .iter()
        .skip(visible_start)
        .take(visible_len)
        .fold(0.0_f64, |max_value, &value| max_value.max(value));
    let max_val = visible_max.max(100.0) * 1.05;

    let start_x = pixel_width.saturating_sub(visible_len);

    for (i, &val) in data
        .iter()
        .skip(visible_start)
        .take(visible_len)
        .enumerate()
    {
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

/// Stacked bar chart: three series rendered as stacked bars on a shared canvas.
/// Series are ordered bottom-to-top. Each row is colored by the band it falls in,
/// determined by the average proportions across visible data.
pub fn render_stacked_bar_chart(
    series: [&VecDeque<f64>; 3],
    colors: [Color; 3],
    char_width: usize,
    char_height: usize,
) -> BarChartResult {
    let len = series[0].len().min(series[1].len()).min(series[2].len());
    if len == 0 || char_width < Y_AXIS_WIDTH + 2 || char_height < 1 {
        return BarChartResult {
            rows: vec![
                ChartRow {
                    content: " ".repeat(char_width),
                    color: Color::Rgb(90, 106, 127),
                };
                char_height
            ],
        };
    }

    let chart_width = char_width.saturating_sub(Y_AXIS_WIDTH);
    let mut canvas = BrailleCanvas::new(chart_width, char_height);
    let pixel_width = chart_width * 2;
    let pixel_height = char_height * 4;

    let visible_start = len.saturating_sub(pixel_width);
    let visible_len = len.saturating_sub(visible_start);

    // Compute combined totals for visible range and find max.
    let mut visible_max = 0.0_f64;
    let mut sum_s0 = 0.0_f64;
    let mut sum_s1 = 0.0_f64;
    let mut sum_s2 = 0.0_f64;
    for ((&s0, &s1), &s2) in series[0]
        .iter()
        .zip(series[1].iter())
        .zip(series[2].iter())
        .skip(visible_start)
        .take(visible_len)
    {
        let total = s0 + s1 + s2;
        visible_max = visible_max.max(total);
        sum_s0 += s0;
        sum_s1 += s1;
        sum_s2 += s2;
    }

    let max_val = visible_max.max(1.0) * 1.05;
    let start_x = pixel_width.saturating_sub(visible_len);

    for i in 0..visible_len {
        let idx = visible_start + i;
        let total = series[0][idx] + series[1][idx] + series[2][idx];
        let bar_height = if max_val > 0.0 {
            ((total / max_val) * pixel_height as f64).round() as usize
        } else {
            0
        };
        if bar_height > 0 {
            let x = start_x + i;
            let top_y = pixel_height.saturating_sub(bar_height);
            let bottom_y = pixel_height.saturating_sub(1);
            canvas.fill_column(x, top_y, bottom_y);
        }
    }

    let chart_lines = canvas.render();

    // Compute color bands from average proportions across visible data.
    let grand_total = sum_s0 + sum_s1 + sum_s2;
    let (band0_frac, band1_frac) = if grand_total > 0.0 {
        (sum_s0 / grand_total, (sum_s0 + sum_s1) / grand_total)
    } else {
        (0.33, 0.66)
    };

    let mut rows = Vec::with_capacity(char_height);
    for (i, chart_line) in chart_lines.iter().enumerate() {
        // position 0.0 = top, 1.0 = bottom; bands are bottom-up: s0 at bottom
        let position = 1.0 - (i as f64 / (char_height.max(1) - 1).max(1) as f64);
        let color = if position < band0_frac {
            colors[0] // bottom band (series 0)
        } else if position < band1_frac {
            colors[1] // middle band (series 1)
        } else {
            colors[2] // top band (series 2)
        };

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
            Color::Rgb(r, g, _) => {
                assert_eq!(r, 0);
                assert_eq!(g, 255);
            }
            _ => panic!("Expected RGB color"),
        }
        match bottom {
            Color::Rgb(r, _, _) => assert_eq!(r, 0),
            _ => panic!("Expected RGB color"),
        }
    }

    #[test]
    fn test_stacked_bar_chart() {
        let mut s0 = VecDeque::new();
        let mut s1 = VecDeque::new();
        let mut s2 = VecDeque::new();
        for i in 0..50 {
            s0.push_back((i as f64) * 5.0 + 100.0); // build: dominant
            s1.push_back((i as f64) * 0.5 + 10.0); // fetch_wait: small
            s2.push_back((i as f64) * 0.2 + 5.0); // flush_wait: smallest
        }
        let colors = [
            Color::Rgb(0, 255, 65),
            Color::Rgb(255, 176, 0),
            Color::Rgb(255, 80, 80),
        ];
        let result = render_stacked_bar_chart([&s0, &s1, &s2], colors, 40, 6);
        assert_eq!(result.rows.len(), 6);
        assert!(result.rows[0].content.len() >= 40);
        // Bottom rows should be green (build dominates ~85%)
        assert_eq!(result.rows[5].color, colors[0]);
    }

    #[test]
    fn test_stacked_bar_chart_empty() {
        let s0 = VecDeque::new();
        let s1 = VecDeque::new();
        let s2 = VecDeque::new();
        let colors = [
            Color::Rgb(0, 255, 65),
            Color::Rgb(255, 176, 0),
            Color::Rgb(255, 80, 80),
        ];
        let result = render_stacked_bar_chart([&s0, &s1, &s2], colors, 40, 6);
        assert_eq!(result.rows.len(), 6);
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
