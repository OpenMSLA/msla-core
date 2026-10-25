use crate::types::{
    model::{
        GlobalPrintingMeta,
        ir::{PrintingIR, TimedIR, ZMoving},
    },
    peripheral::StepperPositioning,
};
use anyhow::{Result, anyhow};
use std::{cmp::max, path::Path, str::SplitWhitespace, time::Duration};

macro_rules! try_parse_number {
    ($n:ident, $ty:ty) => {
        $n.parse::<$ty>().ok()
    };
}

pub struct GCodeParser {
    pub ir: Vec<TimedIR>,
    pub meta: GlobalPrintingMeta,

    layer_counter: usize,
    positioning: StepperPositioning,
    last_move_speed: f64,
}

impl GCodeParser {
    pub fn new() -> Self {
        Self {
            ir: Vec::new(),
            meta: GlobalPrintingMeta::default(),
            layer_counter: 0,
            positioning: StepperPositioning::Absolute,
            last_move_speed: 50.0,
        }
    }

    /// Parse gcode file into PrintingIR
    pub fn parse_gcode(&mut self, gcode: String) -> Result<()> {
        for line in gcode.lines() {
            let (code, comment) = line.split_once(';').unwrap_or((line, ""));

            // Parse comment (probably meta)
            if !comment.is_empty() {
                self.parse_comment(comment);
            }

            let mut parts = code.split_whitespace();

            let Some(command) = parts.next() else {
                continue;
            };

            match command {
                // Movement
                "G0" | "G1" => self.parse_g0(parts),

                // Delay
                "G4" => self.parse_g4(parts)?,

                // mm units
                "G21" => {},

                // homing
                "G28" => self.ir.push(PrintingIR::Home.to_timed_ir()),

                // absolute pos
                "G90" => self.positioning = StepperPositioning::Absolute,

                // relative pos
                "G91" => self.positioning = StepperPositioning::Relative,

                // Enable steppers
                "M17" => self.ir.push(PrintingIR::EnableSteppers.to_timed_ir()),

                // disable steppers
                "M18" => self.ir.push(PrintingIR::DisableSteppers.to_timed_ir()),

                // UV power
                "M106" => {
                    self.parse_m106(parts);
                },

                // Force error
                "M488" => {
                    self.parse_m488(parts);
                },

                // Stepper current
                "M906" => {
                    self.parse_m906(parts);
                },

                // Show image
                "M6054" => {
                    self.parse_m6054(parts);
                },

                gc => return Err(anyhow!("Unknown gcode: {gc}")),
            };
        }

        self.meta.total_layer_count = max(self.meta.total_layer_count, self.layer_counter);
        Ok(())
    }

    // Parse G0 G1 codes
    fn parse_g0(&mut self, iter: SplitWhitespace) {
        let mut z_pos: Option<f64> = None;
        let mut z_speed: Option<f64> = None;

        for part in iter {
            let mut chars = part.chars();
            let Some(letter) = chars.next() else {
                continue;
            };

            match letter {
                // Z position
                'Z' => {
                    z_pos = chars.as_str().parse::<f64>().ok();
                },

                // Feedrate
                'F' => {
                    z_speed = chars.as_str().parse::<f64>().ok();
                },

                _ => { // unknown parameter 
                },
            }
        }

        // If no Z-axis command is found in the current G-code, skip it
        let Some(pos) = z_pos else { return };

        if let Some(speed) = z_speed {
            // Memorize speed for "sticky" effect
            self.last_move_speed = speed;
        }

        self.ir.push(
            PrintingIR::MoveZ(ZMoving::new(
                pos,
                z_speed.unwrap_or(self.last_move_speed),
                self.positioning.clone(),
            ))
            .to_timed_ir(),
        )
    }

    /// Parse dwell
    fn parse_g4(&mut self, mut iter: SplitWhitespace) -> Result<()> {
        let mut chars = iter.next().ok_or(anyhow!("Unknown dwell"))?.chars();
        let letter = chars.next().ok_or(anyhow!("Unknown dwell: {:?}", chars))?;

        let duration = match letter {
            'P' => Duration::from_millis(chars.as_str().parse::<u64>()?),

            'S' => Duration::from_secs(chars.as_str().parse::<u64>()?),

            _ => return Err(anyhow!("Unknown dwell")),
        };

        if !duration.is_zero() {
            self.ir.push(PrintingIR::Wait(duration).to_timed_ir());
        }

        Ok(())
    }

    /// Parse uv led power
    fn parse_m106(&mut self, mut iter: SplitWhitespace) -> bool {
        let Some(power) = iter.find(|p| p.starts_with("S")) else {
            return false;
        };

        if let Some(power) = power
            .strip_prefix("S")
            .and_then(|power| power.parse::<f32>().ok())
        {
            self.ir.push(PrintingIR::TurnUV(power > 0.0).to_timed_ir());
        }

        true
    }

    /// Forcing the printer into an error state
    fn parse_m488(&mut self, mut iter: SplitWhitespace) -> bool {
        let Some(first) = iter.next() else {
            return false;
        };

        let mut message_parts = vec![first];
        if first.starts_with("\"") {
            for part in iter {
                message_parts.push(part);
                if part.ends_with("\"") {
                    break;
                }
            }
        }

        self.ir.push(
            PrintingIR::ForceErr(message_parts.join(" ").trim_matches('"').to_owned())
                .to_timed_ir(),
        );

        true
    }

    /// Set stepper current
    fn parse_m906(&mut self, mut iter: SplitWhitespace) -> bool {
        let Some(current) = iter.find(|p| p.starts_with("Z")) else {
            return false;
        };

        if let Some(current) = current
            .strip_prefix("Z")
            .and_then(|current| current.parse::<u16>().ok())
        {
            self.ir
                .push(PrintingIR::SetStepperCurrent(current).to_timed_ir());
        }

        true
    }

    /// Show image
    fn parse_m6054(&mut self, mut iter: SplitWhitespace) -> bool {
        let Some(filename) = iter.next() else {
            return false;
        };

        self.layer_counter += 1;
        self.ir.push(
            PrintingIR::ShowImage(Path::new(&filename.trim_matches('"')).to_path_buf())
                .to_timed_ir(),
        );

        true
    }

    /// Parse metadata from comments
    fn parse_comment(&mut self, line: &str) -> bool {
        let (comm, data) = line.split_once(':').unwrap_or((line, ""));

        match comm.trim().to_lowercase().as_str() {
            "volume" => {
                self.meta.volume = try_parse_number!(data, f32);
            },

            "filename" => {
                self.meta.file_name = Some(data.to_owned());
            },

            "weight" => {
                self.meta.weight = try_parse_number!(data, f32);
            },

            "price" => {
                self.meta.price = try_parse_number!(data, f32);
            },

            "layerheight" => {
                self.meta.layer_height = try_parse_number!(data, f32);
            },

            "totallayer" => {
                let ex = self.meta.total_layer_count;

                if let Some(n) = try_parse_number!(data, usize) {
                    self.meta.total_layer_count = max(ex, n);
                }
            },

            _ => {},
        }

        true
    }
}
