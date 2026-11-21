use crate::types::model::Model;
use core::fmt;
use std::{error::Error, sync::Arc, time::Instant};

/// Command for controlling printer manager
pub enum PrinterCommand {
    StartPrint(Arc<Model>),
    Pause,
    Resume,
    Abort,
    Home,
    DisableStepper,
    MoveTo { pos: f64, speed: f64 },
}

/// Global printer state
#[derive(Clone, Debug, Default)]
pub enum PrinterState {
    #[default]
    Idle,
    Printing(PrintingTaskMeta),
    Paused(PrintingTaskMeta),
    Pausing(PrintingTaskMeta),
    Error(PrintingError),
    Busy,
    Aborted,
    Finished {
        total_elapsed: Instant,
    },
}

pub enum PrinterTaskCommand {
    Pause,
    Resume,
}

impl PrinterState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Printing(_) => "printing",
            Self::Paused(_) => "paused",
            Self::Pausing(_) => "pausing...",
            Self::Error(_) => "error",
            Self::Busy => "busy",
            Self::Aborted => "aborted",
            Self::Finished { total_elapsed: _ } => "finished",
        }
    }

    /// Check if printer state is busy or printing
    pub fn is_busy(&self) -> bool {
        matches!(
            self,
            PrinterState::Printing(_) | PrinterState::Paused(_) | PrinterState::Busy
        )
    }
}

/// Current print state metadata
#[derive(Clone, Debug)]
pub struct PrintingTaskMeta {
    pub printing_layer: usize,
    pub current_ir_index: usize,
    pub current_ir_elapsed: Instant,
    pub total_elapsed: Instant,
    pub model: Arc<Model>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PrintingError {
    message: String,
}

impl PrintingError {
    /// Create new printing error instance
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for PrintingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "An error occurred due model printing: {}", self.message)
    }
}

impl Error for PrintingError {}
