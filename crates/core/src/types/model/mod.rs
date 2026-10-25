use crate::types::{
    model::ir::{PrintingIR, TimedIR},
    peripheral::StepperPositioning,
};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tempfile::TempDir;
pub mod ir;

#[derive(Clone, Debug, PartialEq, Default)]
pub struct GlobalPrintingMeta {
    pub file_name: Option<String>,
    pub total_layer_count: usize,
    pub estimated_printing_time: usize,
    pub volume: Option<f32>,
    pub weight: Option<f32>,
    pub price: Option<f32>,
    pub layer_height: Option<f32>,
}

#[derive(Clone, Debug)]
pub struct Model {
    pub ir: Vec<TimedIR>,
    pub model_meta: GlobalPrintingMeta,

    pub working_dir: Arc<TempDir>,
    pub model_preview: Option<PathBuf>,
}

impl Model {
    /// Create new model
    pub fn new(
        ir: Vec<TimedIR>,
        model_meta: GlobalPrintingMeta,
        working_dir: Arc<TempDir>,
        model_preview: Option<PathBuf>,
    ) -> Self {
        let mut m = Self {
            ir,
            model_meta,
            working_dir,
            model_preview,
        };

        m.calc_estimated();
        m
    }

    /// Fulfill all internal fields and calculate estimated remaining time
    pub fn calc_estimated(&mut self) {
        let mut pos = 0.0;
        for m in self.ir.iter_mut() {
            if matches!(m.ir, PrintingIR::Home) {
                pos = 0.0;
            }

            if let PrintingIR::MoveZ(mov) = &mut m.ir {
                mov._last_pos = pos;

                match mov.positioning {
                    StepperPositioning::Absolute => pos = mov.pos,
                    StepperPositioning::Relative => pos += mov.pos,
                }
            }
        }
        let mut duration = Duration::ZERO;

        for x in self.ir.iter_mut().rev() {
            duration += x.calc_command_duration();
            x.estimated_remaining = duration;
        }

        self.model_meta.estimated_printing_time = duration.as_secs() as usize;
    }
}
